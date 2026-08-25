use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::oneshot,
    task::JoinHandle,
};
use uuid::Uuid;

use crate::{opencode_protocol::spawn_error, runtime::DriverError};

const MAX_STARTUP_LINE_BYTES: usize = 1024;

pub(super) async fn reserve_loopback_port() -> Result<u16, DriverError> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| spawn_error())?;
    let port = listener.local_addr().map_err(|_| spawn_error())?.port();
    drop(listener);
    Ok(port)
}

pub(super) fn server_password() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(super) fn observe_startup<R>(
    mut output: R,
    expected_line: String,
) -> (JoinHandle<()>, oneshot::Receiver<bool>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let (ready_sender, ready_receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let ready = read_startup_line(&mut output)
            .await
            .is_some_and(|line| line == expected_line.as_bytes());
        let _ = ready_sender.send(ready);
        drain_output(output).await;
    });
    (task, ready_receiver)
}

async fn read_startup_line<R: AsyncRead + Unpin>(output: &mut R) -> Option<Vec<u8>> {
    let mut line = Vec::with_capacity(128);
    let mut byte = [0_u8; 1];
    loop {
        if line.len() >= MAX_STARTUP_LINE_BYTES {
            return None;
        }
        match output.read(&mut byte).await {
            Ok(0) | Err(_) => return None,
            Ok(_) if byte[0] == b'\n' => {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Some(line);
            }
            Ok(_) => line.push(byte[0]),
        }
    }
}

pub(super) async fn drain_output<R: AsyncRead + Unpin>(mut output: R) {
    let mut buffer = [0_u8; 8 * 1024];
    while output.read(&mut buffer).await.is_ok_and(|count| count != 0) {}
}
