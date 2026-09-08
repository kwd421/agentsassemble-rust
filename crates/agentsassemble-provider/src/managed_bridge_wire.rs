//! Private inherited-pipe protocol. Never render frames or decode errors in diagnostics.
use std::path::PathBuf;

use agentsassemble_domain::DurableAgentSession;
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

use crate::{
    credentials::private_handoff::SelectedCredential,
    driver::{DriverError, ProviderDriver, ProviderSessionAttachment, ProviderTurnCompleted},
    launch_error::DriverLaunchError,
};

// Preserve the existing maximum attachment, including base64 expansion and frame metadata.
const MAX_FRAME_BYTES: usize =
    agentsassemble_domain::MAX_ATTACHMENT_BYTES.div_ceil(3) * 4 + 4 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Launch {
    pub(super) session: Box<DurableAgentSession>,
    pub(super) credential: Option<SelectedCredential>,
    pub(super) state_root: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Command {
    Attach {
        id: u64,
        session: Box<DurableAgentSession>,
    },
    Prepare {
        id: u64,
        turn: super::turn::TurnInput,
    },
    Send {
        id: u64,
        session: Box<DurableAgentSession>,
    },
    Interrupt {
        id: u64,
        session: Box<DurableAgentSession>,
    },
    Finish {
        id: u64,
    },
    Abort {
        id: u64,
    },
    Callback {
        id: u64,
        callback_id: u64,
        reply: super::callbacks::Reply,
    },
    IsAlive {
        id: u64,
    },
    Stop {
        id: u64,
    },
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Event {
    Facts {
        facts: Facts,
    },
    Ready {
        result: Result<(), DriverLaunchError>,
    },
    Attached {
        id: u64,
        result: Result<ProviderSessionAttachment, DriverError>,
    },
    Prepared {
        id: u64,
        result: Result<(), DriverError>,
    },
    Turn {
        id: u64,
        result: Result<ProviderTurnCompleted, DriverError>,
    },
    TurnCancelled {
        id: u64,
    },
    Interrupted {
        id: u64,
        result: Result<(), DriverError>,
    },
    Finished {
        id: u64,
        result: Result<crate::room_portal::ProviderTurnOutcome, DriverError>,
    },
    Aborted {
        id: u64,
        result: Result<(), DriverError>,
    },
    Callback {
        id: u64,
        callback: super::callbacks::Callback,
    },
    Alive {
        id: u64,
        result: Result<bool, DriverError>,
    },
    Stopped {
        id: u64,
        result: Result<(), DriverError>,
    },
}

impl Command {
    pub(super) fn id(&self) -> u64 {
        match self {
            Self::Attach { id, .. }
            | Self::Prepare { id, .. }
            | Self::Send { id, .. }
            | Self::Interrupt { id, .. }
            | Self::Finish { id }
            | Self::Abort { id }
            | Self::Callback { id, .. }
            | Self::IsAlive { id }
            | Self::Stop { id } => *id,
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Facts {
    pub(super) retains_runtime_after_turn_interrupt: bool,
    pub(super) continuity: Continuity,
    pub(super) attachment_replay_is_safe: bool,
    pub(super) turn_failure_effect_uncertain: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Continuity {
    Reusable,
    RestartRequired,
}

impl Facts {
    pub(super) fn observe(driver: &dyn ProviderDriver) -> Self {
        Self {
            retains_runtime_after_turn_interrupt: driver.retains_runtime_after_turn_interrupt(),
            continuity: if driver.requires_restart() {
                Continuity::RestartRequired
            } else {
                Continuity::Reusable
            },
            attachment_replay_is_safe: driver.attachment_replay_is_safe(),
            turn_failure_effect_uncertain: driver.turn_failure_effect_uncertain(),
        }
    }
}

pub(super) type Reader<R> = FramedRead<R, LengthDelimitedCodec>;
pub(super) type Writer<W> = FramedWrite<W, LengthDelimitedCodec>;

pub(super) fn reader<R: AsyncRead>(input: R) -> Reader<R> {
    FramedRead::new(input, codec())
}

pub(super) fn writer<W: AsyncWrite>(output: W) -> Writer<W> {
    FramedWrite::new(output, codec())
}

fn codec() -> LengthDelimitedCodec {
    LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_BYTES)
        .new_codec()
}

pub(super) async fn read<R: AsyncRead + Unpin, T: DeserializeOwned>(
    input: &mut Reader<R>,
) -> Result<Option<T>, DriverError> {
    match input.next().await {
        Some(Ok(bytes)) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| protocol_error()),
        Some(Err(_)) => Err(protocol_error()),
        None => Ok(None),
    }
}

pub(super) async fn write<W: AsyncWrite + Unpin, T: Serialize>(
    output: &mut Writer<W>,
    value: &T,
) -> Result<(), DriverError> {
    output
        .send(encode(value)?)
        .await
        .map_err(|_| protocol_error())
}

pub(super) fn encode<T: Serialize>(value: &T) -> Result<Bytes, DriverError> {
    let bytes = serde_json::to_vec(value).map_err(|_| protocol_error())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(protocol_error());
    }
    Ok(Bytes::from(bytes))
}

pub(super) const fn protocol_error() -> DriverError {
    DriverError::new(
        "managed_bridge_protocol_failed",
        "The private provider bridge transport failed.",
    )
}
