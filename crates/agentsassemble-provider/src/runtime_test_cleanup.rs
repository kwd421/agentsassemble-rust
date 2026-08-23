pub(super) struct ExactProcessCleanup(Option<u32>);

impl ExactProcessCleanup {
    pub(super) const fn new(pid: u32) -> Self {
        Self(Some(pid))
    }

    pub(super) fn kill_now(&mut self) {
        let Some(raw_pid) = self.0.take() else {
            return;
        };
        if let Some(pid) = i32::try_from(raw_pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
        }
    }

    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ExactProcessCleanup {
    fn drop(&mut self) {
        self.kill_now();
    }
}

pub(super) struct ExactProcessGroupCleanup(Option<u32>);

impl ExactProcessGroupCleanup {
    pub(super) const fn new(pid: u32) -> Self {
        Self(Some(pid))
    }

    pub(super) fn kill_now(&mut self) {
        let Some(raw_pid) = self.0.take() else {
            return;
        };
        if let Some(pid) = i32::try_from(raw_pid)
            .ok()
            .and_then(rustix::process::Pid::from_raw)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }

    pub(super) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for ExactProcessGroupCleanup {
    fn drop(&mut self) {
        self.kill_now();
    }
}
