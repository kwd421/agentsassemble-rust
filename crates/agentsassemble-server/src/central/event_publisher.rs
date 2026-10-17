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
    identity: &CentralHostIdentity,
) -> Result<(), CentralDirectoryError> {
    match delivered {
        Delivered::Endpoint(event, result) => {
            if store
                .finish_central_endpoint_event(&event, result.is_ok())
                .await?
            {
                match result {
                    Ok(()) => {
                        store
                            .acknowledge_restored_endpoint(&event, identity.fingerprint())
                            .await?;
                        inner.status.write().published(&event.origin);
                    }
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
                    let backoff =
                        i64::try_from(retry_delay(name.failures).as_millis()).unwrap_or(i64::MAX);
                    // A quota park or Retry-After gate outlives the short backoff;
                    // the delivery loop sends nothing until the gate lifts.
                    name.due = clock
                        .now()
                        .saturating_add(backoff)
                        .max(inner.delivery_gate_ms(clock.now()));
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
            abort_delivery(flight.take()).await;
            if !wait_for_registration_resume(
                &inner,
                &store,
                &ingress,
                &mut directory_events,
                &mut endpoints,
                &cancellation,
            )
            .await?
            {
                return Ok(());
            }
            observed = None;
            name = NamePublication {
                due: clock.now(),
                ..NamePublication::default()
            };
            continue;
        }
        let epoch = store.registration_epoch().await?;
        let endpoint = endpoints.borrow().clone();
        let desired = (epoch.clone(), endpoint);
        if observed.as_ref() != Some(&desired) {
            if let Some(epoch) = &epoch {
                if should_reserve_endpoint_event(observed.as_ref(), epoch, &desired.1.1) {
                    store
                        .reserve_central_endpoint_event(epoch, &desired.1.1, clock.now())
                        .await?;
                    inner.status.write().last_error.clear();
                } else if observed.is_none() {
                    drop_superseded_stale_online_event(&store, epoch).await?;
                }
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
            // While a quota park or Retry-After gate holds, earlier dues wait
            // for the gate instead of polling central; the gate itself wakes
            // the loop once for the single bounded attempt afterwards.
            let now = clock.now();
            let gate = inner.delivery_gate_ms(now);
            let mut candidates = Vec::new();
            if gate > now {
                candidates.push(gate);
            }
            for candidate in
                [endpoint_due, name.dirty.then_some(name.due), member_due]
                    .into_iter()
                    .flatten()
            {
                candidates.push(candidate.max(gate));
            }
            candidates.into_iter().min()
        } else {
            None
        };
        #[cfg(test)]
        inner.publisher_probe.send_modify(|probe| {
            probe.1 = flight.is_none();
        });
        tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            notification = endpoints.changed() => { if notification.is_err() { break; } },
            notification = directory_events.changed() => { if notification.is_err() { break; } name.dirty = true; },
            delivered = async { match flight.as_mut() { Some(flight) => flight.await, None => std::future::pending().await } } => {
                flight = None;
                let delivered = delivered.map_err(|_| CentralDirectoryError::Unavailable)?;
                finish_delivery(&inner, &store, &clock, &mut name, delivered, &identity).await?;
            },
            () = wait_for_delivery(due, &clock) => {},
        }
    }
    // Unknown in-flight network outcomes are fenced by the exact pending
    // compare-and-clear plus the durable-monotonic generation: a stale
    // response cannot clear or park a newer event, and the next run reserves
    // strictly above every prior reservation. No offline event is published
    // on quit: the dead origin is liveness, revoked implicitly when the
    // restart PUT replaces the row; see should_reserve_endpoint_event.
    abort_delivery(flight).await;
    Ok(())
}

/// Whether an observed endpoint change reserves a new central event.
///
/// A live origin always reserves. An empty origin reserves only when it
/// revokes a same-epoch origin this run already observed (the owner
/// explicitly closed external access while the runtime lives). A cold start,
/// a fresh epoch, or a repeat empty carries no revocation and reserves
/// nothing, so a restart costs the single new-origin event instead of an
/// offline/online pair.
fn should_reserve_endpoint_event(
    observed: Option<&(Option<String>, (u64, String))>,
    epoch: &str,
    origin: &str,
) -> bool {
    if !origin.is_empty() {
        return true;
    }
    observed.is_some_and(|previous| {
        previous.0.as_deref() == Some(epoch) && !previous.1.1.is_empty()
    })
}

/// A fresh publisher run starts with no live origin when the channel still
/// holds its boot value. An undelivered pre-quit PUT names a dead
/// quick-tunnel origin, so it is superseded locally instead of published;
/// the restart then costs nothing until the tunnel (or an explicit stop)
/// produces an actual event. A pending offline revocation is never dropped:
/// the owner explicitly closed external access and central must hear it.
async fn drop_superseded_stale_online_event(
    store: &SqliteStore,
    epoch: &str,
) -> Result<(), CentralDirectoryError> {
    if let Some(stale) = store
        .pending_central_endpoint_event()
        .await?
        .filter(|pending| pending.online() && pending.registration_epoch == epoch)
    {
        store.finish_central_endpoint_event(&stale, true).await?;
    }
    Ok(())
}

async fn abort_delivery(flight: Option<AbortOnDropHandle<Delivered>>) {
    if let Some(flight) = flight {
        flight.abort();
        let _ = flight.await;
    }
}
async fn wait_for_delivery(due: Option<i64>, clock: &Clock) {
    if let Some(due) = due {
        tokio::time::sleep(Duration::from_millis(
            due.saturating_sub(clock.now()).max(0).cast_unsigned(),
        ))
        .await;
    } else {
        std::future::pending::<()>().await;
    }
}

async fn wait_for_registration_resume(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    ingress: &PublicIngress,
    directory: &mut tokio::sync::watch::Receiver<()>,
    endpoints: &mut tokio::sync::watch::Receiver<(u64, String)>,
    cancellation: &CancellationToken,
) -> Result<bool, CentralDirectoryError> {
    #[cfg(not(test))]
    let _ = inner;
    loop {
        directory.borrow_and_update();
        endpoints.borrow_and_update();
        match store.hosting_restriction().await?.as_deref() {
            Some("account_deleted") => {}
            None if ingress.restricted() => {} // CAS commits before ingress resume notification
            None => return Ok(true),
            Some(_) => return Ok(false),
        }
        #[cfg(test)]
        inner.publisher_probe.send_modify(|probe| {
            probe.1 = true;
        });
        tokio::select! {
            () = cancellation.cancelled() => return Ok(false),
            result = directory.changed() => if result.is_err() { return Ok(false); },
            result = endpoints.changed() => if result.is_err() { return Ok(false); },
        }
    }
}

pub(super) async fn publish(
    inner: &CentralDirectoryInner,
    store: &SqliteStore,
    identity: &CentralHostIdentity,
    event: &CentralEndpointEvent,
) -> Result<(), CentralDirectoryError> {
    let body = json!({"protocol":"secure_admission_v1", "mode":"event_secure_v1", "account_deletion_protocol":"v1", "registration_epoch":event.registration_epoch,
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
    if now < inner.delivery_gate_ms(now) {
        return None;
    }
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
