//! Executable handoff happens only after the async runtime and every owned state graph are dropped.
use std::{
    net::TcpListener,
    os::{
        fd::{AsFd, OwnedFd},
        unix::{net::UnixListener, process::CommandExt},
    },
    process::Command,
};

use agentsassemble_server::{
    runtime_control_socket::RuntimeControlSocket, runtime_restart::PreparedRestart,
};
use anyhow::Context;
use command_fds::{CommandFdExt, FdMapping};

pub(super) struct InheritedListeners {
    pub http: TcpListener,
    pub control: UnixListener,
}

impl InheritedListeners {
    pub fn take() -> anyhow::Result<Option<Self>> {
        let pid = std::env::var_os("LISTEN_PID");
        let count = std::env::var_os("LISTEN_FDS");
        if pid.is_none() && count.is_none() {
            return Ok(None);
        }
        if pid.as_deref() != Some(std::ffi::OsStr::new(&std::process::id().to_string()))
            || count.as_deref() != Some(std::ffi::OsStr::new("2"))
            || std::env::var_os("LISTEN_FDS_FIRST_FD").is_some()
        {
            anyhow::bail!("runtime listener inheritance is invalid");
        }
        // This is called before creating Tokio or spawning any thread. listenfd consumes
        // the environment and safely takes ownership of these two validated socket types.
        let mut inherited = listenfd::ListenFd::from_env();
        let http = inherited
            .take_tcp_listener(0)?
            .context("inherited HTTP listener missing")?;
        let control = inherited
            .take_unix_listener(1)?
            .context("inherited control listener missing")?;
        if !agentsassemble_server::local_bind_is_supported(http.local_addr()?)
            || !unconnected(&http)?
            || !unconnected(&control)?
        {
            anyhow::bail!("inherited runtime descriptors have invalid endpoint roles");
        }
        // Apple does not implement SO_ACCEPTCONN. Its contract is the validated socket
        // type, unconnected endpoint, exact bound address and durable handoff identity.
        #[cfg(not(target_vendor = "apple"))]
        if !rustix::net::sockopt::socket_acceptconn(&http)?
            || !rustix::net::sockopt::socket_acceptconn(&control)?
        {
            anyhow::bail!("inherited runtime sockets are not listening");
        }
        http.set_nonblocking(true)?;
        control.set_nonblocking(true)?;
        Ok(Some(Self { http, control }))
    }
}

fn unconnected(socket: impl AsFd) -> std::io::Result<bool> {
    match rustix::net::getpeername(socket) {
        Ok(None) | Err(rustix::io::Errno::NOTCONN) => Ok(true),
        Ok(Some(_)) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) struct RuntimeReexec {
    pub prepared: PreparedRestart,
    pub arguments: super::Args,
    pub http: OwnedFd,
    pub control: RuntimeControlSocket,
}

impl RuntimeReexec {
    pub fn execute(self) -> anyhow::Result<()> {
        let mut command = Command::new(self.prepared.image.path());
        command
            .arg("--bind")
            .arg(self.arguments.bind.to_string())
            .arg("--database")
            .arg(&self.arguments.database)
            .arg("--restart-source")
            .arg(
                self.arguments
                    .restart_source
                    .as_ref()
                    .context("runtime source missing")?,
            )
            .arg("--reexec-operation")
            .arg(&self.prepared.operation_id)
            .arg("--reexec-image")
            .arg(self.prepared.image.identity())
            .arg("--reexec-control")
            .arg(self.control.identity())
            .env("LISTEN_PID", std::process::id().to_string())
            .env("LISTEN_FDS", "2")
            .env_remove("LISTEN_FDS_FIRST_FD")
            .env_remove("AGENTSASSEMBLE_HOST_TOKEN");
        if let Some(frontend) = &self.arguments.frontend {
            command.arg("--frontend").arg(frontend);
        }
        if let Some(build) = &self.prepared.frontend_build {
            command.arg("--frontend-build").arg(build);
        }
        if let Some(config) = &self.arguments.stable_entry_config {
            command.arg("--stable-entry-config").arg(config);
        }
        if self.arguments.desktop_native_registration {
            command.arg("--desktop-native-registration");
        }
        command.fd_mappings(vec![
            FdMapping {
                parent_fd: self.http,
                child_fd: 3,
            },
            FdMapping {
                parent_fd: self.control.listener().as_fd().try_clone_to_owned()?,
                child_fd: 4,
            },
        ])?;
        Err(command.exec()).context("replace runtime executable")
    }
}

pub(super) async fn validate(
    args: &super::Args,
    inherited: Option<&InheritedListeners>,
) -> anyhow::Result<()> {
    let fields = (
        args.reexec_operation.as_deref(),
        args.reexec_image.as_deref(),
        args.reexec_control.as_deref(),
    );
    match (inherited, fields) {
        (None, (None, None, None)) if args.frontend_build.is_none() => Ok(()),
        (Some(inherited), (Some(operation), Some(image), Some(_))) => {
            uuid::Uuid::parse_str(operation)?;
            if inherited.http.local_addr()? != args.bind || args.restart_source.is_none() {
                anyhow::bail!("runtime handoff address or source is invalid");
            }
            let current = std::env::current_exe()?;
            let actual = tokio::task::spawn_blocking(move || {
                agentsassemble_server::runtime_image::fingerprint(&current)
            })
            .await??;
            if actual != image {
                anyhow::bail!("runtime handoff image identity does not match");
            }
            Ok(())
        }
        _ => anyhow::bail!("runtime handoff descriptors and operation must be supplied together"),
    }
}
