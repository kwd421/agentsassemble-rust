//! One authenticated outer WebSocket owns encrypted HTTP, streams and room sockets.
use axum::{
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use crate::{
    AppState,
    central::host_identity::SecureClientHello,
    http_admission::HttpConnectionAdmission,
    ingress_trust::{PeerAddr, TrustedIngressOrigin},
    public_ingress::PublicIngress,
    secure_client::SecureClient,
    secure_http::Provenance,
    secure_queue::{Budget, Output},
};

registered_routes! {
    pub(crate) fn routes<AppState>() {
        identity_probe_public "/api/secure-channel" => get(upgrade),
    }
}

async fn upgrade(
    State(state): State<AppState>,
    upgrade: WebSocketUpgrade,
    request: Request,
) -> Response {
    let Some(origin) = request.extensions().get::<TrustedIngressOrigin>() else {
        return StatusCode::FORBIDDEN.into_response();
    };
    if state
        .public_ingress
        .ready_snapshot()
        .is_none_or(|ready| ready.public_url != origin.as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (Some(peer), Some(ingress), Some(admission)) = (
        request.extensions().get::<PeerAddr>(),
        request.extensions().get::<PublicIngress>(),
        request.extensions().get::<HttpConnectionAdmission>(),
    ) else {
        return StatusCode::FORBIDDEN.into_response();
    };
    let provenance = Provenance {
        headers: request.headers().clone(),
        peer: *peer,
        ingress: ingress.clone(),
        admission: admission.clone(),
    };
    let origin = origin.as_str().to_owned();
    let connections = state.connections.clone();
    // The cursor advertisement rides the outer URL query, which this upgrade
    // never parsed before; old hosts ignore it and keep serving old snapshots.
    let wire = crate::client_wire::ClientWire::from_query(request.uri().query());
    upgrade
        .max_message_size(96 * 1024 + 24)
        .max_frame_size(96 * 1024 + 24)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(512 * 1024)
        .on_upgrade(move |socket| {
            connections.track_future(run(socket, state, provenance, origin, wire))
        })
        .into_response()
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    CancelConfirm {
        id: u32,
    },
    Credit {
        id: u32,
        offset: u64,
    },
    Ack {
        sequence: u64,
    },
    Request {
        id: u32,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        #[serde(default)]
        upload_credit: bool,
    },
    Data {
        id: u32,
        data: String,
        end: bool,
    },
    Cancel {
        id: u32,
    },
    SocketOpen {
        id: u32,
        ticket: String,
    },
    SocketData {
        id: u32,
        data: String,
        end: bool,
    },
    SocketClose {
        id: u32,
    },
}
#[path = "secure_channel_requests.rs"]
mod requests;
use requests::Requests;

// Axum handles Ping/Pong at the WebSocket layer. They are not handshake or
// encrypted records and must not race admission into a durable disconnect.
async fn next_record<S>(socket: &mut S) -> Option<Result<Message, axum::Error>>
where
    S: futures_util::Stream<Item = Result<Message, axum::Error>> + Unpin,
{
    loop {
        match socket.next().await {
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
            record => return record,
        }
    }
}

async fn handshake(
    socket: &mut WebSocket,
    state: &AppState,
    origin: &str,
) -> Result<crate::central::host_identity::SecureHandshake, ()> {
    let Some(Ok(Message::Text(text))) = next_record(socket).await else {
        return Err(());
    };
    if text.len() > 4096 {
        return Err(());
    }
    let hello: SecureClientHello = serde_json::from_str(&text).map_err(|_| ())?;
    if !hello_is_live(state, origin, &hello).await {
        return Err(());
    }
    let mut handshake = state
        .central_host_identity
        .secure_handshake(hello)
        .map_err(|_| ())?;
    socket
        .send(Message::Text(
            serde_json::to_string(&handshake.hello)
                .map_err(|_| ())?
                .into(),
        ))
        .await
        .map_err(|_| ())?;
    let Some(Ok(Message::Binary(record))) = next_record(socket).await else {
        return Err(());
    };
    let proof = handshake.receive.open(&record).map_err(|_| ())?;
    if proof != br#"{"op":"confirm"}"# {
        return Err(());
    }
    Ok(handshake)
}

/// Whether this host still serves the incarnation the client named. A
/// hello for another origin, registration epoch or generation is refused
/// here, before the host answers with a `ServerHello`: such a channel never
/// exists, so no route of this host ever sees it.
async fn hello_is_live(state: &AppState, origin: &str, hello: &SecureClientHello) -> bool {
    if hello.origin != origin {
        return false;
    }
    let Ok(Some(epoch)) = state.store.registration_epoch().await else {
        return false;
    };
    if epoch != hello.registration_epoch {
        return false;
    }
    let Ok(generation) = state.store.current_central_endpoint_generation().await else {
        return false;
    };
    generation == hello.generation
}

#[path = "secure_writer.rs"]
mod writer;
#[cfg(test)]
pub(crate) use writer::SECURE_CHANNEL_KEEPALIVE;
pub(crate) use writer::write_records;

fn run(
    mut socket: WebSocket,
    state: AppState,
    provenance: Provenance,
    origin: String,
    wire: crate::client_wire::ClientWire,
) -> futures_util::future::BoxFuture<'static, ()> {
    Box::pin(async move {
        let Ok(Ok(mut crypto)) = tokio::time::timeout(
            Duration::from_secs(10),
            handshake(&mut socket, &state, &origin),
        )
        .await
        else {
            return;
        };
        let Some(ingress_lifetime) = state.public_ingress.ready_lifetime(&origin) else {
            return;
        };
        let Ok(client) = SecureClient::new(
            crypto.hello.client.clone(),
            crypto.hello.channel_id.clone(),
            wire,
        ) else {
            return;
        };
        let lifetime = monitor_lifetime(&state, &client, ingress_lifetime);
        let budget = Budget::new(state.secure_queue_budget.clone());
        let (output, outgoing) =
            Output::new(state.secure_queue_budget.clone(), client.closed().clone());
        let (sender, mut receiver) = socket.split();
        let writer_client = client.clone();
        let writer_lifetime = client.close_guard();
        let writer_budget = output.budget.clone();
        let (ack_tx, ack_rx) = tokio::sync::mpsc::channel::<crate::secure_queue::Feedback>(1);
        let writer = tokio::spawn(async move {
            let _lifetime = writer_lifetime;
            write_records(
                sender,
                |bytes| crypto.send.seal(bytes).map_err(|_| ()),
                outgoing,
                ack_rx,
                writer_client.closed().clone(),
                writer_budget,
            )
            .await;
        });
        let _ = output.send(json!({"op":"ready"})).await;
        let mut requests = Requests::new(
            state.clone(),
            client.clone(),
            provenance,
            output.clone(),
            budget,
            ack_tx,
        );
        let first_admission = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let incoming = tokio::select! {
                () = client.closed().cancelled() => break,
                () = state.shutdown.cancelled() => break,
                () = tokio::time::sleep_until(first_admission), if !client.admitted() => { if !client.admitted() { break; } continue; },
                done = requests.tasks.join_next(), if !requests.tasks.is_empty() => { match done { Some(Ok(id)) => { requests.remove(id).await; }, _ => break } continue; },
                incoming = next_record(&mut receiver) => incoming,
            };
            let Some(Ok(Message::Binary(record))) = incoming else {
                break;
            };
            let Ok(plaintext) = crypto.receive.open(&record) else {
                break;
            };
            let accepted = if plaintext.first() == Some(&crate::secure_body_record::REQUEST) {
                requests.accept_binary(&plaintext)
            } else if let Ok(frame) = serde_json::from_slice::<Frame>(&plaintext) {
                requests.accept(frame).await
            } else {
                Err(())
            };
            if accepted.is_err() {
                break;
            }
        }

        client.close();
        requests.clear().await;
        if state
            .store
            .disconnect_secure_channel(&client.binding().channel_id)
            .await
            .is_err()
        {
            tracing::error!("Secure channel disconnect persistence failed");
        }
        // Drain accepted work; every late admission is durably disconnected before releasing this owner.
        while requests.tasks.join_next().await.is_some() {
            if state
                .store
                .disconnect_secure_channel(&client.binding().channel_id)
                .await
                .is_err()
            {
                tracing::error!("Late secure channel disconnect persistence failed");
            }
        }
        let _ = writer.await;
        let _ = lifetime.await;
    })
}

fn monitor_lifetime(
    state: &AppState,
    client: &SecureClient,
    ingress_lifetime: tokio_util::sync::CancellationToken,
) -> tokio::task::JoinHandle<()> {
    let lifetime_client = client.clone();
    let shutdown = state.shutdown.clone();
    let custody_store = state.store.clone();
    let mut custody_changes = custody_store.subscribe_room_directory();
    tokio::spawn(async move {
        let first_admission = async {
            tokio::time::sleep(Duration::from_secs(30)).await;
            if lifetime_client.admitted() {
                std::future::pending::<()>().await;
            }
        };
        let registration_changed = async {
            loop {
                if custody_store
                    .registration_epoch()
                    .await
                    .ok()
                    .flatten()
                    .as_deref()
                    != Some(&lifetime_client.hello().registration_epoch)
                    || custody_store
                        .secure_channel_member_removed(&lifetime_client.binding().channel_id)
                        .await
                        .unwrap_or(true)
                {
                    return;
                }
                if custody_changes.changed().await.is_err() {
                    return;
                }
            }
        };
        tokio::select! {
            () = registration_changed => {},
            () = lifetime_client.closed().cancelled() => {},
            () = shutdown.cancelled() => {},
            () = ingress_lifetime.cancelled() => {},
            () = lifetime_client.owner_ended() => {},
            () = first_admission => {},
        }
        lifetime_client.close();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsassemble_domain::ProviderCatalog;
    use agentsassemble_persistence::SqliteStore;

    /// A hello of another registration epoch is refused before the host
    /// answers with a `ServerHello`: such a channel never exists, so the
    /// resume route is never reached by a stale incarnation.
    #[tokio::test]
    async fn a_hello_of_another_registration_epoch_never_gets_a_server_hello() {
        let store = SqliteStore::open(&format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        ))
        .await
        .unwrap_or_else(|error| panic!("open handshake fixture store: {error:?}"));
        store
            .bootstrap_local_authority("2f6d1c40-9b1a-4a0e-9c2d-7f5b3e8a1c90", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap handshake fixture: {error:?}"));
        store
            .set_registration_epoch(Some("live-epoch"))
            .await
            .unwrap_or_else(|error| panic!("set handshake fixture epoch: {error:?}"));
        let generation = store
            .next_central_endpoint_generation()
            .await
            .unwrap_or_else(|error| panic!("reserve handshake fixture generation: {error:?}"));
        let state = AppState::local(
            store,
            crate::TicketStore::new(std::time::Duration::from_secs(30), 4096),
            agentsassemble_provider::ProviderCatalogService::fixed(ProviderCatalog::default()),
        )
        .await
        .unwrap_or_else(|error| panic!("build handshake fixture state: {error:?}"));
        let hello = SecureClientHello {
            protocol: "secure_admission_v1".into(),
            server_id: "handshake-server".into(),
            registration_epoch: "live-epoch".into(),
            origin: "https://handshake-host.test".into(),
            generation,
            purpose: "member_connect".into(),
            client_nonce: "handshake-nonce".into(),
            client_public_key: "handshake-key".into(),
        };
        assert!(
            hello_is_live(&state, "https://handshake-host.test", &hello).await,
            "the live incarnation must be served"
        );
        let mut old_epoch = hello.clone();
        old_epoch.registration_epoch = "old-epoch".into();
        assert!(
            !hello_is_live(&state, "https://handshake-host.test", &old_epoch).await,
            "a hello of another registration epoch must never get a ServerHello"
        );
        let mut moved = hello.clone();
        moved.generation = hello.generation + 1;
        assert!(
            !hello_is_live(&state, "https://handshake-host.test", &moved).await,
            "a hello of another generation must never get a ServerHello"
        );
        assert!(
            !hello_is_live(&state, "https://other-host.test", &hello).await,
            "a hello of another origin must never get a ServerHello"
        );
    }

    #[tokio::test]
    async fn control_frames_preserve_application_order_and_close() {
        let mut frames = futures_util::stream::iter([
            Ok(Message::Ping(Vec::new().into())),
            Ok(Message::Text("hello".into())),
            Ok(Message::Pong(Vec::new().into())),
            Ok(Message::Binary(vec![1, 2, 3].into())),
            Ok(Message::Close(None)),
        ]);
        assert!(matches!(
            next_record(&mut frames).await,
            Some(Ok(Message::Text(_)))
        ));
        assert!(matches!(
            next_record(&mut frames).await,
            Some(Ok(Message::Binary(_)))
        ));
        assert!(matches!(
            next_record(&mut frames).await,
            Some(Ok(Message::Close(_)))
        ));
        assert!(next_record(&mut frames).await.is_none());
    }
}
