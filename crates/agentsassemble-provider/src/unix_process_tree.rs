use std::{collections::VecDeque, io, time::Duration};

use rustix::process::{Pid, Signal};

const MAX_TRACKED_DESCENDANTS: usize = 512;

pub(crate) struct CapturedDescendants {
    escaped: Vec<Pid>,
    complete: bool,
}

impl CapturedDescendants {
    pub(crate) fn freeze(provider: Pid, anchored_group: Pid) -> Self {
        let mut capture = Self {
            escaped: Vec::new(),
            complete: true,
        };
        if !matches!(
            rustix::process::kill_process_group(anchored_group, Signal::STOP),
            Ok(())
        ) {
            capture.complete = false;
            return capture;
        }
        let mut pending = VecDeque::from([provider]);
        let mut seen = std::collections::HashSet::from([provider.as_raw_pid()]);
        while let Some(parent) = pending.pop_front() {
            let Ok(children) = list_children(parent) else {
                capture.complete = false;
                continue;
            };
            for child in children {
                if !seen.insert(child.as_raw_pid()) {
                    continue;
                }
                if seen.len() > MAX_TRACKED_DESCENDANTS {
                    capture.complete = false;
                    break;
                }
                match rustix::process::getpgid(Some(child)) {
                    Ok(group) if group == anchored_group => {}
                    Ok(_) => {
                        if rustix::process::kill_process(child, Signal::STOP).is_err() {
                            capture.complete = false;
                            continue;
                        }
                        capture.escaped.push(child);
                    }
                    Err(rustix::io::Errno::SRCH) => continue,
                    Err(_) => {
                        capture.complete = false;
                        continue;
                    }
                }
                pending.push_back(child);
            }
        }
        capture
    }

    pub(crate) fn kill(&self) {
        for process in self.escaped.iter().rev() {
            let _ = rustix::process::kill_process(*process, Signal::KILL);
        }
    }

    pub(crate) async fn confirm_gone(&self) -> io::Result<()> {
        if !self.complete {
            return Err(io::Error::other(
                "provider descendant capture was incomplete",
            ));
        }
        loop {
            let mut any_running = false;
            for process in &self.escaped {
                match rustix::process::test_kill_process(*process) {
                    Err(rustix::io::Errno::SRCH) => {}
                    Ok(()) | Err(rustix::io::Errno::PERM) => any_running = true,
                    Err(error) => return Err(error.into()),
                }
            }
            if !any_running {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn list_children(parent: Pid) -> io::Result<Vec<Pid>> {
    use std::io::Read;

    let path = format!("/proc/{0}/task/{0}/children", parent.as_raw_pid());
    let mut children = String::new();
    match std::fs::File::open(path) {
        Ok(file) => {
            file.take(32 * 1024).read_to_string(&mut children)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    }
    parse_children(&children)
}

#[cfg(target_os = "macos")]
fn list_children(parent: Pid) -> io::Result<Vec<Pid>> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

    let parent = u32::try_from(parent.as_raw_pid())
        .map(sysinfo::Pid::from_u32)
        .map_err(|_| io::Error::other("provider parent pid is invalid"))?;
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    let mut children = Vec::new();
    for (pid, process) in system.processes() {
        if process.parent() != Some(parent) {
            continue;
        }
        let child = i32::try_from(pid.as_u32())
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("provider descendant pid is invalid"))?;
        children.push(child);
        if children.len() > MAX_TRACKED_DESCENDANTS {
            return Err(io::Error::other("provider descendant budget was exceeded"));
        }
    }
    Ok(children)
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn list_children(_parent: Pid) -> io::Result<Vec<Pid>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "provider descendant capture is unsupported",
    ))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn parse_children(value: &str) -> io::Result<Vec<Pid>> {
    let mut children = Vec::new();
    for raw in value.split_ascii_whitespace() {
        let pid = raw
            .parse::<i32>()
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("provider descendant pid is invalid"))?;
        children.push(pid);
        if children.len() > MAX_TRACKED_DESCENDANTS {
            return Err(io::Error::other("provider descendant budget was exceeded"));
        }
    }
    Ok(children)
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    #[test]
    fn proc_children_are_bounded_and_strict() {
        assert_eq!(
            super::parse_children("12 34")
                .unwrap_or_else(|error| panic!("parse proc children: {error}"))
                .iter()
                .map(|pid| pid.as_raw_pid())
                .collect::<Vec<_>>(),
            vec![12, 34]
        );
        assert!(super::parse_children("not-a-pid").is_err());
    }
}
