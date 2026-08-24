use std::{fs, path::PathBuf};

use agentsassemble_protocol::{
    CommandAck, HostProductSurface, HttpMethod, HttpRouteSurface, RoomAction, RoomSnapshot,
    RoomStream, ServerProductSurface, Subscribed, TicketResponse,
};
use ts_rs::{Config, TS};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/types/generated");
    fs::create_dir_all(&output)?;
    let config = Config::new()
        .with_out_dir(output)
        .with_large_int("number")
        .with_import_extension(Some("js"));
    RoomSnapshot::export_all(&config)?;
    Subscribed::export_all(&config)?;
    CommandAck::export_all(&config)?;
    TicketResponse::export_all(&config)?;
    HttpMethod::export_all(&config)?;
    HttpRouteSurface::export_all(&config)?;
    RoomStream::export_all(&config)?;
    RoomAction::export_all(&config)?;
    ServerProductSurface::export_all(&config)?;
    HostProductSurface::export_all(&config)?;
    Ok(())
}
