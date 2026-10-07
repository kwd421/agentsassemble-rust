//! One event-driven delivery owner. Network completion cannot ACK a newer event.
use super::{
    Arc, CancellationToken, CentralDirectoryError, CentralDirectoryInner, CentralHostIdentity,
    Duration, Method, PublicIngress, SqliteStore, Utc, json, member_sync, publish_default_name,
    reconcile_demotion, retry_delay, send_signed,
};
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

struct NamePublication {
    published: Option<(String, String)>,
    parked: Option<(String, i64)>,
    dirty: bool,
    failures: u32,
    due: i64,
}
impl Default for NamePublication {
    fn default() -> Self {
        Self {
            published: None,
            parked: None,
            dirty: true,
            failures: 0,
            due: 0,
        }
    }
}

async fn finish_delivery(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    clock: &Clock,
    name: &mut NamePublication,
    delivered: Delivered,
) -> Result<(), CentralDirectoryError> {
    match delivered {
        Delivered::Endpoint(event, result) => {
            if store
                .finish_central_endpoint_event(&event, result.is_ok())
                .await?
            {
                match result {
                    Ok(()) => inner.status.write().published(&event.origin),
                    Err(error) => inner.status.write().last_error = error.to_string(),
                }
            }
        }
        Delivered::Name(result, parked) => {
            name.parked = parked;
            match result {
                Ok(published) => {
                    name.published = published;
                    name.failures = 0;
                    inner.status.write().name_sync_error.clear();
                }
                Err(error) => {
                    name.dirty |= !matches!(error, CentralDirectoryError::Rejected);
                    name.failures = name.failures.saturating_add(1);
                    name.due = clock.now().saturating_add(
                        i64::try_from(retry_delay(name.failures).as_millis()).unwrap_or(i64::MAX),
                    );
                    inner.status.write().name_sync_error = error.to_string();
                }
            }
        }
        Delivered::Member(result) => {
            if let Err(error) = result {
                inner.status.write().last_error = error.to_string();
            }
        }
    }
    Ok(())
}

pub(super) async fn run(
    inner: Arc<CentralDirectoryInner>,
    store: SqliteStore,
    ingress: PublicIngress,
    identity: CentralHostIdentity,
    cancellation: CancellationToken,
) -> Result<(), CentralDirectoryError> {
    let clock = Clock::new();
    let mut directory_events = store.subscribe_room_directory();
    let mut endpoints = ingress.subscribe_endpoint_changes();
    let mut observed = None;
    let mut name = NamePublication {
        due: clock.now(),
        ..NamePublication::default()
    };
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
            flight = start_delivery(
                &inner,
                &store,
                &identity,
                &mut name,
                pending,
                member_due,
                clock.now(),
            );
        }
        let due = if flight.is_none() {
            [endpoint_due, name.dirty.then_some(name.due), member_due]
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
            notification = endpoints.changed() => { if notification.is_err() { break; } },
            notification = directory_events.changed() => { if notification.is_err() { break; } name.dirty = true; },
            delivered = async { match flight.as_mut() { Some(flight) => flight.await, None => std::future::pending().await } } => {
                flight = None;
                let delivered = delivered.map_err(|_| CentralDirectoryError::Unavailable)?;
                finish_delivery(&inner, &store, &clock, &mut name, delivered).await?;
            },
            () = wait => {},
        }
    }
    // Unknown in-flight network outcomes are fenced by the newer fixed offline generation.
    if let Some(flight) = flight {
        flight.abort();
        let _ = flight.await;
    }
    publish_offline(&inner, &store, &ingress, &identity, &clock).await
}

async fn publish_offline(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    ingress: &PublicIngress,
    identity: &CentralHostIdentity,
    clock: &Clock,
) -> Result<(), CentralDirectoryError> {
    if reconcile_demotion(store, ingress).await? {
        return Ok(());
    }
    if let Some(epoch) = store.registration_epoch().await? {
        let event = store
            .reserve_central_endpoint_event(&epoch, "", clock.now())
            .await?;
        let result = publish(inner, store, identity, &event).await;
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

fn start_delivery(
    inner: &Arc<CentralDirectoryInner>,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
    name: &mut NamePublication,
    pending: Option<CentralEndpointEvent>,
    member_due: Option<i64>,
    now: i64,
) -> Option<AbortOnDropHandle<Delivered>> {
    let endpoint_due = pending
        .as_ref()
        .and_then(CentralEndpointEvent::next_attempt_at_ms);
    let (inner, store, identity) = (inner.clone(), store.clone(), identity.clone());
    if endpoint_due.is_some_and(|due| due <= now) {
        if let Some(event) = pending {
            return Some(AbortOnDropHandle::new(tokio::spawn(async move {
                let result = publish(&inner, &store, &identity, &event).await;
                Delivered::Endpoint(event, result)
            })));
        }
    } else if name.dirty && name.due <= now {
        name.dirty = false;
        let published = name.published.clone();
        let mut parked = name.parked.clone();
        return Some(AbortOnDropHandle::new(tokio::spawn(async move {
            let result =
                publish_default_name(&inner, &store, &identity, published.as_ref(), &mut parked)
                    .await;
            Delivered::Name(result, parked)
        })));
    } else if member_due.is_some_and(|due| due <= now) {
        let now = now.div_euclid(1000);
        return Some(AbortOnDropHandle::new(tokio::spawn(async move {
            Delivered::Member(member_sync::send(&inner, &store, &identity, now).await)
        })));
    }
    None
}
