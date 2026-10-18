//! AEAD sequence owner; body credit retains queue charges until consumption.
use crate::secure_queue::{Budget, Feedback, Queued};
use axum::extract::ws::Message;
use futures_util::SinkExt;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

/// WebSocket control keepalive for every open secure channel connection.
/// Cloudflare-proxied idle channels close near ~100 s without frames;
/// a control Ping every 30 s keeps the tunnel alive. Browsers answer Ping
/// with Pong automatically, so there is no client wire or protocol change
/// and no version skew (an old client still works; an old host never pings).
/// Ping is a control frame and never interleaves with encrypted records:
/// the single writer task sends it sequentially on the same sink.
pub(crate) const SECURE_CHANNEL_KEEPALIVE: Duration = Duration::from_secs(30);

struct Emitted {
    at: Instant,
    item: Queued,
}
struct Body {
    consumed: u64,
    records: VecDeque<Emitted>,
}
struct Retired {
    deadline: Instant,
    consumed: u64,
    records: VecDeque<Emitted>,
    notice: Option<Queued>,
}

pub(crate) async fn write_records<S: futures_util::Sink<Message> + Unpin>(
    mut sender: S,
    mut seal: impl FnMut(&[u8]) -> Result<Vec<u8>, ()>,
    mut outgoing: mpsc::Receiver<Queued>,
    mut feedback: mpsc::Receiver<Feedback>,
    closed: CancellationToken,
    budget: Budget,
) {
    let _close_on_exit = closed.clone().drop_guard();
    let mut sequence = 0_u64;
    let mut headers = HashSet::new();
    let mut controls: VecDeque<(u64, Instant, Option<Queued>)> = VecDeque::new();
    let mut pending: VecDeque<Queued> = VecDeque::new();
    let mut bodies: HashMap<u32, Body> = HashMap::new();

    let mut retired: HashMap<u32, Retired> = HashMap::new();
    // This only selects channel closure versus an isolated body error; body
    // deadlines always come from their own oldest emitted record.
    let mut last_credit = Instant::now();
    let mut input_done = false;
    // Idle keepalive owns no sequence or budget: the single writer sends it
    // sequentially on the same sink, so it never interleaves with encrypted
    // records. It stops with the connection via the closed token below.
    let mut keepalive = tokio::time::interval_at(
        Instant::now() + SECURE_CHANNEL_KEEPALIVE,
        SECURE_CHANNEL_KEEPALIVE,
    );
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        pending.retain(|item| !item.cancel.is_cancelled());
        for (_, _, item) in &mut controls {
            if item.as_ref().is_some_and(|r| r.cancel.is_cancelled()) {
                item.take();
            }
        }
        // Cancelled records release charges even while another record awaits ACK.
        if feedback.is_empty() {
            for body in bodies.values_mut() {
                body.records
                    .retain(|record| !record.item.cancel.is_cancelled());
            }
        }
        bodies.retain(|_, body| !body.records.is_empty());
        if let Some(index) = pending.iter().position(|item| {
            (controls.is_empty() || !item.ack)
                && (item.window.is_none() || item.http.is_none_or(|id| headers.contains(&id)))
        }) {
            let Some(item) = pending.remove(index) else {
                break;
            };
            let Ok(record) = seal(&item.bytes) else {
                break;
            };
            let sent = tokio::select! {
                () = closed.cancelled() => break,
                sent = tokio::time::timeout(crate::http_body_policy::IDLE, sender.send(Message::Binary(record.into()))) => sent,
            };
            if !matches!(sent, Ok(Ok(()))) {
                break;
            }
            if item.header {
                if let Some(id) = item.http {
                    headers.insert(id);
                }
            }
            if let Some((id, offset, _, _)) = &item.window {
                if item.bytes.len() == 18 {
                    headers.remove(id);
                }
                if *offset > 0 && item.bytes.len() > 18 {
                    if bodies.is_empty() {
                        last_credit = Instant::now();
                    }
                    let length = item.bytes.len() - 18;
                    let Some(consumed) = offset.checked_sub(length as u64) else {
                        break;
                    };
                    bodies
                        .entry(*id)
                        .or_insert_with(|| {
                            if let Some(progress) = &item.progress {
                                progress.emitted();
                            }
                            Body {
                                consumed,
                                records: VecDeque::new(),
                            }
                        })
                        .records
                        .push_back(Emitted {
                            at: Instant::now(),
                            item,
                        });
                }
            } else if item.ack {
                controls.push_back((sequence, Instant::now(), Some(item)));
            } else if let Some(id) = item.retire {
                let Some(receipt) = retired.get_mut(&id) else {
                    break;
                };
                receipt.notice = Some(item);
            }
            let Some(next) = sequence.checked_add(1) else {
                break;
            };
            sequence = next;
            // Read feedback before emitting the next queued record.
        }
        if input_done
            && pending.is_empty()
            && controls.is_empty()
            && bodies.is_empty()
            && retired.is_empty()
        {
            break;
        }
        let cancellations = pending
            .iter()
            .chain(
                bodies
                    .values()
                    .flat_map(|body| body.records.iter().map(|record| &record.item)),
            )
            .chain(controls.iter().filter_map(|(_, _, r)| r.as_ref()))
            .filter(|item| !item.cancel.is_cancelled())
            .map(|item| Box::pin(item.cancel.clone().cancelled_owned()))
            .collect::<Vec<_>>();
        let cancelled = async {
            if cancellations.is_empty() {
                std::future::pending::<()>().await;
            } else {
                futures_util::future::select_all(cancellations).await;
            }
        };
        let deadline = controls
            .front()
            .map(|(_, at, _)| *at + crate::http_body_policy::IDLE)
            .into_iter()
            .chain(bodies.values().filter_map(|body| {
                body.records
                    .front()
                    .map(|record| record.at + crate::http_body_policy::IDLE)
            }))
            .chain(retired.values().map(|receipt| receipt.deadline))
            .min()
            .unwrap_or_else(|| Instant::now() + crate::http_body_policy::IDLE);
        let expirations = bodies
            .values()
            .filter_map(|body| {
                body.records
                    .front()
                    .and_then(|record| record.item.progress.clone())
            })
            .map(|progress| Box::pin(async move { progress.expired().await }))
            .collect::<Vec<_>>();
        let expired = async {
            if expirations.is_empty() {
                std::future::pending::<()>().await;
            } else {
                futures_util::future::select_all(expirations).await;
            }
        };
        tokio::select! {
            biased;
            () = closed.cancelled() => break,
            () = cancelled => {},
            _ = keepalive.tick() => {
                let sent = tokio::select! {
                    () = closed.cancelled() => break,
                    sent = tokio::time::timeout(
                        crate::http_body_policy::IDLE,
                        sender.send(Message::Ping(axum::body::Bytes::new())),
                    ) => sent,
                };
                if !matches!(sent, Ok(Ok(()))) {
                    break;
                }
            }
            frame = feedback.recv() => match frame {
                Some(Feedback::Cancel { id, receipt }) => {
                    let receipt = receipt || headers.contains(&id) || bodies.contains_key(&id) || pending.iter().any(|item| item.http == Some(id) && (item.header || item.window.is_some()));
                    headers.remove(&id);
                    let body = bodies.remove(&id).unwrap_or_else(|| Body { consumed: 0, records: VecDeque::new() });
                    let Body { consumed, records } = body;
                    for record in &records { record.item.cancel.cancel(); }
                    pending.retain(|item| { if item.http == Some(id) || item.window.as_ref().is_some_and(|r| r.0 == id) { item.cancel.cancel(); false } else { true } });
                    for (_, _, item) in &mut controls { if item.as_ref().is_some_and(|r| r.http == Some(id)) { item.take(); } }
                    if receipt {
                        if retired.contains_key(&id) { continue; }
                        let Ok(bytes) = serde_json::to_vec(&serde_json::json!({"op":"cancelled","id":id})) else { break; };
                        let Ok(charge) = budget.charge(bytes.len() + 24) else { break; };
                        retired.insert(id, Retired { deadline: Instant::now() + crate::http_body_policy::IDLE, consumed, records, notice: None });
                        pending.push_back(Queued { http: None, header: false, ack: false, retire: Some(id), bytes, _charge: charge, window: None, cancel: closed.clone(), progress: None });
                    }
                }
                Some(Feedback::CancelConfirm(id)) => {
                    if !retired.get(&id).is_some_and(|r| r.notice.is_some()) { break; }
                    retired.remove(&id);
                }
                Some(Feedback::Ack(ack)) => {
                    let Some((expected, _, item)) = controls.pop_front() else { break; };
                    if expected != ack { break; }
                    if let Some(id) = item.and_then(|r| r.retire) { retired.remove(&id); }
                }
                Some(Feedback::Credit { id, offset }) => {
                    let Some(body) = bodies.get_mut(&id) else {
                        if let Some(receipt) = retired.get_mut(&id) {
                            if offset <= receipt.consumed || !receipt.records.iter().any(|r| r.item.window.as_ref().is_some_and(|window| window.1 == offset)) { break; }
                            receipt.consumed = offset;
                            while receipt.records.front().is_some_and(|r| r.item.window.as_ref().is_some_and(|window| window.1 <= offset)) { receipt.records.pop_front(); }
                            continue;
                        }
                        break;
                    };
                    // Credits acknowledge whole emitted records, never queued capacity.
                    if offset <= body.consumed || !body.records.iter().any(|r| r.item.window.as_ref().is_some_and(|window| window.1 == offset)) { break; }
                    if let Some(progress) = body.records.front().and_then(|r| r.item.progress.as_ref()) { progress.accept((offset - body.consumed) as usize); }
                    while body.records.front().is_some_and(|r| r.item.window.as_ref().is_some_and(|window| window.1 <= offset)) { body.records.pop_front(); }
                    body.consumed = offset;
                    last_credit = Instant::now();
                }
                None => break,
            },
            () = async { tokio::select! { () = tokio::time::sleep_until(deadline) => {}, () = expired => {} } }, if !controls.is_empty() || !bodies.is_empty() || !retired.is_empty() => {
                if retired.values().any(|receipt| receipt.deadline <= Instant::now()) { break; }
                let stalled = bodies.iter().filter(|(_, body)| body.records.front().is_some_and(|record|
                    record.at.elapsed() >= crate::http_body_policy::IDLE || record.item.progress.as_ref().is_some_and(|p| p.check().is_err())
                )).map(|(id, _)| *id).collect::<Vec<_>>();
                if controls.front().is_some_and(|(_, at, _)| at.elapsed() >= crate::http_body_policy::IDLE)
                    || (!bodies.is_empty() && last_credit.elapsed() >= crate::http_body_policy::IDLE) { break; }
                for id in stalled {
                    headers.remove(&id);
                    let Some(Body { consumed, records }) = bodies.remove(&id) else { continue; };
                    for record in &records { record.item.cancel.cancel(); }
                    retired.insert(id, Retired { deadline: Instant::now() + crate::http_body_policy::IDLE, consumed, records, notice: None });
                    let Ok(bytes) = serde_json::to_vec(&serde_json::json!({"op":"error","id":id,"code":"response_timeout"})) else { closed.cancel(); break; };
                    let Ok(charge) = budget.charge(bytes.len() + 24) else { closed.cancel(); break; };
                    pending.push_back(Queued { http: None, header: false, ack: true, retire: Some(id), bytes, _charge: charge, window: None, cancel: closed.clone(), progress: None });
                }
            },
            item = outgoing.recv(), if !input_done => {
                match item {
                    Some(mut item) => {
                        if let Some(id) = item.retire {
                            headers.remove(&id);
                            if let Some(Body { consumed, records }) = bodies.remove(&id) {
                                for record in &records { record.item.cancel.cancel(); }
                                retired.insert(id, Retired { deadline: Instant::now() + crate::http_body_policy::IDLE, consumed, records, notice: None });
                            } else if !retired.contains_key(&id) {
                                // Nothing emitted remains charged: only the ordinary error ACK is due.
                                item.retire = None; item.ack = true;
                            }
                            pending.retain(|record| { if record.http == Some(id) { record.cancel.cancel(); false } else { true } });
                            item.cancel = closed.clone(); item.http = None;
                        }
                        pending.push_back(item);
                    }
                    None => input_done = true,
                }
            },
            () = std::future::ready(()), if pending.iter().any(|item| (controls.is_empty() || !item.ack) && (item.window.is_none() || item.http.is_none_or(|id| headers.contains(&id)))) => {},
        }
    }
    closed.cancel();
}
