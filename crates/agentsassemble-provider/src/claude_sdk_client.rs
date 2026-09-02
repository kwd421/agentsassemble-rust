use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite, LinesCodec};

use crate::{driver::DriverError, launch_error::DriverLaunchError};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROTOCOL_LINE_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
struct Initialize<'a> {
    r#type: &'static str,
    workspace: &'a str,
    model: &'a str,
    reasoning_effort: &'a str,
    service_tier: &'a str,
    permission_mode: &'a str,
    resume_session_id: &'a str,
    room_portal: RoomPortalAuthority<'a>,
}
#[derive(Serialize)]
struct RoomPortalAuthority<'a> {
    url: String,
    bearer_token: &'a str,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
enum HostMessage {
    Ready {
        session_id: String,
        reused: bool,
        model: String,
    },
    TurnResult {
        turn_id: String,
        provider_turn_id: String,
        session_id: String,
        content: String,
    },
    Stopped,
    Fatal {
        code: String,
    },
}
pub(crate) struct ClaudeSdkAttachment {
    pub(crate) session_id: String,
    pub(crate) reused: bool,
}
pub(crate) struct ClaudeSdkTurn {
    pub(crate) provider_turn_id: String,
    pub(crate) session_id: String,
    pub(crate) content: String,
}
pub(crate) struct ClaudeSdkClient<I, O> {
    input: FramedWrite<I, LinesCodec>,
    output: FramedRead<O, LinesCodec>,
    session_id: String,
    poisoned: bool,
}

impl<I, O> ClaudeSdkClient<I, O>
where
    I: AsyncWrite + Unpin,
    O: AsyncRead + Unpin,
{
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn connect(
        input: I,
        output: O,
        workspace: &str,
        model: &str,
        reasoning_effort: &str,
        service_tier: &str,
        permission_mode: &str,
        resume_session_id: &str,
        room_portal_url: String,
        room_portal_bearer: &str,
    ) -> Result<(Self, ClaudeSdkAttachment), DriverLaunchError> {
        let mut client = Self {
            input: FramedWrite::new(
                input,
                LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
            ),
            output: FramedRead::new(
                output,
                LinesCodec::new_with_max_length(MAX_PROTOCOL_LINE_BYTES),
            ),
            session_id: String::new(),
            poisoned: false,
        };
        client
            .send(&Initialize {
                r#type: "initialize",
                workspace,
                model,
                reasoning_effort,
                service_tier,
                permission_mode,
                resume_session_id,
                room_portal: RoomPortalAuthority {
                    url: room_portal_url,
                    bearer_token: room_portal_bearer,
                },
            })
            .await
            .map_err(DriverLaunchError::uncertain)?;
        let ready = match tokio::time::timeout(PROTOCOL_TIMEOUT, client.receive()).await {
            Ok(Ok(HostMessage::Ready {
                session_id,
                reused,
                model: observed_model,
            })) if observed_model == model
                && reused != resume_session_id.is_empty()
                && valid_session_id(&session_id, resume_session_id) =>
            {
                ClaudeSdkAttachment { session_id, reused }
            }
            Ok(Ok(_) | Err(_)) | Err(_) => {
                return Err(DriverLaunchError::uncertain(protocol_error()));
            }
        };
        client.session_id.clone_from(&ready.session_id);
        Ok((client, ready))
    }

    pub(crate) async fn turn(
        &mut self,
        turn_id: &str,
        input: &str,
    ) -> Result<ClaudeSdkTurn, DriverError> {
        self.send(&serde_json::json!({
            "type": "turn",
            "turn_id": turn_id,
            "input": input,
        }))
        .await?;
        match self.receive().await {
            Ok(HostMessage::TurnResult {
                turn_id: observed_turn_id,
                provider_turn_id,
                session_id,
                content,
            }) if observed_turn_id == turn_id
                && session_id == self.session_id
                && !provider_turn_id.is_empty() =>
            {
                Ok(ClaudeSdkTurn {
                    provider_turn_id,
                    session_id,
                    content,
                })
            }
            Ok(_) | Err(_) => self.poison(protocol_error()),
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), DriverError> {
        if self.poisoned {
            return Err(protocol_error());
        }
        self.send(&serde_json::json!({ "type": "shutdown" }))
            .await?;
        match tokio::time::timeout(PROTOCOL_TIMEOUT, self.receive()).await {
            Ok(Ok(HostMessage::Stopped)) => Ok(()),
            Ok(Ok(_) | Err(_)) | Err(_) => self.poison(protocol_error()),
        }
    }

    pub(crate) const fn requires_restart(&self) -> bool {
        self.poisoned
    }

    async fn send(&mut self, value: &impl Serialize) -> Result<(), DriverError> {
        if self.poisoned {
            return Err(protocol_error());
        }
        let encoded = serde_json::to_string(value).map_err(|_| protocol_error())?;
        if encoded.len() > MAX_PROTOCOL_LINE_BYTES {
            return self.poison(protocol_error());
        }
        if self.input.send(encoded).await.is_err() {
            return self.poison(protocol_error());
        }
        Ok(())
    }

    async fn receive(&mut self) -> Result<HostMessage, DriverError> {
        let Some(Ok(line)) = self.output.next().await else {
            self.poisoned = true;
            return Err(protocol_error());
        };
        let Ok(message) = serde_json::from_str::<HostMessage>(&line) else {
            return self.poison(protocol_error());
        };
        if let HostMessage::Fatal { code } = &message {
            let _ = code;
            self.poisoned = true;
            return Err(protocol_error());
        }
        Ok(message)
    }

    fn poison<T>(&mut self, error: DriverError) -> Result<T, DriverError> {
        self.poisoned = true;
        Err(error)
    }
}

fn valid_session_id(observed: &str, resumed: &str) -> bool {
    let Ok(session_id) = uuid::Uuid::parse_str(observed) else {
        return false;
    };
    session_id.get_version_num() == 4
        && session_id
            .hyphenated()
            .to_string()
            .eq_ignore_ascii_case(observed)
        && (resumed.is_empty() || observed == resumed)
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "Claude Agent SDK returned an invalid protocol receipt.",
    )
}

#[cfg(test)]
#[path = "claude_sdk_client_tests.rs"]
mod tests;
