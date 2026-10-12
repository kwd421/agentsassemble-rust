//! Pull one authorized storage piece; transport owners enforce unpolled stalls.
use crate::{
    download_budget::Lease,
    http_admission::{DownloadHttpWait, HttpConnectionAdmission},
    http_body_policy::Progress,
};
use agentsassemble_persistence::MessageAttachmentReader;
use axum::body::{Body, Bytes};
use std::sync::Arc;

struct Hold {
    _lease: Lease,
    _connection: Option<DownloadHttpWait>,
}
struct Piece {
    bytes: Vec<u8>,
    _hold: Arc<Hold>,
}
impl AsRef<[u8]> for Piece {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

pub(crate) fn body(
    reader: MessageAttachmentReader,
    lease: Lease,
    connection: Option<&HttpConnectionAdmission>,
) -> (Body, Progress) {
    let progress = Progress::new();
    let hold = Arc::new(Hold {
        _lease: lease,
        _connection: connection.map(|connection| connection.retain_download(progress.clone())),
    });
    let stream_progress = progress.clone();
    let stream = futures_util::stream::try_unfold(
        (reader, hold, 0, 0),
        move |(reader, hold, sequence, received)| {
            let progress = stream_progress.clone();
            async move {
                if received == reader.metadata().size {
                    return Ok(None);
                }
                progress.check().map_err(std::io::Error::other)?;
                let bytes = reader
                    .read_piece(sequence)
                    .await
                    .map_err(std::io::Error::other)?;
                let received = received + bytes.len();
                progress.accept(bytes.len());
                let bytes = Bytes::from_owner(Piece {
                    bytes,
                    _hold: hold.clone(),
                });
                Ok::<_, std::io::Error>(Some((bytes, (reader, hold, sequence + 1, received))))
            }
        },
    );
    (Body::from_stream(stream), progress)
}

#[cfg(test)]
pub(crate) mod tests;
