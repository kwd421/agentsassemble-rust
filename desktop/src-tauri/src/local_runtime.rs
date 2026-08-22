use std::{
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use agentsassemble_domain::validate_room_id;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use url::Url;
use uuid::Uuid;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize)]
pub struct TicketGrant {
    ticket: String,
    ttl_seconds: u64,
    websocket_base_url: String,
}

#[derive(Default)]
pub struct LocalRuntime {
    process: Mutex<Option<RuntimeProcess>>,
}

struct RuntimeProcess {
    child: Child,
    control: Option<ChildStdin>,
    watchdog: ParentWatchdog,
    address: Url,
    secret: SecretString,
}

struct ParentWatchdog {
    child: Child,
    control: Option<ChildStdin>,
}

enum TicketFailure {
    Rejected(String),
    Broken(String),
}

#[derive(Debug, Deserialize)]
struct StartupRecord {
    status: String,
    runtime: String,
    address: String,
    pid: u32,
}

#[derive(Debug, Deserialize)]
struct TicketResponse {
    ticket: String,
    ttl_seconds: u64,
}

impl LocalRuntime {
    pub fn issue_ticket(
        &self,
        app: &AppHandle,
        requested_room_id: &str,
    ) -> Result<TicketGrant, String> {
        let room_id = validate_room_id(requested_room_id)
            .map_err(|error| format!("invalid room id: {}", error.message))?;
        let mut process = self
            .process
            .lock()
            .map_err(|_| "local runtime state lock is poisoned".to_owned())?;
        let must_start = match process.as_mut() {
            Some(runtime) => runtime
                .child
                .try_wait()
                .map_err(|error| format!("cannot inspect local runtime: {error}"))?
                .is_some(),
            None => true,
        };
        if must_start {
            if let Some(mut stopped) = process.take() {
                terminate_owned_runtime(&mut stopped);
            }
            *process = Some(start_runtime(app, &room_id)?);
        }
        let runtime = process
            .as_mut()
            .ok_or_else(|| "local runtime did not start".to_owned())?;
        let result = request_ticket(runtime, &room_id);
        match result {
            Ok(grant) => Ok(grant),
            Err(TicketFailure::Rejected(message)) => Err(message),
            Err(TicketFailure::Broken(message)) => {
                if let Some(mut broken) = process.take() {
                    terminate_owned_runtime(&mut broken);
                }
                Err(format!(
                    "{message}; the owned runtime was stopped and will restart on the next attempt"
                ))
            }
        }
    }

    pub fn stop(&self) {
        let Ok(mut process) = self.process.lock() else {
            return;
        };
        if let Some(mut runtime) = process.take() {
            terminate_owned_runtime(&mut runtime);
        }
    }
}

fn start_runtime(app: &AppHandle, room_id: &str) -> Result<RuntimeProcess, String> {
    let data_root = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve desktop data directory: {error}"))?;
    fs::create_dir_all(&data_root)
        .map_err(|error| format!("cannot create {}: {error}", data_root.display()))?;
    let executable = sidecar_executable(app)?;
    let database = data_root.join("runtime.sqlite3");
    let secret = SecretString::from(generate_host_secret());
    let stdout_path = data_root.join("runtime.stdout.log");
    let stderr_path = data_root.join("runtime.stderr.log");
    let stdout_log = File::create(&stdout_path)
        .map_err(|error| format!("cannot open {}: {error}", stdout_path.display()))?;
    let stderr_log = File::create(&stderr_path)
        .map_err(|error| format!("cannot open {}: {error}", stderr_path.display()))?;
    let mut command = Command::new(&executable);
    command
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--database")
        .arg(&database)
        .arg("--bootstrap-room")
        .arg(room_id)
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr_log));
    configure_process_group(&mut command);
    let mut child = command.spawn().map_err(|error| {
        format!(
            "cannot start Rust runtime {}: {error}",
            executable.display()
        )
    })?;
    let mut watchdog = match start_parent_watchdog(child.id()) {
        Ok(watchdog) => watchdog,
        Err(error) => {
            terminate_owned_process(&mut child);
            return Err(error);
        }
    };
    let Some(mut control) = child.stdin.take() else {
        abort_startup(&mut child, &mut watchdog);
        return Err("cannot open Rust runtime control pipe".to_owned());
    };
    if let Err(error) =
        writeln!(control, "{}", secret.expose_secret()).and_then(|()| control.flush())
    {
        abort_startup(&mut child, &mut watchdog);
        return Err(format!(
            "cannot initialize Rust runtime control pipe: {error}"
        ));
    }
    let Some(stdout) = child.stdout.take() else {
        abort_startup(&mut child, &mut watchdog);
        return Err("cannot capture Rust runtime startup output".to_owned());
    };
    let records = capture_startup_record(stdout, stdout_log);
    let record = match wait_for_startup(&mut child, &records) {
        Ok(record) => record,
        Err(error) => {
            abort_startup(&mut child, &mut watchdog);
            return Err(format!("{error} Details: {}", stderr_path.display()));
        }
    };
    let address = match validate_startup_record(&record, child.id()) {
        Ok(address) => address,
        Err(error) => {
            abort_startup(&mut child, &mut watchdog);
            return Err(error);
        }
    };
    if let Err(error) = prove_listening(&address) {
        abort_startup(&mut child, &mut watchdog);
        return Err(error);
    }
    Ok(RuntimeProcess {
        child,
        control: Some(control),
        watchdog,
        address,
        secret,
    })
}

fn request_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<TicketGrant, TicketFailure> {
    if runtime
        .child
        .try_wait()
        .map_err(|error| TicketFailure::Broken(format!("cannot inspect local runtime: {error}")))?
        .is_some()
    {
        return Err(TicketFailure::Broken(
            "the owned Rust runtime exited before ticket issuance".to_owned(),
        ));
    }
    let endpoint = runtime.address.join("api/ws-ticket").map_err(|error| {
        TicketFailure::Broken(format!("cannot construct ticket endpoint: {error}"))
    })?;
    let response = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| {
            TicketFailure::Broken(format!("cannot create local ticket client: {error}"))
        })?
        .post(endpoint)
        .header("x-host-token", runtime.secret.expose_secret())
        .json(&serde_json::json!({"meeting_id": room_id}))
        .send()
        .map_err(|error| {
            TicketFailure::Broken(format!("local runtime ticket request failed: {error}"))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let message = format!("local runtime rejected ticket request with status {status}");
        return if is_application_rejection(status) {
            Err(TicketFailure::Rejected(message))
        } else {
            Err(TicketFailure::Broken(message))
        };
    }
    let ticket: TicketResponse = response.json().map_err(|error| {
        TicketFailure::Broken(format!("local runtime returned an invalid ticket: {error}"))
    })?;
    if ticket.ticket.is_empty() || ticket.ttl_seconds == 0 {
        return Err(TicketFailure::Broken(
            "local runtime returned an empty or expired ticket".to_owned(),
        ));
    }
    let port = runtime
        .address
        .port()
        .ok_or_else(|| TicketFailure::Broken("local runtime address has no port".to_owned()))?;
    Ok(TicketGrant {
        ticket: ticket.ticket,
        ttl_seconds: ticket.ttl_seconds,
        websocket_base_url: format!("ws://127.0.0.1:{port}"),
    })
}

fn is_application_rejection(status: reqwest::StatusCode) -> bool {
    status.is_client_error() && status != reqwest::StatusCode::UNAUTHORIZED
}

fn generate_host_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn capture_startup_record(
    stdout: ChildStdout,
    mut log: File,
) -> mpsc::Receiver<Result<StartupRecord, String>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut reported = false;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let _ = log.write_all(line.as_bytes());
                    let _ = log.flush();
                    if !reported && !line.trim().is_empty() {
                        reported = true;
                        let result =
                            serde_json::from_str::<StartupRecord>(line.trim()).map_err(|error| {
                                format!("invalid Rust runtime startup record: {error}")
                            });
                        let _ = sender.send(result);
                    }
                }
                Err(error) => {
                    if !reported {
                        let _ = sender.send(Err(format!(
                            "cannot read Rust runtime startup output: {error}"
                        )));
                    }
                    break;
                }
            }
        }
    });
    receiver
}

fn wait_for_startup(
    child: &mut Child,
    records: &mpsc::Receiver<Result<StartupRecord, String>>,
) -> Result<StartupRecord, String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        match records.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("Rust runtime closed stdout before readiness".to_owned());
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("cannot inspect Rust runtime startup: {error}"))?
        {
            return Err(format!(
                "Rust runtime exited before readiness with {status}"
            ));
        }
    }
    Err("Rust runtime did not report readiness within 20 seconds".to_owned())
}

fn validate_startup_record(record: &StartupRecord, expected_pid: u32) -> Result<Url, String> {
    if record.status != "ready" || record.runtime != "rust" || record.pid != expected_pid {
        return Err("Rust runtime startup identity did not match the owned process".to_owned());
    }
    let address = Url::parse(&record.address)
        .map_err(|error| format!("Rust runtime reported an invalid address: {error}"))?;
    if address.scheme() != "http"
        || address.host_str() != Some("127.0.0.1")
        || address.port().is_none_or(|port| port == 0)
        || address.username() != ""
        || address.password().is_some()
        || address.path() != "/"
        || address.query().is_some()
        || address.fragment().is_some()
    {
        return Err("Rust runtime reported a non-loopback or unsafe address".to_owned());
    }
    Ok(address)
}

fn prove_listening(address: &Url) -> Result<(), String> {
    let port = address
        .port()
        .ok_or_else(|| "Rust runtime address has no port".to_owned())?;
    let socket = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    TcpStream::connect_timeout(&socket, Duration::from_secs(2))
        .map(|_| ())
        .map_err(|error| format!("Rust runtime readiness proof failed: {error}"))
}

fn terminate_owned_runtime(runtime: &mut RuntimeProcess) {
    runtime.control.take();
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    let mut exited = false;
    while Instant::now() < deadline {
        if runtime.child.try_wait().ok().flatten().is_some() {
            exited = true;
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    if !exited {
        terminate_owned_process(&mut runtime.child);
    }
    stop_parent_watchdog(&mut runtime.watchdog);
}

fn abort_startup(child: &mut Child, watchdog: &mut ParentWatchdog) {
    terminate_owned_process(child);
    stop_parent_watchdog(watchdog);
}

#[cfg(unix)]
fn start_parent_watchdog(runtime_pid: u32) -> Result<ParentWatchdog, String> {
    let mut child = Command::new("/bin/sh")
        .args([
            "-c",
            "while IFS= read -r _; do :; done; kill -KILL -- \"-$1\" 2>/dev/null || true",
            "agentsassemble-watchdog",
            &runtime_pid.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start runtime parent watchdog: {error}"))?;
    let control = child
        .stdin
        .take()
        .ok_or_else(|| "runtime parent watchdog has no control pipe".to_owned())?;
    Ok(ParentWatchdog {
        child,
        control: Some(control),
    })
}

#[cfg(windows)]
fn start_parent_watchdog(runtime_pid: u32) -> Result<ParentWatchdog, String> {
    let script = "$null = [Console]::In.ReadToEnd(); taskkill.exe /PID $args[0] /T /F | Out-Null";
    let mut child = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
            &runtime_pid.to_string(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("cannot start runtime parent watchdog: {error}"))?;
    let control = child
        .stdin
        .take()
        .ok_or_else(|| "runtime parent watchdog has no control pipe".to_owned())?;
    Ok(ParentWatchdog {
        child,
        control: Some(control),
    })
}

fn stop_parent_watchdog(watchdog: &mut ParentWatchdog) {
    watchdog.control.take();
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if watchdog.child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = watchdog.child.kill();
    let _ = watchdog.child.wait();
}

fn sidecar_executable(app: &AppHandle) -> Result<PathBuf, String> {
    let executable_name = if cfg!(windows) {
        "agentsassemble-server.exe"
    } else {
        "agentsassemble-server"
    };
    let mut candidates = Vec::new();
    if cfg!(debug_assertions) {
        if let Some(explicit) = env::var_os("AGENTSASSEMBLE_SIDECAR") {
            candidates.push(PathBuf::from(explicit));
        }
        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug")
                .join(executable_name),
        );
    }
    if let Ok(current) = env::current_exe()
        && let Some(parent) = current.parent()
    {
        candidates.push(parent.join(executable_name));
    }
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join(executable_name));
        candidates.push(resources.join("binaries").join(executable_name));
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| "the bundled AgentsAssemble Rust runtime is missing".to_owned())
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_owned_process(child: &mut Child) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };

    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    let group = Pid::from_raw(-pid);
    let _ = kill(group, Signal::SIGINT);
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = kill(group, Signal::SIGKILL);
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_owned_process(child: &mut Child) {
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .status();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::{
        StartupRecord, generate_host_secret, is_application_rejection, validate_startup_record,
    };

    #[test]
    fn generated_host_secret_is_high_entropy_and_unpadded() {
        let secret = generate_host_secret();
        assert_eq!(secret.len(), 64);
        assert!(secret.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    #[test]
    fn startup_record_is_bound_to_the_owned_loopback_process() {
        let record = StartupRecord {
            status: "ready".to_owned(),
            runtime: "rust".to_owned(),
            address: "http://127.0.0.1:43123".to_owned(),
            pid: 42,
        };
        assert!(validate_startup_record(&record, 42).is_ok());
        assert!(validate_startup_record(&record, 43).is_err());
        let unsafe_record = StartupRecord {
            address: "http://localhost:43123".to_owned(),
            ..record
        };
        assert!(validate_startup_record(&unsafe_record, 42).is_err());
    }

    #[test]
    fn only_normal_client_rejections_preserve_the_runtime() {
        assert!(is_application_rejection(reqwest::StatusCode::BAD_REQUEST));
        assert!(is_application_rejection(reqwest::StatusCode::NOT_FOUND));
        assert!(!is_application_rejection(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_application_rejection(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
    }

    #[cfg(unix)]
    #[test]
    fn watchdog_kills_a_stopped_runtime_when_parent_control_closes() {
        use std::{process::Stdio, thread, time::Duration};

        use nix::{
            sys::signal::{Signal, kill},
            unistd::Pid,
        };

        use super::{configure_process_group, start_parent_watchdog, terminate_owned_process};

        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", "while :; do sleep 60; done"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let mut runtime = command
            .spawn()
            .unwrap_or_else(|error| panic!("spawn stopped runtime fixture: {error}"));
        let mut watchdog = start_parent_watchdog(runtime.id())
            .unwrap_or_else(|error| panic!("start parent watchdog: {error}"));
        let runtime_pid = i32::try_from(runtime.id())
            .unwrap_or_else(|error| panic!("convert runtime pid: {error}"));
        kill(Pid::from_raw(runtime_pid), Signal::SIGSTOP)
            .unwrap_or_else(|error| panic!("stop runtime fixture: {error}"));

        watchdog.control.take();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            if runtime
                .try_wait()
                .unwrap_or_else(|error| panic!("inspect runtime fixture: {error}"))
                .is_some()
            {
                break;
            }
            if std::time::Instant::now() >= deadline {
                terminate_owned_process(&mut runtime);
                panic!("watchdog did not kill the stopped runtime");
            }
            thread::sleep(Duration::from_millis(20));
        }
        let watchdog_status = watchdog
            .child
            .wait()
            .unwrap_or_else(|error| panic!("wait for parent watchdog: {error}"));
        assert!(watchdog_status.success());
    }
}
