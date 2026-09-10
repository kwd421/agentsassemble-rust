//! Real private-pipe EOF, supervisor lifetime, and emergency group termination.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

const FIXTURE_ENV: &str = "AGENTSASSEMBLE_TEST_SHUTDOWN_SIDECAR";

#[test]
fn supervisor_entry() {
    let Some(script) = std::env::var_os(FIXTURE_ENV) else {
        return;
    };
    let result = super::supervise(std::path::Path::new(&script), &[]);
    assert!(result.is_ok(), "fixture supervisor failed: {result:?}");
}

struct Fixture {
    child: Option<Child>,
    gate: TcpStream,
    _directory: tempfile::TempDir,
}

impl Fixture {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let script = directory.path().join("sidecar");
        std::fs::write(
            &script,
            format!(
                "#!/usr/bin/env python3\nimport json,os,socket,sys\ns=socket.create_connection(('127.0.0.1',{}))\nprint(json.dumps(dict(status='ready',runtime='rust',pid=os.getpid(),address='http://127.0.0.1:1234')),flush=True)\ns.sendall(b'R')\nsys.stdin.buffer.read()\ns.sendall(b'C')\ns.recv(1)\ns.sendall(b'D')\n",
                listener.local_addr()?.port(),
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "runtime_supervisor::shutdown_tests::supervisor_entry",
                "--nocapture",
            ])
            .env(FIXTURE_ENV, &script)
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        listener.set_nonblocking(true)?;
        let mut child = Some(child);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break Ok(stream),
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => break Err(error),
            }
        };
        let gate = match stream {
            Ok(gate) => gate,
            Err(error) => {
                if let Some(mut child) = child.take() {
                    let _ = super::join_supervisor(&mut child, Duration::ZERO);
                }
                return Err(error.into());
            }
        };
        gate.set_nonblocking(false)?;
        gate.set_read_timeout(Some(Duration::from_secs(10)))?;
        let mut fixture = Self {
            child,
            gate,
            _directory: directory,
        };
        fixture.read_marker(b'R')?;
        Ok(fixture)
    }

    fn close_control(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.child
            .as_mut()
            .ok_or("missing fixture supervisor")?
            .stdin
            .take();
        self.read_marker(b'C')
    }

    fn read_marker(&mut self, expected: u8) -> Result<(), Box<dyn std::error::Error>> {
        let mut marker = [0];
        self.gate.read_exact(&mut marker)?;
        assert_eq!(marker[0], expected);
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = super::join_supervisor(&mut child, Duration::ZERO);
        }
    }
}

#[test]
fn desktop_joins_cleanup_beyond_both_previous_shutdown_deadlines()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = Fixture::start()?;
    fixture.close_control()?;
    let mut child = fixture.child.take().ok_or("missing fixture supervisor")?;
    let (complete, received) = mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let result = super::join_supervisor(&mut child, super::DESKTOP_SHUTDOWN_GRACE);
        let _ = complete.send(result);
    });
    // The EOF/cleanup barrier owns order. This timed receive specifically tests
    // that neither the old 3s parent nor the old 16s supervisor cuts it short.
    let early = received.recv_timeout(Duration::from_secs(17));
    let waited_full = matches!(early, Err(mpsc::RecvTimeoutError::Timeout));
    let release = fixture.gate.write_all(b"X");
    let completed = match early {
        Err(mpsc::RecvTimeoutError::Timeout) => received.recv_timeout(Duration::from_secs(10))?,
        Ok(result) => result,
        Err(error) => return Err(error.into()),
    };
    waiter.join().map_err(|_| "shutdown waiter panicked")?;
    assert!(waited_full);
    release?;
    completed?;
    fixture.read_marker(b'D')?;
    assert_eq!(fixture.gate.read(&mut [0])?, 0);
    Ok(())
}

#[test]
fn emergency_stop_kills_the_sidecar_even_when_its_supervisor_cannot_run()
-> Result<(), Box<dyn std::error::Error>> {
    let mut fixture = Fixture::start()?;
    fixture.close_control()?;
    let child = fixture.child.as_mut().ok_or("missing fixture supervisor")?;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(i32::try_from(child.id())?),
        nix::sys::signal::Signal::SIGSTOP,
    )?;
    let result = super::join_supervisor(child, Duration::ZERO);
    fixture.child.take();
    assert_eq!(
        result.err().ok_or("expected emergency timeout")?.kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        fixture.gate.read(&mut [0])?,
        0,
        "the owned sidecar must also exit"
    );
    Ok(())
}
