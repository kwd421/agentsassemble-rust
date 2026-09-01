#![allow(dead_code)] // Each integration binary exercises a different subset of this shared peer.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

pub struct AuthenticatedTestSocket<S> {
    socket: WebSocketStream<S>,
}

impl<S> AuthenticatedTestSocket<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    pub fn new(socket: WebSocketStream<S>, _ticket: String, _proof_key: String) -> Self {
        Self { socket }
    }

    pub async fn subscribe(&mut self, cursor: i64) -> Value {
        self.send_json(&json!({
            "op": "subscribe",
            "streams": ["room_events"],
            "resume_from_seq": cursor,
        }))
        .await;
        self.receive_json().await
    }

    pub async fn send_json(&mut self, frame: &Value) {
        self.socket
            .send(Message::Text(frame.to_string().into()))
            .await
            .unwrap_or_else(|error| panic!("send WebSocket test frame: {error}"));
    }

    pub async fn receive_json(&mut self) -> Value {
        parse_json(&self.receive_text().await)
    }

    pub async fn receive_json_with_timeout(&mut self, timeout: Duration) -> Value {
        parse_json(&self.receive_text_with_timeout(timeout).await)
    }

    pub async fn receive_text(&mut self) -> String {
        receive_wire_text(&mut self.socket).await
    }

    async fn receive_text_with_timeout(&mut self, timeout: Duration) -> String {
        receive_wire_text_with_timeout(&mut self.socket, timeout).await
    }

    pub async fn send_binary(&mut self, bytes: Vec<u8>) {
        self.socket
            .send(Message::Binary(bytes.into()))
            .await
            .unwrap_or_else(|error| panic!("send binary test frame: {error}"));
    }

    pub async fn close(&mut self) {
        self.socket
            .close(None)
            .await
            .unwrap_or_else(|error| panic!("close test socket: {error}"));
    }

    pub async fn wait_closed(&mut self) -> bool {
        matches!(
            self.socket.next().await,
            None | Some(Ok(Message::Close(_)) | Err(_))
        )
    }
}

fn parse_json(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|error| panic!("decode WebSocket JSON: {error}"))
}

async fn receive_wire_text<S>(socket: &mut WebSocketStream<S>) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = socket
        .next()
        .await
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    String::from_utf8(message.into_data().to_vec())
        .unwrap_or_else(|error| panic!("WebSocket JSON is not UTF-8: {error}"))
}

async fn receive_wire_text_with_timeout<S>(
    socket: &mut WebSocketStream<S>,
    timeout: Duration,
) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(timeout, socket.next())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for WebSocket frame"))
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    String::from_utf8(message.into_data().to_vec())
        .unwrap_or_else(|error| panic!("WebSocket JSON is not UTF-8: {error}"))
}
