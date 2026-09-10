//! Unix control input has no blocking reader or read-ahead to survive executable handoff.
use std::{
    fs::File,
    io::{self, IsTerminal, Read},
    os::{
        fd::{AsFd, AsRawFd},
        unix::fs::FileTypeExt,
    },
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

    /// Observe pipe loss while recovery owns startup, without consuming control bytes.
    pub(crate) async fn during_recovery(
        &self,
        cancellation: &tokio_util::sync::CancellationToken,
        recovery: impl std::future::Future<Output = io::Result<()>>,
    ) -> io::Result<()> {
        let kind = self.file().metadata()?.file_type();
        if !kind.is_fifo() && !kind.is_socket() {
            // Redirected files and terminals have no supervised parent-pipe lifetime.
            return recovery.await;
        }
        let parent = self.file().try_clone()?;
        let mut poll = mio::Poll::new()?;
        poll.registry().register(
            &mut mio::unix::SourceFd(&parent.as_raw_fd()),
            mio::Token(0),
            mio::Interest::READABLE,
        )?;
        let finished = mio::Waker::new(poll.registry(), mio::Token(1))?;
        let signal = cancellation.clone();
        let observer = tokio::task::spawn_blocking(move || {
            // Keep the separately registered descriptor alive. Ordinary readable
            // events leave bytes for the sole control reader; EOF is a distinct
            // edge even when requests were queued before replacement startup.
            let _parent = parent;
            let mut events = mio::Events::with_capacity(2);
            loop {
                match poll.poll(&mut events, None) {
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        signal.cancel();
                        return Err(error);
                    }
                    Ok(()) => {
                        if events.iter().any(|event| {
                            event.token() == mio::Token(0)
                                && (event.is_read_closed() || event.is_error())
                        }) {
                            signal.cancel();
                            tracing::debug!("parent control ended during runtime recovery");
                            return Ok(());
                        }
                        if events.iter().any(|event| event.token() == mio::Token(1)) {
                            return Ok(());
                        }
                    }
                }
            }
        });
        let result = recovery.await;
        // The startup owner wakes and joins its event wait before
        // handing stdin to its sole reader or restoring descriptors for another image.
        let wake_result = finished.wake();
        observer.await.map_err(io::Error::other)??;
        wake_result?;
        result
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
    async fn recovery_observes_pipe_loss_without_consuming_queued_control_bytes() -> io::Result<()>
    {
        use tokio_util::sync::CancellationToken;
        let (reader, writer) = rustix::pipe::pipe()?;
        let mut writer = File::from(writer);
        let cancellation = CancellationToken::new();
        std::io::Write::write_all(&mut writer, b"queued\n")?;
        let mut input = ControlInput::from_file(File::from(reader))?;
        let (entered, entry) = tokio::sync::oneshot::channel();
        let close = tokio::spawn(async move {
            entry.await.map_err(io::Error::other)?;
            drop(writer);
            Ok::<_, io::Error>(())
        });
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            input.during_recovery(&cancellation, async {
                entered
                    .send(())
                    .map_err(|()| io::Error::other("entry receiver closed"))?;
                cancellation.cancelled().await;
                Ok(())
            }),
        )
        .await??;
        close.await.map_err(io::Error::other)??;
        let mut remaining = Vec::new();
        input.read_to_end(&mut remaining).await?;
        assert_eq!(remaining, b"queued\n");
        Ok(())
    }

    #[tokio::test]
    async fn completed_recovery_joins_observer_and_preserves_live_input() -> io::Result<()> {
        let (reader, mut writer) = UnixStream::pair()?;
        let mut input = ControlInput::from_file(File::from(std::os::fd::OwnedFd::from(reader)))?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        std::io::Write::write_all(&mut writer, b"next\n")?;
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            input.during_recovery(&cancellation, async { Ok(()) }),
        )
        .await??;
        assert!(!cancellation.is_cancelled());
        let mut remaining = [0; 5];
        input.read_exact(&mut remaining).await?;
        assert_eq!(&remaining, b"next\n");
        Ok(())
    }

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
