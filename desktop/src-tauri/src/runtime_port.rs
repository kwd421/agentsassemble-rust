//! The local server keeps the port it had last time, so external AI tools
//! registered with this computer's MCP address keep working after a restart.
//! A port another program took is not fought over: the server then gets a new
//! one and that becomes the remembered port.
use std::{fs, net::TcpListener, path::Path};

const PORT_FILE: &str = "runtime.port";

/// The `--bind` address for this start.
pub(crate) fn bind_address(data_root: &Path) -> String {
    let remembered = fs::read_to_string(data_root.join(PORT_FILE))
        .ok()
        .and_then(|text| text.trim().parse::<u16>().ok())
        .filter(|port| *port >= 1024);
    match remembered {
        // Free now; the server binds it a moment later.
        Some(port) if TcpListener::bind(("127.0.0.1", port)).is_ok() => format!("127.0.0.1:{port}"),
        _ => "127.0.0.1:0".to_owned(),
    }
}

/// Remembers the port the server actually listens on.
pub(crate) fn remember(data_root: &Path, port: u16) {
    let path = data_root.join(PORT_FILE);
    if fs::read_to_string(&path).is_ok_and(|text| text.trim() == port.to_string()) {
        return;
    }
    // Losing this only costs a new port next time.
    let _ = fs::write(path, port.to_string());
}

/// A start that failed may have lost the port; the next start takes a new one.
pub(crate) fn forget(data_root: &Path) {
    let _ = fs::remove_file(data_root.join(PORT_FILE));
}

#[cfg(test)]
mod tests {
    use super::{bind_address, remember};

    #[test]
    fn keeps_a_free_remembered_port_and_gives_up_a_taken_one() {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("temp dir: {error}"));
        assert_eq!(bind_address(root.path()), "127.0.0.1:0");
        let taken = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap_or_else(|error| panic!("bind: {error}"));
        let port = taken
            .local_addr()
            .unwrap_or_else(|error| panic!("addr: {error}"))
            .port();
        remember(root.path(), port);
        assert_eq!(
            bind_address(root.path()),
            "127.0.0.1:0",
            "another program holds it"
        );
        drop(taken);
        assert_eq!(bind_address(root.path()), format!("127.0.0.1:{port}"));
        std::fs::write(root.path().join("runtime.port"), "80")
            .unwrap_or_else(|error| panic!("write: {error}"));
        assert_eq!(
            bind_address(root.path()),
            "127.0.0.1:0",
            "privileged ports are never used"
        );
    }
}
