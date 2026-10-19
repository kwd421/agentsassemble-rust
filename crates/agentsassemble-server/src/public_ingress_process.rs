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

/// Spawns cloudflared in locally-managed named-tunnel mode for one fixed hostname.
///
/// The credentials JSON travels only in the child's `TUNNEL_CRED_CONTENTS`
/// environment variable (the documented env alias for the bundled cloudflared
/// `2026.5.2` `--credentials-contents` flag) and never appears in argv.
/// `TUNNEL_TOKEN` is never set. The fixed origin is the current ingress
/// origin; the caller's `origin_host` gate stays the public hostname.
pub(crate) async fn spawn_cloudflared_named(
    executable: &Path,
    local_url: &str,
    tunnel_id: &str,
    credentials_contents: &str,
) -> io::Result<SpawnedTunnel> {
    let config_root = tempfile::tempdir()?;
    let mut command = owned_command(executable);
    command
        .command_mut()
        .args([
            "tunnel",
            "--no-autoupdate",
            "run",
            "--url",
            local_url,
            tunnel_id,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    inherit_environment(&mut command, &NETWORK_ENVIRONMENT);
    command
        .command_mut()
        .env("TUNNEL_CRED_CONTENTS", credentials_contents);
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

    const TEST_TUNNEL_ID: &str = "test-tunnel-id";
    const TEST_CREDENTIALS: &str =
        r#"{"AccountTag":"account-tag","TunnelID":"test-tunnel-id","TunnelSecret":"test-secret"}"#;

    #[tokio::test]
    async fn named_spawn_passes_the_credentials_via_environment_not_argv() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("test directory: {error}"));
        let executable = directory.path().join("cloudflared");
        // Shell builtins only: the spawned env clears PATH, so no external
        // lookup beyond the absolute sleep below.
        std::fs::write(
            &executable,
            r#"#!/bin/sh
printf '%s\n' "$@" > "$0.args"
printf '%s' "$TUNNEL_CRED_CONTENTS" > "$0.credentials"
printf '%s' "${TUNNEL_TOKEN-unset}" > "$0.token"
exec /bin/sleep 60
"#,
        )
        .unwrap_or_else(|error| panic!("write fake cloudflared: {error}"));
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("make fake cloudflared executable: {error}"));
        let mut spawned = super::spawn_cloudflared_named(
            &executable,
            "http://127.0.0.1:41955",
            TEST_TUNNEL_ID,
            TEST_CREDENTIALS,
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
        let credentials = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(credentials) =
                    tokio::fs::read_to_string(executable.with_extension("credentials")).await
                {
                    return credentials;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("fake cloudflared credentials timed out"));
        assert!(arguments.contains("--no-autoupdate"));
        assert!(arguments.contains("run"));
        assert!(arguments.contains("http://127.0.0.1:41955"));
        assert!(arguments.contains(TEST_TUNNEL_ID));
        assert!(
            !arguments.contains("test-secret"),
            "tunnel credentials must never appear in argv"
        );
        assert!(
            !arguments.contains("TUNNEL_CRED_CONTENTS"),
            "credentials travel via env value, not argv flag text"
        );
        assert!(
            !arguments.contains("--token"),
            "tunnel credentials must travel via TUNNEL_CRED_CONTENTS, not --token"
        );
        assert!(
            !arguments.contains("TUNNEL_TOKEN"),
            "TUNNEL_TOKEN must never be used for locally-managed tunnels"
        );
        assert_eq!(credentials, TEST_CREDENTIALS);
        let token = tokio::fs::read_to_string(executable.with_extension("token"))
            .await
            .unwrap_or_else(|error| panic!("read fake cloudflared token: {error}"));
        assert_eq!(token, "unset", "TUNNEL_TOKEN must not be set");
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
