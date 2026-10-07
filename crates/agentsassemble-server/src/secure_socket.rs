//! Reuses room ticket consumption and the product room socket owner inside a channel.
use crate::{
    AppState,
    secure_client::SecureClient,
    secure_queue::{Budget, CHUNK_BYTES, Charge, Output},
    ticket::{ConsumedSocketTicket, SocketTicketHint},
};
use axum::extract::ws::Message;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{Sink, Stream};
use serde_json::json;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::sync::mpsc;

struct Incoming {
    message: Message,
    _charges: Vec<Charge>,
}
pub(crate) struct Input {
    tx: mpsc::Sender<Incoming>,
    pending: Vec<u8>,
    charges: Vec<Charge>,
}
impl Input {
    pub(crate) fn push(&mut self, data: &str, end: bool, budget: &Budget) -> Result<(), ()> {
        if data.len() > CHUNK_BYTES.div_ceil(3) * 4 {
            return Err(());
        }
        let bytes = URL_SAFE_NO_PAD.decode(data).map_err(|_| ())?;
        if bytes.len() > CHUNK_BYTES
            || self.pending.len() + bytes.len()
                > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
        {
            return Err(());
        }
        self.charges.push(budget.charge(bytes.len())?);
        self.pending.extend(bytes);
        if end {
            let text = String::from_utf8(std::mem::take(&mut self.pending)).map_err(|_| ())?;
            self.tx
                .try_send(Incoming {
                    message: Message::Text(text.into()),
                    _charges: std::mem::take(&mut self.charges),
                })
                .map_err(|_| ())?;
        }
        Ok(())
    }
}

struct VirtualSocket {
    incoming: mpsc::Receiver<Incoming>,
    outgoing: Pin<Box<dyn Sink<Message, Error = axum::Error> + Send>>,
}
impl Stream for VirtualSocket {
    type Item = Result<Message, axum::Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.incoming
            .poll_recv(cx)
            .map(|item| item.map(|item| Ok(item.message)))
    }
}
impl Sink<Message> for VirtualSocket {
    type Error = axum::Error;
    fn poll_ready(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.outgoing.as_mut().poll_ready(cx)
    }
    fn start_send(mut self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
        self.outgoing.as_mut().start_send(item)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.outgoing.as_mut().poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.outgoing.as_mut().poll_close(cx)
    }
}

pub(crate) async fn start(
    state: AppState,
    client: &SecureClient,
    ticket: &str,
    id: u32,
    output: Output,
) -> Result<
    (
        Input,
        impl std::future::Future<Output = ()> + Send + 'static,
    ),
    (),
> {
    if !client.admitted() || client.closed().is_cancelled() {
        return Err(());
    }
    let hint = state
        .tickets
        .socket_ticket_hint(ticket)
        .await
        .map_err(|_| ())?;
    let SocketTicketHint::RoomSession { room_id } = hint else {
        return Err(());
    };
    let revocations = state.rooms.session_revocations(&room_id).await;
    let grant = state.tickets.consume_socket(ticket).await.map_err(|_| ())?;
    let ConsumedSocketTicket::RoomSession(session) = &grant else {
        return Err(());
    };
    if grant.principal().room_id != room_id {
        return Err(());
    }
    state
        .store
        .require_secure_room_transport(session.authorization(), Some(client.binding()))
        .await
        .map_err(|_| ())?;
    let lease = state
        .connection_admission
        .acquire(grant.principal())
        .map_err(|_| ())?;
    let (tx, incoming) = mpsc::channel(8);
    let input = Input {
        tx,
        pending: Vec::new(),
        charges: Vec::new(),
    };
    let outgoing = futures_util::sink::unfold(
        output.clone(),
        move |output, message: Message| async move {
            let result = async {
            match message {
                Message::Text(text) => {
                    if text.len() > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES { return Err(()); }
                    let count = text.len().div_ceil(CHUNK_BYTES).max(1);
                    for (index, chunk) in text.as_bytes().chunks(CHUNK_BYTES).enumerate() {
                        output.send(json!({"op":"socket_data", "id":id, "data":URL_SAFE_NO_PAD.encode(chunk), "end":index+1==count})).await?;
                    }
                }
                Message::Close(_) => { output.send(json!({"op":"socket_close", "id":id})).await?; }
                Message::Ping(_) | Message::Pong(_) => {}
                Message::Binary(_) => return Err(()),
            }
            Ok(())
        }.await;
            result
                .map_err(|()| axum::Error::new(std::io::Error::other("Encrypted socket closed")))?;
            Ok(output)
        },
    );
    let socket = VirtualSocket {
        incoming,
        outgoing: Box::pin(outgoing),
    };
    let task = async move {
        if output
            .send(json!({"op":"socket_open", "id":id}))
            .await
            .is_err()
        {
            return;
        }
        crate::room_socket_session::run(socket, state, grant, Some(revocations), lease).await;
        let _ = output.send(json!({"op":"socket_close", "id":id})).await;
    };
    Ok((input, task))
}
