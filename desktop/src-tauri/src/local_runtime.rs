use std::{
    env,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Stdio},
    sync::{Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use agentsassemble_domain::validate_room_id;
use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use url::Url;
use uuid::Uuid;

use crate::runtime_supervisor;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(3);
const RUNTIME_LOG_LIMIT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct TicketGrant {
    ticket: String,
    ttl_seconds: u64,
    websocket_base_url: String,
    server_proof_key: String,
}

#[derive(Default)]
pub struct LocalRuntime {
    process: Mutex<Option<RuntimeProcess>>,
}

struct RuntimeProcess {
    child: Child,
    control: Option<ChildStdin>,
    output: mpsc::Receiver<Result<RuntimeOutput, String>>,
    address: Url,
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

enum RuntimeOutput {
    Startup(StartupRecord),
    Control(LocalControlResponse),
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
    make_private_directory(&data_root)
        .map_err(|error| format!("cannot secure {}: {error}", data_root.display()))?;
    let executable = sidecar_executable(app)?;
    let database = data_root.join("runtime.sqlite3");
    let secret = generate_host_secret();
    let stdout_path = data_root.join("runtime.stdout.log");
    let stderr_path = data_root.join("runtime.stderr.log");
    let stdout_log = open_private_rotating_log(&stdout_path)?;
    let stderr_log = open_private_rotating_log(&stderr_path)?;
    let mut command = runtime_supervisor::command(&executable)?;
    command
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--database")
        .arg(&database)
        .arg("--initialize-room")
        .arg(room_id)
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        format!(
            "cannot start Rust runtime supervisor for {}: {error}",
            executable.display()
        )
    })?;
    let Some(stderr) = child.stderr.take() else {
        abort_startup(&mut child, None);
        return Err("cannot capture Rust runtime diagnostics".to_owned());
    };
    capture_capped_stderr(stderr, stderr_log);
    let Some(mut control) = child.stdin.take() else {
        abort_startup(&mut child, None);
        return Err("cannot open Rust runtime control pipe".to_owned());
    };
    if let Err(error) = writeln!(control, "{secret}").and_then(|()| control.flush()) {
        abort_startup(&mut child, Some(control));
        return Err(format!(
            "cannot initialize Rust runtime control pipe: {error}"
        ));
    }
    let Some(stdout) = child.stdout.take() else {
        abort_startup(&mut child, Some(control));
        return Err("cannot capture Rust runtime startup output".to_owned());
    };
    let output = capture_runtime_output(stdout, stdout_log);
    let record = match wait_for_startup(&mut child, &output) {
        Ok(record) => record,
        Err(error) => {
            abort_startup(&mut child, Some(control));
            return Err(format!("{error} Details: {}", stderr_path.display()));
        }
    };
    let address = match validate_startup_record(&record) {
        Ok(address) => address,
        Err(error) => {
            abort_startup(&mut child, Some(control));
            return Err(error);
        }
    };
    if let Err(error) = prove_listening(&address) {
        abort_startup(&mut child, Some(control));
        return Err(error);
    }
    Ok(RuntimeProcess {
        child,
        control: Some(control),
        output,
        address,
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
    let request_id = Uuid::new_v4().to_string();
    let request = LocalControlRequest::IssueTicket {
        request_id: request_id.clone(),
        meeting_id: room_id.to_owned(),
    };
    let mut encoded = serde_json::to_vec(&request).map_err(|error| {
        TicketFailure::Broken(format!("cannot encode local ticket request: {error}"))
    })?;
    encoded.push(b'\n');
    let control = runtime
        .control
        .as_mut()
        .ok_or_else(|| TicketFailure::Broken("local runtime control pipe is closed".to_owned()))?;
    control
        .write_all(&encoded)
        .and_then(|()| control.flush())
        .map_err(|error| {
            TicketFailure::Broken(format!("cannot write local ticket request: {error}"))
        })?;
    let response = runtime
        .output
        .recv_timeout(REQUEST_TIMEOUT)
        .map_err(|error| {
            TicketFailure::Broken(format!("local runtime ticket response timed out: {error}"))
        })?
        .map_err(TicketFailure::Broken)?;
    let RuntimeOutput::Control(response) = response else {
        return Err(TicketFailure::Broken(
            "local runtime returned a duplicate startup record".to_owned(),
        ));
    };
    let (ticket, ttl_seconds, server_proof_key) = match response {
        LocalControlResponse::Ok {
            request_id: response_id,
            ticket,
            ttl_seconds,
            server_proof_key,
        } if response_id == request_id => (ticket, ttl_seconds, server_proof_key),
        LocalControlResponse::Error {
            request_id: response_id,
            code,
            message,
        } if response_id == request_id => {
            return if is_application_rejection(&code) {
                Err(TicketFailure::Rejected(message))
            } else {
                Err(TicketFailure::Broken(message))
            };
        }
        _ => {
            return Err(TicketFailure::Broken(
                "local runtime ticket response id did not match the request".to_owned(),
            ));
        }
    };
    if ticket.is_empty()
        || ttl_seconds == 0
        || server_proof_key.len() != 64
        || !server_proof_key
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(TicketFailure::Broken(
            "local runtime returned an invalid ticket grant".to_owned(),
        ));
    }
    let port = runtime
        .address
        .port()
        .ok_or_else(|| TicketFailure::Broken("local runtime address has no port".to_owned()))?;
    Ok(TicketGrant {
        ticket,
        ttl_seconds,
        websocket_base_url: format!("ws://127.0.0.1:{port}"),
        server_proof_key,
    })
}

fn is_application_rejection(code: &str) -> bool {
    matches!(code, "bad_request" | "room_not_found" | "session_revoked")
}

fn generate_host_secret() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn open_private_rotating_log(path: &Path) -> Result<File, String> {
    let mut previous_name = path.as_os_str().to_os_string();
    previous_name.push(".previous");
    let previous = PathBuf::from(previous_name);
    if path.exists() {
        if previous.exists() {
            fs::remove_file(&previous)
                .map_err(|error| format!("cannot replace {}: {error}", previous.display()))?;
        }
        fs::rename(path, &previous)
            .map_err(|error| format!("cannot rotate {}: {error}", path.display()))?;
    }
    let file =
        File::create(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    make_private_file(&file)
        .map_err(|error| format!("cannot secure {}: {error}", path.display()))?;
    Ok(file)
}

fn capture_capped_stderr(stderr: ChildStderr, mut log: File) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let _ = copy_capped(&mut reader, &mut log, RUNTIME_LOG_LIMIT_BYTES);
        let _ = log.flush();
    });
}

fn copy_capped(
    reader: &mut impl Read,
    writer: &mut impl Write,
    limit: u64,
) -> std::io::Result<u64> {
    let mut buffer = [0_u8; 8 * 1024];
    let mut written = 0_u64;
    while let Ok(count) = reader.read(&mut buffer) {
        if count == 0 {
            break;
        }
        let remaining = limit.saturating_sub(written);
        let allowed = usize::try_from(remaining.min(count as u64)).unwrap_or(0);
        if allowed > 0 {
            writer.write_all(&buffer[..allowed])?;
            written = written.saturating_add(allowed as u64);
        }
    }
    Ok(written)
}

#[cfg(unix)]
fn make_private_directory(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn make_private_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_private_file(file: &File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn make_private_file(_file: &File) -> std::io::Result<()> {
    Ok(())
}

fn capture_runtime_output(
    stdout: ChildStdout,
    mut log: File,
) -> mpsc::Receiver<Result<RuntimeOutput, String>> {
    let (sender, receiver) = mpsc::sync_channel(8);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if !line.trim().is_empty() {
                        let parsed = parse_runtime_output(line.trim());
                        if matches!(parsed, Ok(RuntimeOutput::Startup(_))) {
                            let _ = log.write_all(line.as_bytes());
                            let _ = log.flush();
                        }
                        let _ = sender.send(parsed);
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(format!("cannot read Rust runtime output: {error}")));
                    break;
                }
            }
        }
    });
    receiver
}

fn parse_runtime_output(line: &str) -> Result<RuntimeOutput, String> {
    let value: serde_json::Value = serde_json::from_str(line)
        .map_err(|error| format!("invalid Rust runtime output: {error}"))?;
    if value.get("status").and_then(serde_json::Value::as_str) == Some("ready") {
        serde_json::from_value(value)
            .map(RuntimeOutput::Startup)
            .map_err(|error| format!("invalid Rust runtime startup record: {error}"))
    } else {
        serde_json::from_value(value)
            .map(RuntimeOutput::Control)
            .map_err(|error| format!("invalid Rust runtime control response: {error}"))
    }
}

fn wait_for_startup(
    child: &mut Child,
    records: &mpsc::Receiver<Result<RuntimeOutput, String>>,
) -> Result<StartupRecord, String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    while Instant::now() < deadline {
        match records.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(RuntimeOutput::Startup(record))) => return Ok(record),
            Ok(Ok(RuntimeOutput::Control(_))) => {
                return Err("Rust runtime sent a control response before readiness".to_owned());
            }
            Ok(Err(error)) => return Err(error),
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

fn validate_startup_record(record: &StartupRecord) -> Result<Url, String> {
    if record.status != "ready" || record.runtime != "rust" || record.pid == 0 {
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
        let _ = runtime.child.kill();
        let _ = runtime.child.wait();
    }
}

fn abort_startup(child: &mut Child, control: Option<ChildStdin>) {
    drop(control);
    let deadline = Instant::now() + SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
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

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};

    use super::{
        RUNTIME_LOG_LIMIT_BYTES, StartupRecord, copy_capped, generate_host_secret,
        is_application_rejection, open_private_rotating_log, validate_startup_record,
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
        assert!(validate_startup_record(&record).is_ok());
        let unsafe_record = StartupRecord {
            address: "http://localhost:43123".to_owned(),
            ..record
        };
        assert!(validate_startup_record(&unsafe_record).is_err());
    }

    #[test]
    fn only_normal_client_rejections_preserve_the_runtime() {
        assert!(is_application_rejection("bad_request"));
        assert!(is_application_rejection("room_not_found"));
        assert!(!is_application_rejection("persistence_failed"));
        assert!(!is_application_rejection("unavailable"));
    }

    #[test]
    fn runtime_log_writer_drains_but_caps_persisted_bytes() {
        let input = vec![b'x'; usize::try_from(RUNTIME_LOG_LIMIT_BYTES + 4096).unwrap_or(0)];
        let mut reader = Cursor::new(input);
        let mut output = Vec::new();
        let written = copy_capped(&mut reader, &mut output, RUNTIME_LOG_LIMIT_BYTES)
            .unwrap_or_else(|error| panic!("copy capped log: {error}"));
        assert_eq!(written, RUNTIME_LOG_LIMIT_BYTES);
        assert_eq!(output.len() as u64, RUNTIME_LOG_LIMIT_BYTES);
        let mut remaining = Vec::new();
        reader
            .read_to_end(&mut remaining)
            .unwrap_or_else(|error| panic!("inspect drained input: {error}"));
        assert!(remaining.is_empty());
    }

    #[test]
    fn runtime_logs_rotate_one_private_previous_generation() {
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create log test directory: {error}"));
        let path = directory.path().join("runtime.stderr.log");
        let mut first = open_private_rotating_log(&path)
            .unwrap_or_else(|error| panic!("open first log: {error}"));
        first
            .write_all(b"first generation")
            .unwrap_or_else(|error| panic!("write first log: {error}"));
        drop(first);
        let _second = open_private_rotating_log(&path)
            .unwrap_or_else(|error| panic!("open second log: {error}"));
        let previous = directory.path().join("runtime.stderr.log.previous");
        assert_eq!(
            std::fs::read_to_string(previous)
                .unwrap_or_else(|error| panic!("read previous log: {error}")),
            "first generation"
        );
    }
}
