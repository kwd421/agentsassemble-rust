use std::{fs::File, path::Path};

use agentsassemble_domain::{stable_bundle_identity, stable_content_identity};
use same_file::Handle;

pub fn write_codex_bundle(root: &Path, fixture: &[u8]) -> (String, String) {
    let executable = root.join(if cfg!(windows) {
        "provider-fixture.exe"
    } else {
        "provider-fixture"
    });
    let companion = root.join(if cfg!(windows) {
        "codex-code-mode-host.exe"
    } else {
        "codex-code-mode-host"
    });
    write_executable(&executable, fixture);
    #[cfg(unix)]
    write_executable(
        &companion,
        b"#!/bin/sh\nprintf '%s\\n' 'ws://127.0.0.1:43123'\nexec /usr/bin/tail -f /dev/null\n",
    );
    #[cfg(not(unix))]
    write_executable(&companion, b"codex companion fixture");
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("resolve test executable: {error}"));
    let main_identity = executable_identity(&executable);
    let companion_identity = executable_identity(&companion);
    let bundle_identity =
        stable_bundle_identity("codex-native", &[&main_identity, &companion_identity]);
    (executable.to_string_lossy().into_owned(), bundle_identity)
}

fn write_executable(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write test executable: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("read test executable mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions)
            .unwrap_or_else(|error| panic!("set test executable mode: {error}"));
    }
}

fn executable_identity(path: &Path) -> String {
    let mut file = File::open(path).unwrap_or_else(|error| panic!("open test executable: {error}"));
    let handle = Handle::from_file(
        file.try_clone()
            .unwrap_or_else(|error| panic!("clone test executable: {error}")),
    )
    .unwrap_or_else(|error| panic!("identify test executable: {error}"));
    stable_content_identity(&handle, &mut file)
        .unwrap_or_else(|error| panic!("hash test executable: {error}"))
}
