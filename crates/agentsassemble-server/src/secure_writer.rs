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

/// The writer's bookkeeping between records: what is queued, what awaits an
/// ACK or body credit, and which cancelled requests still owe a receipt.
struct Writer {
    sequence: u64,
    headers: HashSet<u32>,
    controls: VecDeque<(u64, Instant, Option<Queued>)>,
    pending: VecDeque<Queued>,
    bodies: HashMap<u32, Body>,
    retired: HashMap<u32, Retired>,
    // This only selects channel closure versus an isolated body error; body
    // deadlines always come from their own oldest emitted record.
    last_credit: Instant,
    input_done: bool,
}

/// `Err` closes the channel.
type Flow = Result<(), ()>;

impl Writer {
    fn new() -> Self {
        Self {
            sequence: 0,
            headers: HashSet::new(),
            controls: VecDeque::new(),
            pending: VecDeque::new(),
            bodies: HashMap::new(),
            retired: HashMap::new(),
            last_credit: Instant::now(),
            input_done: false,
        }
    }

    fn sendable(&self, item: &Queued) -> bool {
        (self.controls.is_empty() || !item.ack)
            && (item.window.is_none() || item.http.is_none_or(|id| self.headers.contains(&id)))
    }

    fn finished(&self) -> bool {
        self.input_done
            && self.pending.is_empty()
            && self.controls.is_empty()
            && self.bodies.is_empty()
            && self.retired.is_empty()
    }

    fn waiting(&self) -> bool {
        !self.controls.is_empty() || !self.bodies.is_empty() || !self.retired.is_empty()
    }

    fn drop_cancelled(&mut self, feedback_empty: bool) {
        self.pending.retain(|item| !item.cancel.is_cancelled());
        for (_, _, item) in &mut self.controls {
            if item.as_ref().is_some_and(|r| r.cancel.is_cancelled()) {
                item.take();
            }
        }
        // Cancelled records release charges even while another record awaits ACK.
        if feedback_empty {
            for body in self.bodies.values_mut() {
                body.records
                    .retain(|record| !record.item.cancel.is_cancelled());
            }
        }
        self.bodies.retain(|_, body| !body.records.is_empty());
    }

    /// Bookkeeping for a record the sink accepted.
    fn emitted(&mut self, item: Queued) -> Flow {
        if item.header
            && let Some(id) = item.http
        {
            self.headers.insert(id);
        }
        if let Some((id, offset, _, _)) = &item.window {
            if item.bytes.len() == 18 {
                self.headers.remove(id);
            }
            if *offset > 0 && item.bytes.len() > 18 {
                if self.bodies.is_empty() {
                    self.last_credit = Instant::now();
                }
                let length = item.bytes.len() - 18;
                let consumed = offset.checked_sub(length as u64).ok_or(())?;
                self.bodies
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
            self.controls
                .push_back((self.sequence, Instant::now(), Some(item)));
        } else if let Some(id) = item.retire {
            let receipt = self.retired.get_mut(&id).ok_or(())?;
            receipt.notice = Some(item);
        }
        self.sequence = self.sequence.checked_add(1).ok_or(())?;
        Ok(())
    }

    /// The earliest moment an ACK, body credit or retirement receipt is overdue.
    fn deadline(&self) -> Instant {
        self.controls
            .front()
            .map(|(_, at, _)| *at + crate::http_body_policy::IDLE)
            .into_iter()
            .chain(self.bodies.values().filter_map(|body| {
                body.records
                    .front()
                    .map(|record| record.at + crate::http_body_policy::IDLE)
            }))
            .chain(self.retired.values().map(|receipt| receipt.deadline))
            .min()
            .unwrap_or_else(|| Instant::now() + crate::http_body_policy::IDLE)
    }

    fn feedback(&mut self, frame: &Feedback, budget: &Budget, closed: &CancellationToken) -> Flow {
        match *frame {
            Feedback::Cancel { id, receipt } => {
                let receipt = receipt
                    || self.headers.contains(&id)
                    || self.bodies.contains_key(&id)
                    || self.pending.iter().any(|item| {
                        item.http == Some(id) && (item.header || item.window.is_some())
                    });
                self.headers.remove(&id);
                let Body { consumed, records } = self.bodies.remove(&id).unwrap_or_else(|| Body {
                    consumed: 0,
                    records: VecDeque::new(),
                });
                for record in &records {
                    record.item.cancel.cancel();
                }
                self.pending.retain(|item| {
                    if item.http == Some(id) || item.window.as_ref().is_some_and(|r| r.0 == id) {
                        item.cancel.cancel();
                        false
                    } else {
                        true
                    }
                });
                for (_, _, item) in &mut self.controls {
                    if item.as_ref().is_some_and(|r| r.http == Some(id)) {
                        item.take();
                    }
                }
                if receipt && !self.retired.contains_key(&id) {
                    let bytes = serde_json::to_vec(&serde_json::json!({"op":"cancelled","id":id}))
                        .map_err(|_| ())?;
                    let charge = budget.charge(bytes.len() + 24)?;
                    self.retire(id, consumed, records);
                    self.pending.push_back(Queued {
                        http: None,
                        header: false,
                        ack: false,
                        retire: Some(id),
                        bytes,
                        _charge: charge,
                        window: None,
                        cancel: closed.clone(),
                        progress: None,
                    });
                }
            }
            Feedback::CancelConfirm(id) => {
                if self.retired.get(&id).is_none_or(|r| r.notice.is_none()) {
                    return Err(());
                }
                self.retired.remove(&id);
            }
            Feedback::Ack(ack) => {
                let (expected, _, item) = self.controls.pop_front().ok_or(())?;
                if expected != ack {
                    return Err(());
                }
                if let Some(id) = item.and_then(|r| r.retire) {
                    self.retired.remove(&id);
                }
            }
            Feedback::Credit { id, offset } => {
                let Some(body) = self.bodies.get_mut(&id) else {
                    let receipt = self.retired.get_mut(&id).ok_or(())?;
                    if offset <= receipt.consumed || !ends_record(&receipt.records, offset) {
                        return Err(());
                    }
                    receipt.consumed = offset;
                    drop_credited(&mut receipt.records, offset);
                    return Ok(());
                };
                // Credits acknowledge whole emitted records, never queued capacity.
                if offset <= body.consumed || !ends_record(&body.records, offset) {
                    return Err(());
                }
                if let Some(progress) = body.records.front().and_then(|r| r.item.progress.as_ref())
                {
                    progress.accept(usize::try_from(offset - body.consumed).unwrap_or(usize::MAX));
                }
                drop_credited(&mut body.records, offset);
                body.consumed = offset;
                self.last_credit = Instant::now();
            }
        }
        Ok(())
    }

    fn retire(&mut self, id: u32, consumed: u64, records: VecDeque<Emitted>) {
        self.retired.insert(
            id,
            Retired {
                deadline: Instant::now() + crate::http_body_policy::IDLE,
                consumed,
                records,
                notice: None,
            },
        );
    }

    /// An overdue ACK, credit or receipt closes the channel; a body whose own
    /// oldest record stalled is retired with a timeout error instead.
    fn overdue(&mut self, budget: &Budget, closed: &CancellationToken) -> Flow {
        if self
            .retired
            .values()
            .any(|receipt| receipt.deadline <= Instant::now())
        {
            return Err(());
        }
        let stalled = self
            .bodies
            .iter()
            .filter(|(_, body)| {
                body.records.front().is_some_and(|record| {
                    record.at.elapsed() >= crate::http_body_policy::IDLE
                        || record
                            .item
                            .progress
                            .as_ref()
                            .is_some_and(|p| p.check().is_err())
                })
            })
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if self
            .controls
            .front()
            .is_some_and(|(_, at, _)| at.elapsed() >= crate::http_body_policy::IDLE)
            || (!self.bodies.is_empty()
                && self.last_credit.elapsed() >= crate::http_body_policy::IDLE)
        {
            return Err(());
        }
        for id in stalled {
            self.headers.remove(&id);
            let Some(Body { consumed, records }) = self.bodies.remove(&id) else {
                continue;
            };
            for record in &records {
                record.item.cancel.cancel();
            }
            self.retire(id, consumed, records);
            let Ok(bytes) = serde_json::to_vec(
                &serde_json::json!({"op":"error","id":id,"code":"response_timeout"}),
            ) else {
                closed.cancel();
                break;
            };
            let Ok(charge) = budget.charge(bytes.len() + 24) else {
                closed.cancel();
                break;
            };
            self.pending.push_back(Queued {
                http: None,
                header: false,
                ack: true,
                retire: Some(id),
                bytes,
                _charge: charge,
                window: None,
                cancel: closed.clone(),
                progress: None,
            });
        }
        Ok(())
    }

    fn outgoing(&mut self, item: Option<Queued>, closed: &CancellationToken) {
        let Some(mut item) = item else {
            self.input_done = true;
            return;
        };
        if let Some(id) = item.retire {
            self.headers.remove(&id);
            if let Some(Body { consumed, records }) = self.bodies.remove(&id) {
                for record in &records {
                    record.item.cancel.cancel();
                }
                self.retire(id, consumed, records);
            } else if !self.retired.contains_key(&id) {
                // Nothing emitted remains charged: only the ordinary error ACK is due.
                item.retire = None;
                item.ack = true;
            }
            self.pending.retain(|record| {
                if record.http == Some(id) {
                    record.cancel.cancel();
                    false
                } else {
                    true
                }
            });
            item.cancel = closed.clone();
            item.http = None;
        }
        self.pending.push_back(item);
    }
}

fn ends_record(records: &VecDeque<Emitted>, offset: u64) -> bool {
    records.iter().any(|r| {
        r.item
            .window
            .as_ref()
            .is_some_and(|window| window.1 == offset)
    })
}

fn drop_credited(records: &mut VecDeque<Emitted>, offset: u64) {
    while records.front().is_some_and(|r| {
        r.item
            .window
            .as_ref()
            .is_some_and(|window| window.1 <= offset)
    }) {
        records.pop_front();
    }
}

async fn any_of<F: std::future::Future + Unpin>(futures: Vec<F>) {
    if futures.is_empty() {
        std::future::pending::<()>().await;
    } else {
        futures_util::future::select_all(futures).await;
    }
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
    let mut writer = Writer::new();
    // Idle keepalive owns no sequence or budget: the single writer sends it
    // sequentially on the same sink, so it never interleaves with encrypted
    // records. It stops with the connection via the closed token below.
    let mut keepalive = tokio::time::interval_at(
        Instant::now() + SECURE_CHANNEL_KEEPALIVE,
        SECURE_CHANNEL_KEEPALIVE,
    );
    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        writer.drop_cancelled(feedback.is_empty());
        if let Some(index) = writer.pending.iter().position(|item| writer.sendable(item)) {
            let Some(item) = writer.pending.remove(index) else {
                break;
            };
            let Ok(record) = seal(&item.bytes) else {
                break;
            };
            let sent = tokio::select! {
                () = closed.cancelled() => break,
                sent = tokio::time::timeout(crate::http_body_policy::IDLE, sender.send(Message::Binary(record.into()))) => sent,
            };
            if !matches!(sent, Ok(Ok(()))) || writer.emitted(item).is_err() {
                break;
            }
            // Read feedback before emitting the next queued record.
        }
        if writer.finished() {
            break;
        }
        let cancellations = writer
            .pending
            .iter()
            .chain(
                writer
                    .bodies
                    .values()
                    .flat_map(|body| body.records.iter().map(|record| &record.item)),
            )
            .chain(writer.controls.iter().filter_map(|(_, _, r)| r.as_ref()))
            .filter(|item| !item.cancel.is_cancelled())
            .map(|item| Box::pin(item.cancel.clone().cancelled_owned()))
            .collect::<Vec<_>>();
        let deadline = writer.deadline();
        let expirations = writer
            .bodies
            .values()
            .filter_map(|body| {
                body.records
                    .front()
                    .and_then(|record| record.item.progress.clone())
            })
            .map(|progress| Box::pin(async move { progress.expired().await }))
            .collect::<Vec<_>>();
        let waiting = writer.waiting();
        let ready = writer.pending.iter().any(|item| writer.sendable(item));
        let input_done = writer.input_done;
        tokio::select! {
            biased;
            () = closed.cancelled() => break,
            () = any_of(cancellations) => {},
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
            frame = feedback.recv() => {
                let Some(frame) = frame else { break };
                if writer.feedback(&frame, &budget, &closed).is_err() {
                    break;
                }
            }
            () = async { tokio::select! { () = tokio::time::sleep_until(deadline) => {}, () = any_of(expirations) => {} } }, if waiting => {
                if writer.overdue(&budget, &closed).is_err() {
                    break;
                }
            },
            item = outgoing.recv(), if !input_done => writer.outgoing(item, &closed),
            () = std::future::ready(()), if ready => {},
        }
    }
    closed.cancel();
}
