use std::{
    io::{self, Write},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const SUPERVISOR_FLAG: &str = "--agentsassemble-runtime-supervisor";
const SIDECAR_SHUTDOWN_GRACE: Duration = Duration::from_secs(3);

pub(crate) fn command(executable: &std::path::Path) -> Result<Command, String> {
    let current = std::env::current_exe()
        .map_err(|error| format!("cannot resolve desktop supervisor executable: {error}"))?;
    let mut command = Command::new(current);
    command.arg(SUPERVISOR_FLAG).arg(executable);
    Ok(command)
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
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    configure_process_group(&mut command);
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
    thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        let _ = io::copy(&mut sidecar_output, &mut stdout);
        let _ = stdout.flush();
    });

    loop {
        if let Ok(copy_result) = parent_closed.try_recv() {
            terminate_sidecar(&mut sidecar);
            return copy_result;
        }
        if sidecar.try_wait()?.is_some() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_sidecar(child: &mut Child) {
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };

    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    let group = Pid::from_raw(-pid);
    let _ = kill(group, Signal::SIGINT);
    let deadline = Instant::now() + SIDECAR_SHUTDOWN_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = kill(group, Signal::SIGKILL);
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_sidecar(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::run_if_requested;

    #[test]
    fn ordinary_desktop_invocation_does_not_enter_supervisor_mode() {
        assert_eq!(run_if_requested(), None);
    }
}
