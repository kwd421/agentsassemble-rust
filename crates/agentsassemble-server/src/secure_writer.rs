//! AEAD sequence owner; body credit retains queue charges until consumption.
use crate::secure_queue::{Budget, Feedback, Queued};
use axum::extract::ws::Message;
use futures_util::SinkExt;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

struct Retired {
    deadline: Instant,
    consumed: u64,
    records: VecDeque<Queued>,
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
    let mut sequence = 0;
    let mut headers = HashSet::new();
    let mut controls: VecDeque<(u64, Instant, Option<Queued>)> = VecDeque::new();
    let mut pending: VecDeque<Queued> = VecDeque::new();
    let mut bodies: HashMap<u32, (u64, Instant, VecDeque<Queued>)> = HashMap::new();

    let mut retired: HashMap<u32, Retired> = HashMap::new();
    let mut last_credit = Instant::now();
    let mut last_credit_id = None;
    let mut input_done = false;
    loop {
        pending.retain(|item| !item.cancel.is_cancelled());
        for (_, _, item) in &mut controls {
            if item.as_ref().is_some_and(|r| r.cancel.is_cancelled()) {
                item.take();
            }
        }
        // Cancelled records release charges even while another record awaits ACK.
        if feedback.is_empty() {
            for (_, _, records) in bodies.values_mut() {
                records.retain(|item| !item.cancel.is_cancelled());
            }
        }
        bodies.retain(|_, (_, _, records)| !records.is_empty());
        if let Some(index) = pending.iter().position(|item| {
            (controls.is_empty() || !item.ack)
                && (item.window.is_none() || item.http.is_none_or(|id| headers.contains(&id)))
        }) {
            let item = pending.remove(index).expect("queued");
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
                        last_credit_id = None;
                    }
                    bodies
                        .entry(*id)
                        .or_insert_with(|| {
                            (
                                *offset - (item.bytes.len() - 18) as u64,
                                Instant::now(),
                                VecDeque::new(),
                            )
                        })
                        .2
                        .push_back(item);
                }
            } else if item.ack {
                controls.push_back((sequence, Instant::now(), Some(item)));
            } else if let Some(id) = item.retire {
                retired.get_mut(&id).expect("cancel receipt").notice = Some(item);
            }
            sequence += 1;
            // Read feedback before emitting the next queued record.
        }
        if input_done && pending.is_empty() && controls.is_empty() && bodies.is_empty() {
            break;
        }
        let cancellations = pending
            .iter()
            .chain(bodies.values().flat_map(|(_, _, r)| r.iter()))
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
            .chain(
                bodies
                    .values()
                    .map(|(_, at, _)| *at + crate::http_body_policy::IDLE),
            )
            .chain(retired.values().map(|receipt| receipt.deadline))
            .min()
            .unwrap_or_else(|| Instant::now() + crate::http_body_policy::IDLE);
        let expirations = bodies
            .values()
            .filter_map(|(_, _, r)| r.front().and_then(|r| r.progress.clone()))
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
            frame = feedback.recv() => match frame {
                Some(Feedback::Cancel { id, receipt }) => {
                    let receipt = receipt || headers.contains(&id) || bodies.contains_key(&id) || pending.iter().any(|item| item.http == Some(id) && (item.header || item.window.is_some()));
                    headers.remove(&id);
                    let (consumed, _, records) = bodies.remove(&id).unwrap_or_else(|| (0, Instant::now(), VecDeque::new()));
                    for item in &records { item.cancel.cancel(); }
                    pending.retain(|item| { if item.http == Some(id) || item.window.as_ref().is_some_and(|r| r.0 == id) { item.cancel.cancel(); false } else { true } });
                    for (_, _, item) in &mut controls { if item.as_ref().is_some_and(|r| r.http == Some(id)) { item.take(); } }
                    if receipt {
                        if retired.contains_key(&id) { continue; }
                        let bytes = serde_json::to_vec(&serde_json::json!({"op":"cancelled","id":id})).expect("control");
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
                    let Some((consumed, at, records)) = bodies.get_mut(&id) else {
                        if let Some(receipt) = retired.get_mut(&id) {
                            if offset <= receipt.consumed || !receipt.records.iter().any(|r| r.window.as_ref().expect("window").1 == offset) { break; }
                            receipt.consumed = offset;
                            while receipt.records.front().is_some_and(|r| r.window.as_ref().expect("window").1 <= offset) { receipt.records.pop_front(); }
                            continue;
                        }
                        break;
                    };
                    if offset <= *consumed || records.back().is_none_or(|r| offset > r.window.as_ref().expect("window").1) { break; }
                    // Credits acknowledge whole records pulled by the virtual-body consumer.
                    if !records.iter().any(|r| r.window.as_ref().expect("window").1 == offset) { break; }
                    if let Some(progress) = records.front().and_then(|r| r.progress.as_ref()) { progress.accept((offset - *consumed) as usize); }
                    while records.front().is_some_and(|r| r.window.as_ref().expect("window").1 <= offset) { records.pop_front(); }
                    *consumed = offset; *at = Instant::now(); last_credit = *at; last_credit_id = Some(id);
                }
                None => break,
            },
            () = async { tokio::select! { () = tokio::time::sleep_until(deadline) => {}, () = expired => {} } }, if !controls.is_empty() || !bodies.is_empty() || !retired.is_empty() => {
                if retired.values().any(|receipt| receipt.deadline <= Instant::now()) { break; }
                let stalled = bodies.iter().filter(|(_, (_, at, r))| at.elapsed() >= crate::http_body_policy::IDLE || r.front().and_then(|r| r.progress.as_ref()).is_some_and(|p| p.check().is_err())).map(|(id, _)| *id).collect::<Vec<_>>();
                let all_idle = bodies.values().all(|(_, at, r)| at.elapsed() >= crate::http_body_policy::IDLE || r.front().and_then(|r| r.progress.as_ref()).is_some_and(|p| p.idle()));
                if controls.front().is_some_and(|(_, at, _)| at.elapsed() >= crate::http_body_policy::IDLE)
                    || (!bodies.is_empty() && (last_credit.elapsed() >= crate::http_body_policy::IDLE || (all_idle && last_credit_id.is_none_or(|id| stalled.contains(&id))))) { break; }
                for id in stalled {
                    headers.remove(&id);
                    let (consumed, _, records) = bodies.remove(&id).expect("stalled body");
                    for item in &records { item.cancel.cancel(); }
                    retired.insert(id, Retired { deadline: Instant::now() + crate::http_body_policy::IDLE, consumed, records, notice: None });
                    let bytes = serde_json::to_vec(&serde_json::json!({"op":"error","id":id,"code":"response_timeout"})).expect("control");
                    let Ok(charge) = budget.charge(bytes.len() + 24) else { closed.cancel(); break; };
                    pending.push_back(Queued { http: None, header: false, ack: true, retire: Some(id), bytes, _charge: charge, window: None, cancel: closed.clone(), progress: None });
                }
            },
            item = outgoing.recv(), if !input_done => {
                match item {
                    Some(mut item) => {
                        if let Some(id) = item.retire {
                            headers.remove(&id);
                            if let Some((consumed, _, records)) = bodies.remove(&id) {
                                for record in &records { record.cancel.cancel(); }
                                retired.insert(id, Retired { deadline: Instant::now() + crate::http_body_policy::IDLE, consumed, records, notice: None });
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
