use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

use agentsassemble_protocol::{LocalControlRequest, LocalControlResponse};

use super::super::{
    RuntimeProcess, capture_runtime_output,
    control::{TicketFailure, request_operator_http_ticket},
    handle_ticket_result, terminate_owned_runtime,
};

const CHILD_ENV: &str = "AGENTSASSEMBLE_CONTROL_EXCHANGE_FIXTURE";

struct Fixture {
    process: Option<RuntimeProcess>,
    actions: mpsc::Sender<u8>,
    requests: mpsc::Receiver<String>,
    owner: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("bind: {e}"));
        let mut command =
            Command::new(std::env::current_exe().unwrap_or_else(|e| panic!("exe: {e}")));
        command
            .args([
                "--exact",
                "local_runtime::control_exchange::tests::exchange_child",
                "--nocapture",
            ])
            .env(
                CHILD_ENV,
                listener
                    .local_addr()
                    .unwrap_or_else(|e| panic!("address: {e}"))
                    .to_string(),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().unwrap_or_else(|e| panic!("spawn: {e}"));
        let control = child.stdin.take();
        let mut stdout = BufReader::new(child.stdout.take().unwrap_or_else(|| panic!("stdout")));
        // Discard only libtest's startup text before the fixture's explicit barrier.
        loop {
            let mut line = String::new();
            assert!(
                stdout
                    .read_line(&mut line)
                    .unwrap_or_else(|e| panic!("ready: {e}"))
                    > 0
            );
            if line.trim() == "exchange-ready" {
                break;
            }
        }
        let output = capture_runtime_output(
            stdout.into_inner(),
            tempfile::tempfile().unwrap_or_else(|e| panic!("log: {e}")),
        );
        let (stream, _) = listener.accept().unwrap_or_else(|e| panic!("accept: {e}"));
        let (actions, action_rx) = mpsc::channel();
        let (request_tx, requests) = mpsc::channel();
        let owner = thread::spawn(move || {
            let mut stream = BufReader::new(stream);
            loop {
                let mut request = String::new();
                if !matches!(stream.read_line(&mut request), Ok(n) if n > 0) {
                    break;
                }
                if request_tx.send(request.trim().to_owned()).is_err() {
                    break;
                }
                let action = action_rx.recv().unwrap_or(b'q');
                if stream.get_mut().write_all(&[action]).is_err() || action == b'q' {
                    break;
                }
            }
        });
        Self {
            process: Some(RuntimeProcess {
                child,
                control,
                output,
                pending_response: None,
                address: url::Url::parse("http://127.0.0.1:43123")
                    .unwrap_or_else(|e| panic!("url: {e}")),
            }),
            actions,
            requests,
            owner: Some(owner),
        }
    }

    fn ticket(&mut self) -> Result<super::super::HttpTicketGrant, String> {
        let result = request_operator_http_ticket(
            self.process.as_mut().unwrap_or_else(|| panic!("runtime")),
        );
        handle_ticket_result(&mut self.process, result)
    }

    fn release(&self, action: u8) {
        self.actions
            .send(action)
            .unwrap_or_else(|e| panic!("release: {e}"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.actions.send(b'q');
        if let Some(mut runtime) = self.process.take() {
            terminate_owned_runtime(&mut runtime);
        }
        if let Some(owner) = self.owner.take() {
            let _ = owner.join();
        }
    }
}

#[test]
fn timed_out_response_retains_its_decoder_and_never_queues_a_second_request() {
    let mut fixture = Fixture::new();
    let pid = fixture
        .process
        .as_ref()
        .unwrap_or_else(|| panic!("runtime"))
        .child
        .id();
    assert!(fixture.ticket().is_err());
    let first = fixture
        .requests
        .recv()
        .unwrap_or_else(|e| panic!("request: {e}"));
    assert!(fixture.ticket().is_err());
    assert!(matches!(
        fixture.requests.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert_eq!(
        fixture
            .process
            .as_ref()
            .unwrap_or_else(|| panic!("retained runtime"))
            .child
            .id(),
        pid
    );
    // One release finishes the old request. The next releases belong to fresh calls.
    fixture.release(b'a');
    fixture.release(b'a');
    fixture.release(b'a');
    let second = fixture
        .ticket()
        .unwrap_or_else(|e| panic!("fresh ticket: {e}"));
    assert_eq!(second.ticket, "b".repeat(64));
    let second_id = fixture
        .requests
        .recv()
        .unwrap_or_else(|e| panic!("second: {e}"));
    assert_ne!(first, second_id);
    let third = fixture
        .ticket()
        .unwrap_or_else(|e| panic!("continued ticket: {e}"));
    assert_eq!(third.ticket, "c".repeat(64));
}

#[test]
fn late_invalid_response_is_still_rejected_by_the_original_decoder() {
    for action in *b"ipvjq" {
        let mut fixture = Fixture::new();
        assert!(fixture.ticket().is_err());
        fixture.release(action);
        assert!(fixture.ticket().is_err());
        assert!(
            fixture.process.is_none(),
            "invalid late response must stop its runtime"
        );
    }
}

#[test]
fn explicit_stop_closes_control_even_when_a_response_is_pending() {
    let mut fixture = Fixture::new();
    let result = request_operator_http_ticket(
        fixture
            .process
            .as_mut()
            .unwrap_or_else(|| panic!("runtime")),
    );
    assert!(matches!(result, Err(TicketFailure::Unavailable(_))));
    // Let the admitted response finish, but keep it pending in the native owner.
    fixture.release(b'a');
    let mut runtime = fixture.process.take().unwrap_or_else(|| panic!("runtime"));
    terminate_owned_runtime(&mut runtime);
    assert!(
        runtime
            .child
            .try_wait()
            .unwrap_or_else(|e| panic!("reap: {e}"))
            .is_some()
    );
}

#[test]
fn exchange_child() {
    let Ok(address) = std::env::var(CHILD_ENV) else {
        return;
    };
    let mut gate = TcpStream::connect(address).unwrap_or_else(|e| panic!("gate: {e}"));
    println!("\nexchange-ready");
    std::io::stdout()
        .flush()
        .unwrap_or_else(|e| panic!("flush: {e}"));
    for (index, line) in std::io::stdin().lock().lines().enumerate() {
        let request: LocalControlRequest =
            serde_json::from_str(&line.unwrap_or_else(|e| panic!("input: {e}")))
                .unwrap_or_else(|e| panic!("json: {e}"));
        let LocalControlRequest::IssueOperatorHttpTicket { request_id } = request else {
            panic!("purpose");
        };
        writeln!(gate, "{request_id}").unwrap_or_else(|e| panic!("request: {e}"));
        let mut action = [0];
        if gate.read_exact(&mut action).is_err() || action == *b"q" {
            break;
        }
        let ticket = char::from(b'a' + u8::try_from(index).unwrap_or(0))
            .to_string()
            .repeat(64);
        let response = match action[0] {
            b'i' => LocalControlResponse::OperatorHttpOk {
                request_id: "wrong-id".into(),
                ticket,
                ttl_seconds: 30,
            },
            b'p' => LocalControlResponse::PreferencesReadOk {
                request_id,
                ticket,
                ttl_seconds: 30,
            },
            b'v' => LocalControlResponse::OperatorHttpOk {
                request_id,
                ticket: String::new(),
                ttl_seconds: 0,
            },
            b'j' => {
                println!("not json");
                std::io::stdout()
                    .flush()
                    .unwrap_or_else(|e| panic!("flush: {e}"));
                continue;
            }
            _ => LocalControlResponse::OperatorHttpOk {
                request_id,
                ticket,
                ttl_seconds: 30,
            },
        };
        println!(
            "{}",
            serde_json::to_string(&response).unwrap_or_else(|e| panic!("encode: {e}"))
        );
        std::io::stdout()
            .flush()
            .unwrap_or_else(|e| panic!("flush: {e}"));
    }
    std::process::exit(0);
}
