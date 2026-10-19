use std::{io, path::Path, process::Stdio, time::Duration};

use crate::owned_command::{
    NETWORK_ENVIRONMENT, inherit_environment, owned_command, terminate_now,
};
use process_wrap::tokio::ChildWrapper;
use tempfile::TempDir;
use tokio::{
    process::{ChildStderr, ChildStdout},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCED_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) struct SpawnedTunnel {
    pub(crate) child: Box<dyn ChildWrapper>,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: ChildStderr,
    _config_root: TempDir,
}

pub(crate) async fn spawn_cloudflared(
    executable: &Path,
    local_url: &str,
    origin_host: &str,
) -> io::Result<SpawnedTunnel> {
    let config_root = tempfile::tempdir()?;
    let config_path = config_root.path().join("config.yml");
    std::fs::write(&config_path, [])?;
    let mut command = owned_command(executable);
    command
        .command_mut()
        .args([
            "tunnel",
            "--no-autoupdate",
            "--config",
            config_path.to_string_lossy().as_ref(),
            "--url",
            local_url,
            "--http-host-header",
            origin_host,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    inherit_environment(&mut command, &NETWORK_ENVIRONMENT);
    spawn_piped(command, config_root).await
}

/// Spawns cloudflared in named-tunnel mode for one fixed hostname.
///
/// The token travels in the child's `TUNNEL_TOKEN` environment variable (the
/// documented env alias for `tunnel run --token`, keeping it out of process
/// listings) and never appears in argv. The fixed origin is the current
/// ingress origin; the caller's `origin_host` gate stays the public hostname.
pub(crate) async fn spawn_cloudflared_named(
    executable: &Path,
    local_url: &str,
    token: &str,
) -> io::Result<SpawnedTunnel> {
    let config_root = tempfile::tempdir()?;
    let mut command = owned_command(executable);
    command
        .command_mut()
        .args(["tunnel", "--no-autoupdate", "run", "--url", local_url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    inherit_environment(&mut command, &NETWORK_ENVIRONMENT);
    command.command_mut().env("TUNNEL_TOKEN", token);
    spawn_piped(command, config_root).await
}

async fn spawn_piped(
    mut command: process_wrap::tokio::CommandWrap,
    config_root: TempDir,
) -> io::Result<SpawnedTunnel> {
    let mut child = command.spawn()?;
    let Some(stdout) = child.stdout().take() else {
        terminate_now(child.as_mut()).await;
        return Err(io::Error::other("cloudflared stdout pipe is unavailable"));
    };
    let Some(stderr) = child.stderr().take() else {
        terminate_now(child.as_mut()).await;
        return Err(io::Error::other("cloudflared stderr pipe is unavailable"));
    };
    Ok(SpawnedTunnel {
        child,
        stdout,
        stderr,
        _config_root: config_root,
    })
}

pub(crate) async fn supervise_child(
    child: &mut dyn ChildWrapper,
    cancellation: &CancellationToken,
) -> io::Result<std::process::ExitStatus> {
    tokio::select! {
        result = child.wait() => return result,
        () = cancellation.cancelled() => {}
    }
    let _ = request_graceful_stop(child);
    if let Ok(result) = timeout(GRACEFUL_STOP_TIMEOUT, child.wait()).await {
        return result;
    }
    child.start_kill()?;
    timeout(FORCED_STOP_TIMEOUT, child.wait())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "owned child did not exit"))?
}

#[cfg(unix)]
fn request_graceful_stop(child: &dyn ChildWrapper) -> io::Result<()> {
    use rustix::process::{Pid, Signal, kill_process_group};

    let pid = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("owned child PID is unavailable"))?;
    Ok(kill_process_group(pid, Signal::TERM)?)
}

#[cfg(not(unix))]
fn request_graceful_stop(child: &mut dyn ChildWrapper) -> io::Result<()> {
    child.start_kill()
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn named_spawn_passes_the_token_via_environment_not_argv() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
        let executable = directory.path().join("cloudflared");
        // Shell builtins only: the spawned env clears PATH, so no external
        // lookup beyond the absolute sleep below.
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
printf '%s' "$TUNNEL_TOKEN" > "$0.token"
exec /bin/sleep 60
"#,
        )
        .unwrap_or_else(|error| panic!("write fake cloudflared: {error}"));
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("make fake cloudflared executable: {error}"));
        let mut spawned = super::spawn_cloudflared_named(
            &executable,
            "http://127.0.0.1:41955",
            "secret-tunnel-token",
        )
        .await
        .unwrap_or_else(|error| panic!("spawn named tunnel: {error}"));
        let arguments = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(arguments) =
                    tokio::fs::read_to_string(executable.with_extension("args")).await
                {
                    return arguments;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("fake cloudflared arguments timed out"));
        let token = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(token) =
                    tokio::fs::read_to_string(executable.with_extension("token")).await
                {
                    return token;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("fake cloudflared token timed out"));
        assert!(arguments.contains("--no-autoupdate"));
        assert!(arguments.contains("run"));
        assert!(arguments.contains("http://127.0.0.1:41955"));
        assert!(
            !arguments.contains("secret-tunnel-token"),
            "tunnel token must never appear in argv"
        );
        assert!(
            !arguments.contains("--token"),
            "tunnel token must travel via TUNNEL_TOKEN, not --token"
        );
        assert_eq!(token, "secret-tunnel-token");
        let stop = tokio_util::sync::CancellationToken::new();
        stop.cancel();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            super::supervise_child(spawned.child.as_mut(), &stop),
        )
        .await
        .unwrap_or_else(|_| panic!("named fake cloudflared did not stop"));
    }
}
