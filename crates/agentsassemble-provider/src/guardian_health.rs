use std::{
    io::{self, BufRead, BufReader, Read, Write},
    process::Child,
    time::Duration,
};

use futures_util::StreamExt;
use rustix::process::Pid;
use tokio::{io::AsyncWriteExt, process::ChildStdin};
use tokio_util::codec::{FramedRead, LinesCodec};

use crate::runtime::DriverError;

const REQUEST: &str = "HEALTH";
const RESPONSE_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_HEALTH=";
const MAX_COMMAND_BYTES: u64 = 64;
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn serve(provider: &mut Child, provider_pid: u32) -> io::Result<()> {
    let mut input = BufReader::new(io::stdin().lock());
    loop {
        let mut command = String::new();
        let count = Read::by_ref(&mut input)
            .take(MAX_COMMAND_BYTES + 1)
            .read_line(&mut command)?;
        if count == 0 {
            return Ok(());
        }
        if count as u64 > MAX_COMMAND_BYTES
            || !command.ends_with('\n')
            || command.trim_end_matches(['\r', '\n']) != REQUEST
        {
            return Err(io::Error::other("provider guardian command was invalid"));
        }
        let exact_child_is_alive = provider.try_wait()?.is_none();
        writeln!(
            io::stdout().lock(),
            "{RESPONSE_PREFIX}{provider_pid}:{}",
            if exact_child_is_alive {
                "alive"
            } else {
                "exited"
            }
        )?;
        io::stdout().lock().flush()?;
    }
}

pub(crate) async fn probe(
    input: &mut ChildStdin,
    output: &mut FramedRead<tokio::process::ChildStdout, LinesCodec>,
    provider_pid: Pid,
) -> Result<bool, DriverError> {
    input
        .write_all(format!("{REQUEST}\n").as_bytes())
        .await
        .map_err(|_| health_error())?;
    input.flush().await.map_err(|_| health_error())?;
    let response = tokio::time::timeout(RESPONSE_TIMEOUT, output.next())
        .await
        .map_err(|_| health_error())?
        .ok_or_else(health_error)?
        .map_err(|_| health_error())?;
    let prefix = format!("{RESPONSE_PREFIX}{}:", provider_pid.as_raw_pid());
    match response.strip_prefix(&prefix) {
        Some("alive") => Ok(true),
        Some("exited") => Ok(false),
        _ => Err(health_error()),
    }
}

const fn health_error() -> DriverError {
    DriverError::new(
        "provider_health_unknown",
        "The provider runtime health could not be observed.",
    )
}
