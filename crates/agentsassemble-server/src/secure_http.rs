//! Virtual HTTP uses the original router, public provenance and connection permits.
use crate::secure_body_record::{self, CAPABILITY, CONFIRMED};
use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Uri},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{OwnedSemaphorePermit, mpsc};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use crate::{
    AppState,
    http_admission::HttpConnectionAdmission,
    ingress_trust::PeerAddr,
    public_ingress::PublicIngress,
    secure_client::SecureClient,
    secure_queue::{Budget, CHUNK_BYTES, Charge, Output},
};

pub(crate) struct Provenance {
    pub(crate) headers: HeaderMap,
    pub(crate) peer: PeerAddr,
    pub(crate) ingress: PublicIngress,
    pub(crate) admission: HttpConnectionAdmission,
}

pub(crate) struct Input {
    tx: mpsc::Sender<Part>,
    pub(crate) cancel: CancellationToken,
    ended: bool,
    binary: bool,
    offset: u64,
}
struct Part {
    bytes: Vec<u8>,
    end: bool,
    charge: Charge,
}

impl Input {
    pub(crate) fn push(&mut self, data: &str, end: bool, budget: &Budget) -> Result<(), ()> {
        // An early response/timeout has already dropped this request's receiver.
        // In-flight child data must not tear down unrelated channel operations.
        if self.tx.is_closed() {
            return Ok(());
        }
        if self.binary || self.ended || data.len() > CHUNK_BYTES.div_ceil(3) * 4 {
            return Err(());
        }
        let bytes = URL_SAFE_NO_PAD.decode(data).map_err(|_| ())?;
        if bytes.len() > CHUNK_BYTES {
            return Err(());
        }
        let charge = budget.charge(bytes.len())?;
        self.tx
            .try_send(Part { bytes, end, charge })
            .map_err(|_| ())?;
        self.ended = end;
        Ok(())
    }
    pub(crate) fn push_binary(
        &mut self,
        record: secure_body_record::Record<'_>,
        budget: &Budget,
    ) -> Result<(), ()> {
        if !self.binary {
            return Err(());
        }
        if self.tx.is_closed() {
            return Ok(());
        }
        if self.ended || record.offset != self.offset {
            return Err(());
        }
        let charge = budget.charge(record.bytes.len())?;
        self.tx
            .try_send(Part {
                bytes: record.bytes.to_vec(),
                end: record.end,
                charge,
            })
            .map_err(|_| ())?;
        self.offset += record.bytes.len() as u64;
        self.ended = record.end;
        Ok(())
    }
}

pub(crate) struct Start {
    pub(crate) id: u32,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) upload_credit: bool,
    pub(crate) binary_confirmed: Arc<AtomicBool>,
}

pub(crate) fn start(
    state: AppState,
    client: &SecureClient,
    provenance: &Provenance,
    start: &Start,
    output: &Output,
    permit: OwnedSemaphorePermit,
) -> Result<
    (
        Input,
        impl std::future::Future<Output = ()> + Send + 'static,
    ),
    (),
> {
    let mode = start
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(CAPABILITY))
        .map(|(_, value)| value.as_str());
    let binary = match mode {
        None | Some("1") => false,
        Some("2") if start.binary_confirmed.load(Ordering::Acquire) => true,
        _ => return Err(()),
    };
    let admission = provenance.admission.admit_virtual().ok_or(())?;
    let (method, uri, headers) = request_metadata(client, provenance, start)?;
    let (tx, rx) = mpsc::channel::<Part>(16);
    let cancel = client.closed().child_token();
    let policy = crate::http_body_policy::Policy::new();
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(request_body(
            rx,
            cancel.clone(),
            start.id,
            start.upload_credit,
            output.with_cancel(cancel.clone()),
            policy.clone(),
        ))
        .map_err(|_| ())?;
    *request.headers_mut() = headers;
    request.extensions_mut().insert(provenance.peer);
    request.extensions_mut().insert(provenance.ingress.clone());
    request.extensions_mut().insert(admission.clone());
    request.extensions_mut().insert(client.clone());
    request.extensions_mut().insert(policy);
    let input = Input {
        tx,
        cancel: cancel.clone(),
        ended: false,
        binary,
        offset: 0,
    };
    let output = output.with_cancel(cancel.clone());
    let id = start.id;
    let channel = client.closed().clone();
    let confirm = (mode == Some("1")).then(|| start.binary_confirmed.clone());
    let task = async move {
        let _permit = permit;
        let _admission = admission;
        // Once dispatched, never abort a handler: its mutation may already own durable work.
        let response = crate::web::router(state).oneshot(request).await;
        let Ok(response) = response;
        respond_mode(response, id, output, cancel, channel, binary, confirm).await;
    };
    Ok((input, task))
}

#[cfg(test)]
async fn respond(
    response: axum::response::Response,
    id: u32,
    output: Output,
    cancel: CancellationToken,
    channel: CancellationToken,
) {
    respond_mode(response, id, output, cancel, channel, false, None).await;
}

async fn respond_mode(
    response: axum::response::Response,
    id: u32,
    output: Output,
    cancel: CancellationToken,
    channel: CancellationToken,
    binary: bool,
    confirm: Option<Arc<AtomicBool>>,
) {
    let progress = response
        .extensions()
        .get::<crate::http_body_policy::Progress>()
        .cloned();
    let output = output.with_progress(progress.clone());
    let response = send_response(response, id, output, cancel.clone(), binary, confirm);
    if let Some(progress) = progress {
        tokio::select! {
            // Stop-and-wait is channel-wide: a withheld ACK can prevent an error
            // record too. Closing exposes failure and releases all queued records.
            () = progress.expired() => channel.cancel(),
            () = cancel.cancelled() => {},
            () = response => {},
        }
    } else {
        tokio::select! { () = cancel.cancelled() => {}, () = response => {} }
    }
}

async fn send_response(
    response: axum::response::Response,
    id: u32,
    output: Output,
    cancel: CancellationToken,
    binary: bool,
    confirm: Option<Arc<AtomicBool>>,
) {
    let mut headers = response
        .headers()
        .iter()
        .filter(|(name, _)| {
            !matches!(
                name.as_str(),
                "set-cookie" | "content-security-policy" | "access-control-allow-origin"
            )
        })
        .filter_map(|(name, value)| value.to_str().ok().map(|value| (name.as_str(), value)))
        .collect::<Vec<_>>();
    headers.push(("x-agentsassemble-upload-credit", "1"));
    headers.push((CAPABILITY, "1"));
    if let Some(confirm) = confirm {
        confirm.store(true, Ordering::Release);
        headers.push((CONFIRMED, "1"));
    }
    if !headers
        .iter()
        .any(|(name, _)| *name == "x-agentsassemble-upload-transfer")
    {
        headers.push(("x-agentsassemble-upload-transfer", "1"));
    }
    if output.send(json!({"op":"response", "id":id, "status":response.status().as_u16(), "headers":headers})).await.is_err() { return; }
    let mut body = response.into_body().into_data_stream();
    let mut offset = 0;
    loop {
        let next = tokio::select! { () = cancel.cancelled() => return, next = body.next() => next };
        match next {
            Some(Ok(bytes)) => {
                for chunk in bytes.chunks(CHUNK_BYTES) {
                    let sent = if binary {
                        output
                            .send_bytes(secure_body_record::encode(id, offset, chunk, false))
                            .await
                    } else {
                        output.send(json!({"op":"data", "id":id, "data":URL_SAFE_NO_PAD.encode(chunk), "end":false})).await
                    };
                    if sent.is_err() {
                        return;
                    }
                    offset += chunk.len() as u64;
                }
            }
            Some(Err(_)) => {
                let _ = output
                    .send(json!({"op":"error", "id":id, "code":"response_failed"}))
                    .await;
                return;
            }
            None => {
                if binary {
                    let _ = output
                        .send_bytes(secure_body_record::encode(id, offset, &[], true))
                        .await;
                } else {
                    let _ = output
                        .send(json!({"op":"data", "id":id, "data":"", "end":true}))
                        .await;
                }
                return;
            }
        }
    }
}

fn request_body(
    rx: mpsc::Receiver<Part>,
    cancel: CancellationToken,
    id: u32,
    upload_credit: bool,
    output: Output,
    policy: crate::http_body_policy::Policy,
) -> Body {
    let stream = futures_util::stream::unfold(
        (rx, false, cancel, false, policy),
        move |(mut rx, ended, cancel, credit, policy)| {
            let output = output.clone();
            async move {
                if ended {
                    return None;
                }
                // Resumption means the bounded consumer has processed the prior chunk.
                // A failed credit path is an incomplete body, never successful EOF.
                let credit_failed = if credit {
                    let _server_wait = policy.server_wait();
                    output
                        .send(json!({"op":"consumed", "id":id}))
                        .await
                        .is_err()
                } else {
                    false
                };
                let next = if credit_failed {
                    None
                } else {
                    tokio::select! { biased; () = cancel.cancelled() => None, next = rx.recv() => next }
                };
                match next {
                    Some(part) => {
                        let Part { bytes, end, charge } = part;
                        // The HTTP consumer now owns these bytes under its body limit;
                        // they are no longer retained in the encrypted ingress queue.
                        drop(charge);
                        Some((
                            Ok::<_, std::io::Error>(Bytes::from(bytes)),
                            (rx, end, cancel, upload_credit && !end, policy),
                        ))
                    }
                    None => Some((
                        Err(std::io::Error::other(
                            "Encrypted request ended before its body completed",
                        )),
                        (rx, true, cancel, false, policy),
                    )),
                }
            }
        },
    );
    Body::from_stream(stream)
}

fn request_metadata(
    client: &SecureClient,
    provenance: &Provenance,
    start: &Start,
) -> Result<(Method, Uri, HeaderMap), ()> {
    let method: Method = start.method.parse().map_err(|_| ())?;
    if !matches!(
        method,
        Method::GET | Method::POST | Method::DELETE | Method::HEAD
    ) {
        return Err(());
    }
    let uri: Uri = start.path.parse().map_err(|_| ())?;
    if !start.path.starts_with('/')
        || start.path.starts_with("//")
        || uri.scheme().is_some()
        || uri.authority().is_some()
        || start.path.len() > 8192
        || start.headers.len() > 64
    {
        return Err(());
    }
    let mut headers = provenance.headers.clone();
    for name in [
        "origin",
        "authorization",
        "cookie",
        "content-length",
        "content-type",
        "connection",
        "upgrade",
        "sec-websocket-key",
        "sec-websocket-version",
        "sec-websocket-extensions",
        "sec-websocket-protocol",
        "accept",
        "if-none-match",
        "range",
        "x-device-token",
        "x-central-generation",
    ] {
        headers.remove(name);
    }
    headers.insert("origin", client.hello().origin.parse().map_err(|_| ())?);
    let mut size = 0;
    for (name, value) in &start.headers {
        size += name.len() + value.len();
        // Product credentials/content only. Physical proxy/host identity belongs to the outer request.
        if size > 16 * 1024
            || !matches!(
                name.to_ascii_lowercase().as_str(),
                "authorization"
                    | "content-type"
                    | "x-device-token"
                    | "x-central-generation"
                    | "accept"
                    | "if-none-match"
                    | "range"
                    | "x-agentsassemble-body-binary"
            )
        {
            return Err(());
        }
        let name: HeaderName = name.parse().map_err(|_| ())?;
        if headers.contains_key(&name) {
            return Err(());
        }
        headers.insert(name, HeaderValue::from_str(value).map_err(|_| ())?);
    }
    Ok((method, uri, headers))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "secure_download_tests.rs"]
mod download_tests;
