//! One authenticated outer WebSocket owns encrypted HTTP, streams and room sockets.
use axum::{
    extract::{
        Request, State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, time::Duration};
use tokio::{sync::Semaphore, task::JoinSet};

use crate::{
    AppState,
    central::host_identity::SecureClientHello,
    http_admission::HttpConnectionAdmission,
    ingress_trust::{PeerAddr, TrustedIngressOrigin},
    public_ingress::PublicIngress,
    secure_client::SecureClient,
    secure_http::{Provenance, Start},
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
    upgrade
        .max_message_size(96 * 1024 + 24)
        .max_frame_size(96 * 1024 + 24)
        .write_buffer_size(64 * 1024)
        .max_write_buffer_size(512 * 1024)
        .on_upgrade(move |socket| connections.track_future(run(socket, state, provenance, origin)))
        .into_response()
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Ack {
        sequence: u64,
    },
    Request {
        id: u32,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
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
enum Input {
    Http(crate::secure_http::Input),
    Socket(crate::secure_socket::Input),
}

async fn handshake(
    socket: &mut WebSocket,
    state: &AppState,
    origin: &str,
) -> Result<crate::central::host_identity::SecureHandshake, ()> {
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        return Err(());
    };
    if text.len() > 4096 {
        return Err(());
    }
    let hello: SecureClientHello = serde_json::from_str(&text).map_err(|_| ())?;
    if hello.origin != origin
        || state
            .store
            .registration_epoch()
            .await
            .map_err(|_| ())?
            .as_deref()
            != Some(&hello.registration_epoch)
        || state
            .store
            .current_central_endpoint_generation()
            .await
            .map_err(|_| ())?
            != hello.generation
    {
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
    let Some(Ok(Message::Binary(record))) = socket.next().await else {
        return Err(());
    };
    let proof = handshake.receive.open(&record).map_err(|_| ())?;
    if proof != br#"{"op":"confirm"}"# {
        return Err(());
    }
    Ok(handshake)
}

fn run(
    mut socket: WebSocket,
    state: AppState,
    provenance: Provenance,
    origin: String,
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
        let Ok(client) =
            SecureClient::new(crypto.hello.client.clone(), crypto.hello.channel_id.clone())
        else {
            return;
        };
        let lifetime_client = client.clone();
        let shutdown = state.shutdown.clone();
        let lifetime = tokio::spawn(async move {
            let first_admission = async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                if lifetime_client.admitted() {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                () = lifetime_client.closed().cancelled() => {},
                () = shutdown.cancelled() => {},
                () = ingress_lifetime.cancelled() => {},
                () = lifetime_client.owner_ended() => {},
                () = first_admission => {},
            }
            lifetime_client.close();
        });
        let budget = Budget::new(state.secure_queue_budget.clone());
        let (output, mut outgoing) =
            Output::new(state.secure_queue_budget.clone(), client.closed().clone());
        let (mut sender, mut receiver) = socket.split();
        let writer_client = client.clone();
        let (ack_tx, mut ack_rx) = tokio::sync::mpsc::channel::<u64>(1);
        let writer = tokio::spawn(async move {
            let mut sequence = 0;
            loop {
                let item = tokio::select! { () = writer_client.closed().cancelled() => break, item = outgoing.recv() => item };
                let Some(item) = item else {
                    break;
                };
                let Ok(record) = crypto.send.seal(&item.bytes) else {
                    break;
                };
                let sent = tokio::select! { () = writer_client.closed().cancelled() => break, sent = sender.send(Message::Binary(record.into())) => sent };
                if sent.is_err() {
                    break;
                }
                let ack = tokio::select! { () = writer_client.closed().cancelled() => break, ack = ack_rx.recv() => ack };
                if ack != Some(sequence) {
                    break;
                }
                sequence += 1;
            }
            writer_client.close();
        });
        let _ = output.send(json!({"op":"ready"})).await;
        let limits = std::sync::Arc::new(Semaphore::new(32));
        let mut inputs = HashMap::<u32, Input>::new();
        let mut tasks = JoinSet::<u32>::new();
        let mut last_id = 0;
        let first_admission = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let incoming = tokio::select! {
                () = client.closed().cancelled() => break,
                () = state.shutdown.cancelled() => break,
                () = tokio::time::sleep_until(first_admission), if !client.admitted() => { if !client.admitted() { break; } continue; },
                done = tasks.join_next(), if !tasks.is_empty() => { match done { Some(Ok(id)) => { inputs.remove(&id); }, _ => break } continue; },
                incoming = receiver.next() => incoming,
            };
            let Some(Ok(Message::Binary(record))) = incoming else {
                break;
            };
            let Ok(plaintext) = crypto.receive.open(&record) else {
                break;
            };
            let Ok(frame) = serde_json::from_slice::<Frame>(&plaintext) else {
                break;
            };
            let result = match frame {
                Frame::Ack { sequence } => ack_tx.try_send(sequence).map_err(|_| ()),
                Frame::Request {
                    id,
                    method,
                    path,
                    headers,
                } => {
                    if id <= last_id {
                        break;
                    }
                    last_id = id;
                    let Ok(permit) = limits.clone().try_acquire_owned() else {
                        let _ = output
                            .send(json!({"op":"error","id":id,"code":"capacity"}))
                            .await;
                        continue;
                    };
                    match crate::secure_http::start(
                        state.clone(),
                        client.clone(),
                        &provenance,
                        Start {
                            id,
                            method,
                            path,
                            headers,
                        },
                        output.clone(),
                        permit,
                    ) {
                        Ok((input, task)) => {
                            inputs.insert(id, Input::Http(input));
                            tasks.spawn(async move {
                                task.await;
                                id
                            });
                            Ok(())
                        }
                        Err(()) => {
                            output
                                .send(json!({"op":"error","id":id,"code":"request_rejected"}))
                                .await
                        }
                    }
                }
                Frame::Data { id, data, end } => match inputs.get_mut(&id) {
                    Some(Input::Http(input)) => input.push(&data, end, &budget),
                    None if id <= last_id => Ok(()),
                    _ => Err(()),
                },
                Frame::Cancel { id } => {
                    if let Some(Input::Http(input)) = inputs.remove(&id) {
                        input.cancel.cancel();
                    }
                    Ok(())
                }
                Frame::SocketOpen { id, ticket } => {
                    if id <= last_id {
                        break;
                    }
                    last_id = id;
                    let Ok(permit) = limits.clone().try_acquire_owned() else {
                        let _ = output
                            .send(json!({"op":"error","id":id,"code":"capacity"}))
                            .await;
                        continue;
                    };
                    match crate::secure_socket::start(
                        state.clone(),
                        &client,
                        &ticket,
                        id,
                        output.clone(),
                    )
                    .await
                    {
                        Ok((input, task)) => {
                            inputs.insert(id, Input::Socket(input));
                            tasks.spawn(async move {
                                let _permit = permit;
                                task.await;
                                id
                            });
                            Ok(())
                        }
                        Err(()) => {
                            output
                                .send(json!({"op":"error","id":id,"code":"socket_rejected"}))
                                .await
                        }
                    }
                }
                Frame::SocketData { id, data, end } => match inputs.get_mut(&id) {
                    Some(Input::Socket(input)) => input.push(&data, end, &budget),
                    None if id <= last_id => Ok(()),
                    _ => Err(()),
                },
                Frame::SocketClose { id } => {
                    inputs.remove(&id);
                    Ok(())
                }
            };
            if result.is_err() {
                break;
            }
        }
        client.close();
        inputs.clear();
        if state
            .store
            .disconnect_secure_channel(&client.binding().channel_id)
            .await
            .is_err()
        {
            tracing::error!("Secure channel disconnect persistence failed");
        }
        // Drain accepted work; every late admission is durably disconnected before releasing this owner.
        while tasks.join_next().await.is_some() {
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
