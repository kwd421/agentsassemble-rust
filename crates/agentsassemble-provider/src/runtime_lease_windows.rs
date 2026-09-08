//! The actual Windows Job and its exclusive durable-generation lease share one owner.
use std::{fs::File, io, sync::Mutex};

pub(crate) struct WindowsRuntimeCustody {
    file: Mutex<File>,
    token: String,
    group: processkit::ProcessGroup,
}

impl WindowsRuntimeCustody {
    pub(super) fn new(file: File, token: String) -> io::Result<Self> {
        let group = processkit::ProcessGroup::new()
            .map_err(|_| io::Error::other("provider runtime job could not be created"))?;
        Ok(Self {
            file: Mutex::new(file),
            token,
            group,
        })
    }

    pub(super) fn begin_launch_effect(&self) -> io::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("runtime lease lock failed"))?;
        if super::read_marker(&mut file)? != format!("windows-pending:{}", self.token) {
            return Err(io::Error::other("provider launch lease generation changed"));
        }
        super::write_marker(&mut file, &format!("windows-active:{}", self.token))
    }

    pub(crate) fn group(&self) -> &processkit::ProcessGroup {
        &self.group
    }

    pub(crate) fn is_gone(&self) -> bool {
        self.confirm_cleanup().is_ok()
    }

    fn confirm_cleanup(&self) -> io::Result<()> {
        // Called only after the serialized launch owner ends. The kernel's empty Job
        // proves no remaining member can spawn descendants; leader exit alone cannot.
        if !self
            .group
            .members()
            .map_err(|_| io::Error::other("runtime job query failed"))?
            .is_empty()
        {
            return Err(io::Error::other("runtime job still has members"));
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("runtime lease lock failed"))?;
        let marker = super::read_marker(&mut file)?;
        if marker == format!("gone:{}", self.token) {
            return Ok(());
        }
        if marker != format!("windows-active:{}", self.token) {
            return Err(io::Error::other("runtime cleanup generation changed"));
        }
        super::write_marker(&mut file, &format!("gone:{}", self.token))
    }
}
