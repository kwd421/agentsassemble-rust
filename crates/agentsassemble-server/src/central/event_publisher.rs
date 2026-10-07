//! One event-driven delivery owner. Network completion cannot ACK a newer event.
use super::*;
use agentsassemble_persistence::CentralEndpointEvent;
use tokio_util::task::AbortOnDropHandle;

struct Clock {
    wall_ms: i64,
    monotonic: tokio::time::Instant,
}
impl Clock {
    fn new() -> Self {
        Self {
            wall_ms: Utc::now().timestamp_millis(),
            monotonic: tokio::time::Instant::now(),
        }
    }
    fn now(&self) -> i64 {
        self.wall_ms
            .saturating_add(i64::try_from(self.monotonic.elapsed().as_millis()).unwrap_or(i64::MAX))
    }
}

enum Delivered {
    Endpoint(CentralEndpointEvent, Result<(), CentralDirectoryError>),
    Name(
        Result<Option<(String, String)>, CentralDirectoryError>,
        Option<(String, i64)>,
    ),
    Member(Result<(), CentralDirectoryError>),
}

pub(super) async fn run(
    inner: Arc<CentralDirectoryInner>,
    store: SqliteStore,
    ingress: PublicIngress,
    identity: CentralHostIdentity,
    cancellation: CancellationToken,
) -> Result<(), CentralDirectoryError> {
    let clock = Clock::new();
    let mut changes = store.subscribe_room_directory();
    let mut endpoints = ingress.subscribe_endpoint_changes();
    let mut observed = None;
    let mut published_name = None;
    let mut parked_name = None;
    let mut name_dirty = true;
    let mut name_failures = 0_u32;
    let mut name_due = clock.now();
    let mut flight: Option<AbortOnDropHandle<Delivered>> = None;
    loop {
        #[cfg(test)]
        inner.publisher_probe.send_modify(|probe| {
            probe.0 += 1;
            probe.1 = false;
        });
        if reconcile_demotion(&store, &ingress).await? {
            inner.status.write().registered_origin.clear();
            return Ok(());
        }
        let epoch = store.registration_epoch().await?;
        let endpoint = endpoints.borrow().clone();
        let desired = (epoch.clone(), endpoint);
        if observed.as_ref() != Some(&desired) {
            if let Some(epoch) = &epoch {
                store
                    .reserve_central_endpoint_event(epoch, &desired.1.1, clock.now())
                    .await?;
                inner.status.write().last_error.clear();
            }
            observed = Some(desired);
        }
        let pending = store
            .pending_central_endpoint_event()
            .await?
            .filter(|event| Some(&event.registration_epoch) == epoch.as_ref());
        let endpoint_due = pending
            .as_ref()
            .and_then(CentralEndpointEvent::next_attempt_at_ms);
        let member_due = store
            .next_member_projection_attempt(clock.now().div_euclid(1000))
            .await?
            .map(|due| due.saturating_mul(1000));
        if flight.is_none() {
            let (inner, store, identity) = (inner.clone(), store.clone(), identity.clone());
            if endpoint_due.is_some_and(|due| due <= clock.now()) {
                if let Some(event) = pending {
                    flight = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                        let result = publish(&inner, &store, &identity, &event).await;
                        Delivered::Endpoint(event, result)
                    })));
                }
            } else if name_dirty && name_due <= clock.now() {
                name_dirty = false;
                let published = published_name.clone();
                let mut parked = parked_name.clone();
                flight = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                    let result = publish_default_name(
                        &inner,
                        &store,
                        &identity,
                        published.as_ref(),
                        &mut parked,
                    )
                    .await;
                    Delivered::Name(result, parked)
                })));
            } else if member_due.is_some_and(|due| due <= clock.now()) {
                let now = clock.now().div_euclid(1000);
                flight = Some(AbortOnDropHandle::new(tokio::spawn(async move {
                    Delivered::Member(member_sync::send(&inner, &store, &identity, now).await)
                })));
            }
        }
        let due = if flight.is_none() {
            [endpoint_due, name_dirty.then_some(name_due), member_due]
                .into_iter()
                .flatten()
                .min()
        } else {
            None
        };
        #[cfg(test)]
        inner.publisher_probe.send_modify(|probe| {
            probe.1 = flight.is_none();
        });
        let wait = async {
            if let Some(due) = due {
                tokio::time::sleep(Duration::from_millis(
                    due.saturating_sub(clock.now()).max(0).cast_unsigned(),
                ))
                .await;
            } else {
                std::future::pending::<()>().await;
            }
        };
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            changed = endpoints.changed() => { if changed.is_err() { break; } },
            changed = changes.changed() => { if changed.is_err() { break; } name_dirty = true; },
            delivered = async { match flight.as_mut() { Some(flight) => flight.await, None => std::future::pending().await } } => {
                flight = None;
                let delivered = delivered.map_err(|_| CentralDirectoryError::Unavailable)?;
                match delivered {
                    Delivered::Endpoint(event, result) => {
                        if store.finish_central_endpoint_event(&event, result.is_ok()).await? {
                            match result { Ok(()) => inner.status.write().published(&event.origin), Err(error) => inner.status.write().last_error = error.to_string() }
                        }
                    }
                    Delivered::Name(result, parked) => {
                        parked_name = parked;
                        match result {
                            Ok(published) => { published_name = published; name_failures = 0; inner.status.write().name_sync_error.clear(); },
                            Err(error) => { name_dirty |= !matches!(error, CentralDirectoryError::Rejected); name_failures = name_failures.saturating_add(1); name_due = clock.now().saturating_add(i64::try_from(retry_delay(name_failures).as_millis()).unwrap_or(i64::MAX)); inner.status.write().name_sync_error = error.to_string(); }
                        }
                    }
                    Delivered::Member(result) => { if let Err(error) = result { inner.status.write().last_error = error.to_string(); } }
                }
            },
            () = wait => {},
        }
    }
    // Unknown in-flight network outcomes are fenced by the newer fixed offline generation.
    if let Some(flight) = flight {
        flight.abort();
        let _ = flight.await;
    }
    if reconcile_demotion(&store, &ingress).await? {
        return Ok(());
    }
    if let Some(epoch) = store.registration_epoch().await? {
        let event = store
            .reserve_central_endpoint_event(&epoch, "", clock.now())
            .await?;
        let result = publish(&inner, &store, &identity, &event).await;
        store
            .finish_central_endpoint_event(&event, result.is_ok())
            .await?;
        if let Err(error) = &result {
            inner.status.write().last_error = error.to_string();
        }
        return result;
    }
    Ok(())
}

pub(super) async fn publish(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
    event: &CentralEndpointEvent,
) -> Result<(), CentralDirectoryError> {
    let body = json!({"protocol":"secure_admission_v1", "mode":"event_secure_v1", "registration_epoch":event.registration_epoch,
        "origin":event.origin,"generation":event.generation,"issued_at":Utc::now().timestamp()});
    let path = format!("/v1/servers/{}/endpoint", identity.server_id());
    send_signed(
        inner,
        identity,
        store,
        if event.online() {
            Method::PUT
        } else {
            Method::DELETE
        },
        &path,
        body,
    )
    .await?;
    Ok(())
}

#[cfg(test)]
#[path = "event_publisher_tests.rs"]
mod tests;
