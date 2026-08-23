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

const REQUEST_PREFIX: &str = "HEALTH:";
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
        let request_id = command
            .ends_with('\n')
            .then(|| command.trim_end_matches(['\r', '\n']))
            .and_then(parse_request);
        if count as u64 > MAX_COMMAND_BYTES || request_id.is_none() {
            return Err(io::Error::other("provider guardian command was invalid"));
        }
        let request_id = request_id.unwrap_or_else(|| unreachable!("request identity checked"));
        let exact_child_is_alive = provider.try_wait()?.is_none();
        writeln!(
            io::stdout().lock(),
            "{RESPONSE_PREFIX}{provider_pid}:{request_id}:{}",
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
    request_id: u64,
) -> Result<bool, DriverError> {
    let response = tokio::time::timeout(RESPONSE_TIMEOUT, async {
        input
            .write_all(format!("{REQUEST_PREFIX}{request_id}\n").as_bytes())
            .await
            .map_err(|_| health_error())?;
        input.flush().await.map_err(|_| health_error())?;
        output
            .next()
            .await
            .ok_or_else(health_error)?
            .map_err(|_| health_error())
    })
    .await
    .map_err(|_| health_error())??;
    parse_response(&response, provider_pid, request_id)
}

fn parse_request(command: &str) -> Option<u64> {
    command
        .strip_prefix(REQUEST_PREFIX)?
        .parse::<u64>()
        .ok()
        .filter(|request_id| *request_id != 0)
}

fn parse_response(response: &str, provider_pid: Pid, request_id: u64) -> Result<bool, DriverError> {
    let prefix = format!(
        "{RESPONSE_PREFIX}{}:{request_id}:",
        provider_pid.as_raw_pid()
    );
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

#[cfg(test)]
mod tests {
    use rustix::process::Pid;

    #[test]
    fn health_response_is_bound_to_exact_request_and_child() {
        let provider = Pid::from_raw(123).unwrap_or_else(|| panic!("valid provider pid"));
        assert_eq!(super::parse_request("HEALTH:7"), Some(7));
        assert!(
            super::parse_response("AGENTSASSEMBLE_PROVIDER_HEALTH=123:7:alive", provider, 7)
                .unwrap_or_else(|error| panic!("parse exact health response: {error}"))
        );
        for response in [
            "AGENTSASSEMBLE_PROVIDER_HEALTH=123:6:alive",
            "AGENTSASSEMBLE_PROVIDER_HEALTH=124:7:alive",
            "AGENTSASSEMBLE_PROVIDER_HEALTH=123:7:unknown",
        ] {
            assert!(super::parse_response(response, provider, 7).is_err());
        }
    }
}
