use std::{
    io::{self, BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(unix)]
use std::time::Instant;

const SUPERVISOR_FLAG: &str = "--agentsassemble-runtime-supervisor";
#[cfg(target_os = "macos")]
const STAGED_SERVER_ENV: &str = "AGENTSASSEMBLE_INTERNAL_SERVER_STAGED";
const SIDECAR_SHUTDOWN_GRACE: Duration = Duration::from_secs(16);
#[cfg(target_os = "macos")]
const MAX_LSOF_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) struct RuntimeSupervisorCommand {
    command: Command,
    #[cfg(target_os = "macos")]
    _binding: BoundSidecar,
}

impl RuntimeSupervisorCommand {
    pub(crate) fn command_mut(&mut self) -> &mut Command {
        &mut self.command
    }

    pub(crate) fn spawn(&mut self) -> io::Result<Child> {
        self.command.spawn()
    }
}

pub(crate) fn command(executable: &std::path::Path) -> Result<RuntimeSupervisorCommand, String> {
    #[cfg(target_os = "macos")]
    let binding = BoundSidecar::bind_running_current_executable()
        .map_err(|error| format!("cannot bind running desktop executable: {error}"))?;
    #[cfg(target_os = "macos")]
    let current = binding.launch_path().to_path_buf();
    #[cfg(not(target_os = "macos"))]
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot resolve desktop supervisor executable: {error}"))?;
    let mut command = Command::new(current);
    command.arg(SUPERVISOR_FLAG).arg(executable);
    #[cfg(unix)]
    command.process_group(0);
    Ok(RuntimeSupervisorCommand {
        command,
        #[cfg(target_os = "macos")]
        _binding: binding,
    })
}

pub fn run_if_requested() -> Option<i32> {
    let mut arguments = std::env::args_os();
    let _ = arguments.next();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(SUPERVISOR_FLAG)) {
        return None;
    }
    let Some(executable) = arguments.next().map(PathBuf::from) else {
        eprintln!("runtime supervisor did not receive a sidecar executable");
        return Some(2);
    };
    let sidecar_arguments = arguments.collect::<Vec<_>>();
    Some(match supervise(&executable, &sidecar_arguments) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("runtime supervisor failed: {error}");
            1
        }
    })
}

fn supervise(executable: &std::path::Path, arguments: &[std::ffi::OsString]) -> io::Result<()> {
    let _container = supervisor_container()?;
    #[cfg(target_os = "macos")]
    let bound_executable = BoundSidecar::bind(executable)?;
    #[cfg(target_os = "macos")]
    let executable = bound_executable.launch_path();
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(target_os = "macos")]
    command.env(STAGED_SERVER_ENV, "v1");
    let mut sidecar = command.spawn()?;
    let Some(mut sidecar_input) = sidecar.stdin.take() else {
        terminate_sidecar(&mut sidecar);
        return Err(io::Error::other("sidecar stdin pipe is unavailable"));
    };
    let Some(mut sidecar_output) = sidecar.stdout.take() else {
        terminate_sidecar(&mut sidecar);
        return Err(io::Error::other("sidecar stdout pipe is unavailable"));
    };

    let (parent_closed_sender, parent_closed) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let result = io::copy(&mut io::stdin().lock(), &mut sidecar_input)
            .and_then(|_| sidecar_input.flush());
        drop(sidecar_input);
        let _ = parent_closed_sender.send(result);
    });
    let expected_pid = sidecar.id();
    let (output_sender, output_closed) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        let result = forward_owned_output(
            &mut BufReader::new(&mut sidecar_output),
            &mut stdout,
            expected_pid,
        );
        let _ = stdout.flush();
        let _ = output_sender.send(result);
    });

    loop {
        if let Ok(copy_result) = parent_closed.try_recv() {
            terminate_sidecar(&mut sidecar);
            return copy_result;
        }
        if let Ok(output_result) = output_closed.try_recv() {
            terminate_sidecar(&mut sidecar);
            return output_result;
        }
        if sidecar.try_wait()?.is_some() {
            terminate_sidecar(&mut sidecar);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
struct BoundSidecar {
    launch_path: PathBuf,
    _staging: tempfile::TempDir,
}

#[cfg(target_os = "macos")]
impl BoundSidecar {
    fn bind(executable: &std::path::Path) -> io::Result<Self> {
        let canonical = executable.canonicalize()?;
        let source = std::fs::File::open(&canonical)?;
        Self::bind_open_source(source)
    }

    fn bind_running_current_executable() -> io::Result<Self> {
        let current = std::env::current_exe()?;
        let source = std::fs::File::open(current)?;
        let metadata = source.metadata()?;
        let mapped = mapped_text_identities(std::process::id())?;
        if !mapped.contains(&(metadata.dev(), metadata.ino())) {
            return Err(io::Error::other(
                "desktop executable path does not name the running image",
            ));
        }
        Self::bind_open_source(source)
    }

    fn bind_open_source(mut source: std::fs::File) -> io::Result<Self> {
        use std::io::{Seek, Write};

        use agentsassemble_domain::stable_content_identity;
        use same_file::Handle;

        let source_metadata = source.metadata()?;
        if !source_metadata.is_file() || source_metadata.permissions().mode() & 0o111 == 0 {
            return Err(io::Error::other("sidecar source is not executable"));
        }
        let source_handle = Handle::from_file(source.try_clone()?)?;
        let expected_identity = stable_content_identity(&source_handle, &mut source)?;
        source.rewind()?;

        let staging = tempfile::Builder::new()
            .prefix("agentsassemble-server-exec-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()?;
        let staging_metadata = std::fs::symlink_metadata(staging.path())?;
        if !staging_metadata.is_dir()
            || staging_metadata.uid() != nix::unistd::geteuid().as_raw()
            || staging_metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::other("sidecar staging directory is not private"));
        }
        let launch_path = staging.path().join("agentsassemble-server");
        let mut staged = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&launch_path)?;
        io::copy(&mut source, &mut staged)?;
        staged.flush()?;
        staged.sync_all()?;
        staged.rewind()?;
        if stable_content_identity(&source_handle, &mut staged)? != expected_identity {
            return Err(io::Error::other("sidecar staging identity changed"));
        }
        std::fs::set_permissions(&launch_path, std::fs::Permissions::from_mode(0o500))?;
        Ok(Self {
            launch_path,
            _staging: staging,
        })
    }

    fn launch_path(&self) -> &std::path::Path {
        &self.launch_path
    }
}

#[cfg(target_os = "macos")]
fn mapped_text_identities(pid: u32) -> io::Result<Vec<(u64, u64)>> {
    let output = Command::new("/usr/sbin/lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "txt", "-F0Di"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() || output.stdout.len() > MAX_LSOF_OUTPUT_BYTES {
        return Err(io::Error::other(
            "desktop mapped executable inspection failed",
        ));
    }
    parse_mapped_text_identities(&output.stdout)
}

#[cfg(target_os = "macos")]
fn parse_mapped_text_identities(output: &[u8]) -> io::Result<Vec<(u64, u64)>> {
    let mut identities = Vec::new();
    let mut is_text = false;
    let mut device = None;
    for raw_field in output.split(|byte| *byte == 0) {
        let field = raw_field
            .iter()
            .copied()
            .skip_while(u8::is_ascii_whitespace)
            .collect::<Vec<_>>();
        if field.is_empty() {
            continue;
        }
        match field[0] {
            b'f' => {
                is_text = field.get(1..) == Some(b"txt");
                device = None;
            }
            b'D' if is_text => {
                let value = std::str::from_utf8(&field[1..])
                    .map_err(|_| io::Error::other("mapped device identity is invalid"))?;
                device = Some(
                    value
                        .strip_prefix("0x")
                        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
                        .or_else(|| value.parse::<u64>().ok())
                        .ok_or_else(|| io::Error::other("mapped device identity is invalid"))?,
                );
            }
            b'i' if is_text => {
                let inode = std::str::from_utf8(&field[1..])
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| io::Error::other("mapped inode identity is invalid"))?;
                if let Some(device) = device {
                    identities.push((device, inode));
                }
            }
            _ => {}
        }
    }
    if identities.is_empty() {
        return Err(io::Error::other(
            "desktop mapped executable identity is unavailable",
        ));
    }
    Ok(identities)
}

#[cfg(unix)]
struct SupervisorContainer;

#[cfg(unix)]
fn supervisor_container() -> io::Result<SupervisorContainer> {
    use nix::unistd::{getpgrp, getpid};

    if getpgrp() == getpid() {
        Ok(SupervisorContainer)
    } else {
        Err(io::Error::other(
            "runtime supervisor is not its stable process-group leader",
        ))
    }
}

#[cfg(windows)]
struct SupervisorContainer {
    _job: win32job::Job,
}

#[cfg(windows)]
fn supervisor_container() -> io::Result<SupervisorContainer> {
    let job = win32job::Job::create().map_err(io::Error::other)?;
    let mut limits = job.query_extended_limit_info().map_err(io::Error::other)?;
    limits.limit_kill_on_job_close();
    job.set_extended_limit_info(&limits)
        .map_err(io::Error::other)?;
    job.assign_current_process().map_err(io::Error::other)?;
    Ok(SupervisorContainer { _job: job })
}

fn forward_owned_output(
    reader: &mut impl BufRead,
    writer: &mut impl Write,
    expected_pid: u32,
) -> io::Result<()> {
    let mut startup_seen = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return if startup_seen {
                Ok(())
            } else {
                Err(io::Error::other("sidecar closed output before readiness"))
            };
        }
        if !startup_seen && !line.trim().is_empty() {
            let record: serde_json::Value = serde_json::from_str(line.trim())?;
            if record.get("status").and_then(serde_json::Value::as_str) != Some("ready")
                || record.get("runtime").and_then(serde_json::Value::as_str) != Some("rust")
                || record.get("pid").and_then(serde_json::Value::as_u64)
                    != Some(u64::from(expected_pid))
            {
                return Err(io::Error::other(
                    "sidecar readiness did not match the owned child handle",
                ));
            }
            startup_seen = true;
        }
        writer.write_all(line.as_bytes())?;
        writer.flush()?;
    }
}

#[cfg(unix)]
fn terminate_sidecar(child: &mut Child) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::{Pid, getpid},
    };

    let deadline = Instant::now() + SIDECAR_SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let stable_group = Pid::from_raw(-getpid().as_raw());
    let _ = kill(stable_group, Signal::SIGKILL);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_sidecar(child: &mut Child) {
    let deadline = std::time::Instant::now() + SIDECAR_SHUTDOWN_GRACE;
    while std::time::Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    #[cfg(target_os = "macos")]
    use std::process::Command;

    #[cfg(target_os = "macos")]
    use super::{BoundSidecar, parse_mapped_text_identities};
    use super::{SIDECAR_SHUTDOWN_GRACE, forward_owned_output, run_if_requested};

    #[test]
    fn ordinary_desktop_invocation_does_not_enter_supervisor_mode() {
        assert_eq!(run_if_requested(), None);
    }

    #[test]
    fn sidecar_readiness_is_bound_to_the_owned_child_pid() {
        let valid = b"{\"status\":\"ready\",\"runtime\":\"rust\",\"pid\":42}\n";
        let mut output = Vec::new();
        forward_owned_output(&mut Cursor::new(valid), &mut output, 42)
            .unwrap_or_else(|error| panic!("forward valid readiness: {error}"));
        assert_eq!(output, valid);
        assert!(forward_owned_output(&mut Cursor::new(valid), &mut Vec::new(), 41).is_err());
    }

    #[test]
    fn outer_shutdown_budget_covers_server_and_provider_cleanup() {
        assert!(SIDECAR_SHUTDOWN_GRACE >= std::time::Duration::from_secs(14));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mapped_text_identity_parser_accepts_nul_delimited_lsof_fields() {
        let output = b"ftxt\0D0x1000012\0i42\0n/example\0\nftxt\0D16777234\0i84\0";
        assert_eq!(
            parse_mapped_text_identities(output)
                .unwrap_or_else(|error| panic!("parse mapped identities: {error}")),
            vec![(16_777_234, 42), (16_777_234, 84)]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn running_desktop_image_can_be_bound_before_reexecution() {
        let bound = BoundSidecar::bind_running_current_executable()
            .unwrap_or_else(|error| panic!("bind running desktop image: {error}"));
        assert!(bound.launch_path().is_file());
        let output = Command::new(bound.launch_path())
            .arg("--list")
            .output()
            .unwrap_or_else(|error| panic!("execute staged running image: {error}"));
        assert!(output.status.success());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sidecar_binding_survives_source_path_replacement() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create sidecar binding fixture: {error}"));
        let source = directory.path().join("sidecar");
        std::fs::write(&source, "#!/bin/sh\nprintf 'bound-sidecar'")
            .unwrap_or_else(|error| panic!("write sidecar binding fixture: {error}"));
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|error| panic!("make sidecar binding fixture executable: {error}"));
        let bound = BoundSidecar::bind(&source)
            .unwrap_or_else(|error| panic!("bind sidecar fixture: {error}"));
        std::fs::write(&source, "#!/bin/sh\nprintf 'replacement'")
            .unwrap_or_else(|error| panic!("replace sidecar fixture: {error}"));
        let output = Command::new(bound.launch_path())
            .output()
            .unwrap_or_else(|error| panic!("launch bound sidecar fixture: {error}"));
        assert!(output.status.success());
        assert_eq!(output.stdout, b"bound-sidecar");
    }
}
