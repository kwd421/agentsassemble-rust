//! Virtual HTTP uses the original router, public provenance and connection permits.
use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, HeaderName, HeaderValue, Method, Request, Uri},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use serde_json::json;
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
}
struct Part {
    bytes: Vec<u8>,
    end: bool,
    _charge: Charge,
}
impl AsRef<[u8]> for Part {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

impl Input {
    pub(crate) fn push(&mut self, data: &str, end: bool, budget: &Budget) -> Result<(), ()> {
        if self.ended || data.len() > CHUNK_BYTES.div_ceil(3) * 4 {
            return Err(());
        }
        let bytes = URL_SAFE_NO_PAD.decode(data).map_err(|_| ())?;
        if bytes.len() > CHUNK_BYTES {
            return Err(());
        }
        let charge = budget.charge(bytes.len())?;
        self.tx
            .try_send(Part {
                bytes,
                end,
                _charge: charge,
            })
            .map_err(|_| ())?;
        self.ended = end;
        Ok(())
    }
}

pub(crate) struct Start {
    pub(crate) id: u32,
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) headers: Vec<(String, String)>,
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
    let admission = provenance.admission.admit_virtual().ok_or(())?;
    let (method, uri, headers) = request_metadata(client, provenance, start)?;
    let (tx, rx) = mpsc::channel::<Part>(16);
    let cancel = client.closed().child_token();
    let body_cancel = cancel.clone();
    let stream = futures_util::stream::unfold(
        (rx, false, body_cancel),
        |(mut rx, ended, cancel)| async move {
            if ended {
                return None;
            }
            let next = tokio::select! { () = cancel.cancelled() => None, next = rx.recv() => next };
            match next {
                Some(part) => {
                    let end = part.end;
                    Some((
                        Ok::<_, std::io::Error>(Bytes::from_owner(part)),
                        (rx, end, cancel),
                    ))
                }
                None => Some((
                    Err(std::io::Error::other(
                        "Encrypted request ended before its body completed",
                    )),
                    (rx, true, cancel),
                )),
            }
        },
    );
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::from_stream(stream))
        .map_err(|_| ())?;
    *request.headers_mut() = headers;
    request.extensions_mut().insert(provenance.peer);
    request.extensions_mut().insert(provenance.ingress.clone());
    request.extensions_mut().insert(admission.clone());
    request.extensions_mut().insert(client.clone());
    let input = Input {
        tx,
        cancel: cancel.clone(),
        ended: false,
    };
    let output = output.with_cancel(cancel.clone());
    let id = start.id;
    let task = async move {
        let _permit = permit;
        let _admission = admission;
        // Once dispatched, never abort a handler: its mutation may already own durable work.
        let response = crate::web::router(state).oneshot(request).await;
        let Ok(response) = response;
        let headers = response
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
        if output.send(json!({"op":"response", "id":id, "status":response.status().as_u16(), "headers":headers})).await.is_err() { return; }
        let mut body = response.into_body().into_data_stream();
        loop {
            let next =
                tokio::select! { () = cancel.cancelled() => return, next = body.next() => next };
            match next {
                Some(Ok(bytes)) => {
                    for chunk in bytes.chunks(CHUNK_BYTES) {
                        if output.send(json!({"op":"data", "id":id, "data":URL_SAFE_NO_PAD.encode(chunk), "end":false})).await.is_err() { return; }
                    }
                }
                Some(Err(_)) => {
                    let _ = output
                        .send(json!({"op":"error", "id":id, "code":"response_failed"}))
                        .await;
                    return;
                }
                None => {
                    let _ = output
                        .send(json!({"op":"data", "id":id, "data":"", "end":true}))
                        .await;
                    return;
                }
            }
        }
    };
    Ok((input, task))
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
