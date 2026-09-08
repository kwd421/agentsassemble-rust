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
    driver::{DriverError, ProviderDriver, ProviderSessionAttachment},
    launch_error::DriverLaunchError,
};

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

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
    Alive {
        id: u64,
        result: Result<bool, DriverError>,
    },
    Stopped {
        id: u64,
        result: Result<(), DriverError>,
    },
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
    let bytes = serde_json::to_vec(value).map_err(|_| protocol_error())?;
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(protocol_error());
    }
    output
        .send(Bytes::from(bytes))
        .await
        .map_err(|_| protocol_error())
}

pub(super) const fn protocol_error() -> DriverError {
    DriverError::new(
        "managed_bridge_protocol_failed",
        "The private provider bridge transport failed.",
    )
}
