use std::{
    fs::{File, OpenOptions},
    io::{self, BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    process::{Child, Command, Stdio},
};

use command_fds::{CommandFdExt, FdMapping};

const MAX_READY_BYTES: u64 = 128;

pub(crate) struct CodexCodeModeHost {
    child: Child,
    endpoint: String,
}

impl CodexCodeModeHost {
    pub(crate) fn start(
        executable: &Path,
        working_directory: &Path,
        runtime_token: &str,
        stderr: File,
    ) -> io::Result<Self> {
        if !executable.is_absolute() || !executable.metadata()?.is_file() {
            return Err(io::Error::other(
                "Codex code-mode host authority is invalid",
            ));
        }
        let mut command = Command::new(executable);
        crate::process::sanitize_std_environment(&mut command);
        command
            .args(["--listen", "ws://127.0.0.1:0"])
            .current_dir(working_directory)
            .env(crate::unix_process_tree::RUNTIME_TOKEN_ENV, runtime_token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr));
        replace_provider_descriptors(&mut command)?;
        let mut child = command.spawn()?;
        let result = (|| {
            let raw_pid = child
                .id()
                .try_into()
                .ok()
                .and_then(rustix::process::Pid::from_raw)
                .ok_or_else(|| io::Error::other("Codex code-mode host pid is invalid"))?;
            if rustix::process::getpgid(Some(raw_pid))? != rustix::process::getpgrp() {
                return Err(io::Error::other(
                    "Codex code-mode host escaped provider process custody",
                ));
            }
            let output = child
                .stdout
                .take()
                .ok_or_else(|| io::Error::other("Codex code-mode host readiness is unavailable"))?;
            let mut output = BufReader::new(output);
            let mut line = String::new();
            let count = Read::by_ref(&mut output)
                .take(MAX_READY_BYTES + 1)
                .read_line(&mut line)?;
            let endpoint = parse_endpoint(&line, count)?;
            if child.try_wait()?.is_some()
                || rustix::process::getpgid(Some(raw_pid))? != rustix::process::getpgrp()
            {
                return Err(io::Error::other(
                    "Codex code-mode host exited or changed custody before readiness",
                ));
            }
            Ok(endpoint)
        })();
        match result {
            Ok(endpoint) => Ok(Self { child, endpoint }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    pub(crate) fn endpoint(&self) -> &str {
        let _ = &self.child;
        &self.endpoint
    }
}

fn replace_provider_descriptors(command: &mut Command) -> io::Result<()> {
    let null = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    let mappings = [4, 5, 6, 7, 8]
        .into_iter()
        .map(|child_fd| {
            Ok(FdMapping {
                parent_fd: null.try_clone()?.into(),
                child_fd,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    command.fd_mappings(mappings).map_err(io::Error::other)?;
    Ok(())
}

fn parse_endpoint(line: &str, count: usize) -> io::Result<String> {
    if count == 0 || count as u64 > MAX_READY_BYTES || !line.ends_with('\n') {
        return Err(io::Error::other(
            "Codex code-mode host readiness exceeded its bound",
        ));
    }
    let endpoint = line
        .trim_end_matches('\n')
        .strip_suffix('\r')
        .unwrap_or_else(|| line.trim_end_matches('\n'));
    let address = endpoint
        .strip_prefix("ws://")
        .and_then(|value| value.parse::<SocketAddr>().ok())
        .filter(|address| address.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST) && address.port() != 0)
        .ok_or_else(|| io::Error::other("Codex code-mode host endpoint is invalid"))?;
    let canonical = format!("ws://{address}");
    if endpoint != canonical {
        return Err(io::Error::other(
            "Codex code-mode host endpoint is not canonical",
        ));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    #[test]
    fn readiness_accepts_only_canonical_loopback_websockets() {
        let value = "ws://127.0.0.1:43123\n";
        assert_eq!(
            super::parse_endpoint(value, value.len())
                .unwrap_or_else(|error| panic!("parse endpoint: {error}")),
            "ws://127.0.0.1:43123"
        );
        for value in [
            "ws://0.0.0.0:43123\n",
            "ws://127.0.0.1:0\n",
            " ws://127.0.0.1:43123\n",
            "ws://127.0.0.1:43123/path\n",
        ] {
            assert!(super::parse_endpoint(value, value.len()).is_err());
        }
    }
}
