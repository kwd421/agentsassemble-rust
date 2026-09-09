//! Unix control input has no blocking worker or read-ahead to survive an executable handoff.
use std::{
    fs::File,
    io::{self, IsTerminal, Read},
    os::{fd::AsFd, unix::fs::FileTypeExt},
    pin::Pin,
    task::{Context, Poll},
};

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use tokio::io::{AsyncRead, ReadBuf, unix::AsyncFd};

pub(crate) struct ControlInput {
    input: Input,
    original_flags: OFlags,
}

enum Input {
    Readiness(AsyncFd<File>),
    // Ordinary file redirection and non-terminal character input (including /dev/null).
    // Reads remain bounded by the control frame; nonblocking devices may return an I/O error.
    Immediate(File),
}

impl ControlInput {
    pub(crate) fn stdin() -> io::Result<Self> {
        Self::from_file(File::from(io::stdin().as_fd().try_clone_to_owned()?))
    }

    fn from_file(file: File) -> io::Result<Self> {
        let kind = file.metadata()?.file_type();
        let readiness = kind.is_fifo() || kind.is_socket() || file.is_terminal();
        if !readiness && !kind.is_file() && !kind.is_char_device() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "unsupported control input",
            ));
        }
        let original_flags = fcntl_getfl(&file)?;
        fcntl_setfl(&file, original_flags | OFlags::NONBLOCK)?;
        let input = if readiness {
            match AsyncFd::try_new(file) {
                Ok(input) => Input::Readiness(input),
                Err(error) => {
                    let (file, failure) = error.into_parts();
                    fcntl_setfl(&file, original_flags)?;
                    return Err(failure);
                }
            }
        } else {
            Input::Immediate(file)
        };
        Ok(Self {
            input,
            original_flags,
        })
    }

    pub(crate) fn restore(&self) -> io::Result<()> {
        fcntl_setfl(self.file(), self.original_flags)?;
        Ok(())
    }

    fn file(&self) -> &File {
        match &self.input {
            Input::Readiness(input) => input.get_ref(),
            Input::Immediate(file) => file,
        }
    }
}

impl AsyncRead for ControlInput {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        match &self.input {
            Input::Immediate(file) => {
                let mut file = file;
                let count = file.read(buffer.initialize_unfilled())?;
                buffer.advance(count);
                Poll::Ready(Ok(()))
            }
            Input::Readiness(input) => loop {
                let mut ready = std::task::ready!(input.poll_read_ready(cx))?;
                if let Ok(result) =
                    ready.try_io(|input| input.get_ref().read(buffer.initialize_unfilled()))
                {
                    buffer.advance(result?);
                    return Poll::Ready(Ok(()));
                }
            },
        }
    }
}

impl Drop for ControlInput {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            tracing::error!(%error, "restore control input descriptor flags");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn cancellation_does_not_read_ahead_or_leave_descriptor_flags_changed() -> io::Result<()>
    {
        let (reader, mut writer) = UnixStream::pair()?;
        let observer = File::from(reader.as_fd().try_clone_to_owned()?);
        let flags = fcntl_getfl(&observer)?;
        let mut input = ControlInput::from_file(File::from(std::os::fd::OwnedFd::from(reader)))?;
        let mut byte = [0];
        {
            let read = input.read(&mut byte);
            tokio::pin!(read);
            assert!(futures_util::poll!(&mut read).is_pending());
        }
        std::io::Write::write_all(&mut writer, b"ab")?;
        assert_eq!(input.read(&mut byte).await?, 1);
        assert_eq!(byte, [b'a']);
        input.restore()?;
        drop(input);
        assert_eq!(fcntl_getfl(&observer)?, flags);
        let mut observer = observer;
        assert_eq!(observer.read(&mut byte)?, 1);
        assert_eq!(byte, [b'b']);
        Ok(())
    }
}
