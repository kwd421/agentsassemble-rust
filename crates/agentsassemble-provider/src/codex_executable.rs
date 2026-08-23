use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Seek},
    path::Path,
};

use agentsassemble_domain::{stable_bundle_identity, stable_content_identity};
use same_file::Handle;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use super::{BoundExecutable, FilesystemFailure};

const CODEX_COMPANION: &str = "codex-code-mode-host";

pub(crate) async fn resolve_codex_executable() -> Result<Option<(String, String)>, FilesystemFailure>
{
    super::run_bounded(resolve_codex_executable_sync).await
}

pub(crate) async fn codex_executable_identity(path: String) -> Result<String, FilesystemFailure> {
    super::run_bounded(move || {
        let path = Path::new(&path);
        #[cfg(test)]
        return codex_executable_identity_sync(path)
            .or_else(|_| super::executable_identity_sync(path));
        #[cfg(not(test))]
        codex_executable_identity_sync(path)
    })
    .await
}

pub(crate) async fn bind_codex_executable(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    super::run_bounded(move || {
        let path = Path::new(&path);
        #[cfg(test)]
        if !expected_identity.starts_with("bundle-identity-v1-") {
            return super::bind_executable_sync(path, &expected_identity);
        }
        bind_codex_executable_sync(path, &expected_identity)
    })
    .await
}

fn resolve_codex_executable_sync() -> io::Result<Option<(String, String)>> {
    let Some((entry, _)) = super::resolve_executable_sync("codex")? else {
        return Ok(None);
    };
    resolve_codex_entry(Path::new(&entry))
}

fn resolve_codex_entry(entry: &Path) -> io::Result<Option<(String, String)>> {
    let mut prefix = [0_u8; 2];
    let script = File::open(entry)?.read_exact(&mut prefix).is_ok() && prefix == *b"#!";
    if !script {
        return codex_authority(entry).map(Some);
    }
    let Some(package_root) = entry.parent().and_then(Path::parent) else {
        return Ok(None);
    };
    let Some((platform_package, target, binary)) = codex_native_layout() else {
        return Ok(None);
    };
    let candidates = [
        package_root
            .join("node_modules")
            .join("@openai")
            .join(platform_package)
            .join("vendor")
            .join(target)
            .join("bin")
            .join(binary),
        package_root
            .join("vendor")
            .join(target)
            .join("bin")
            .join(binary),
    ];
    for candidate in candidates {
        if super::is_executable_file(&candidate)? {
            return codex_authority(&candidate).map(Some);
        }
    }
    Ok(None)
}

fn codex_authority(path: &Path) -> io::Result<(String, String)> {
    let canonical = path.canonicalize()?;
    let identity = codex_executable_identity_sync(&canonical)?;
    let encoded = canonical
        .to_str()
        .ok_or_else(|| io::Error::other("Codex executable path is not UTF-8"))?;
    Ok((encoded.to_owned(), identity))
}

fn codex_executable_identity_sync(path: &Path) -> io::Result<String> {
    let (mut executable, executable_handle, mut companion, companion_handle) =
        open_codex_bundle(path)?;
    let executable_identity = stable_content_identity(&executable_handle, &mut executable)?;
    let companion_identity = stable_content_identity(&companion_handle, &mut companion)?;
    Ok(bundle_identity(&executable_identity, &companion_identity))
}

fn bind_codex_executable_sync(path: &Path, expected_identity: &str) -> io::Result<BoundExecutable> {
    let (mut executable, executable_handle, mut companion, companion_handle) =
        open_codex_bundle(path)?;
    let executable_identity = stable_content_identity(&executable_handle, &mut executable)?;
    let companion_identity = stable_content_identity(&companion_handle, &mut companion)?;
    if bundle_identity(&executable_identity, &companion_identity) != expected_identity {
        return Err(io::Error::other("Codex executable bundle identity changed"));
    }
    executable.rewind()?;
    companion.rewind()?;
    #[cfg(unix)]
    return stage_codex_bundle(
        executable,
        &executable_handle,
        &executable_identity,
        companion,
        &companion_handle,
        &companion_identity,
    );
    #[cfg(not(unix))]
    {
        let launch_path = path
            .to_str()
            .ok_or_else(|| io::Error::other("Codex executable path is not UTF-8"))?
            .to_owned();
        Ok(BoundExecutable {
            file: executable,
            launch_path,
            companion_files: vec![companion],
            allows_child_processes: true,
        })
    }
}

fn open_codex_bundle(path: &Path) -> io::Result<(File, Handle, File, Handle)> {
    let canonical = path.canonicalize()?;
    if canonical != path || !super::is_executable_file(&canonical)? {
        return Err(io::Error::other(
            "Codex executable authority is not canonical",
        ));
    }
    let companion_path = canonical
        .parent()
        .ok_or_else(|| io::Error::other("Codex executable directory is unavailable"))?
        .join(codex_companion_name());
    let companion_canonical = companion_path.canonicalize()?;
    if companion_canonical != companion_path || !super::is_executable_file(&companion_canonical)? {
        return Err(io::Error::other(
            "Codex code-mode host authority is invalid",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        options.share_mode(FILE_SHARE_READ);
    }
    let executable = options.open(&canonical)?;
    let companion = options.open(&companion_canonical)?;
    let executable_handle = Handle::from_file(executable.try_clone()?)?;
    let companion_handle = Handle::from_file(companion.try_clone()?)?;
    Ok((executable, executable_handle, companion, companion_handle))
}

fn bundle_identity(executable_identity: &str, companion_identity: &str) -> String {
    stable_bundle_identity("codex-native", &[executable_identity, companion_identity])
}

#[cfg(unix)]
fn stage_codex_bundle(
    mut executable: File,
    executable_handle: &Handle,
    executable_identity: &str,
    mut companion: File,
    companion_handle: &Handle,
    companion_identity: &str,
) -> io::Result<BoundExecutable> {
    let (executable, launch_path, staging) =
        super::stage_private_executable(&mut executable, executable_handle, executable_identity)?;
    let companion_path = staging.path().join(codex_companion_name());
    let mut staged_companion = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&companion_path)?;
    super::copy_and_verify_staged(
        &mut companion,
        companion_handle,
        companion_identity,
        &mut staged_companion,
    )?;
    std::fs::set_permissions(&companion_path, std::fs::Permissions::from_mode(0o500))?;
    Ok(BoundExecutable {
        file: executable,
        launch_path,
        companion_files: vec![staged_companion],
        allows_child_processes: true,
        _staging: Some(staging),
        #[cfg(any(target_os = "linux", target_os = "android"))]
        inherited_executable_fd: false,
    })
}

fn codex_companion_name() -> &'static str {
    if cfg!(windows) {
        "codex-code-mode-host.exe"
    } else {
        CODEX_COMPANION
    }
}

fn codex_native_layout() -> Option<(&'static str, &'static str, &'static str)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("macos", "aarch64") => Some(("codex-darwin-arm64", "aarch64-apple-darwin", "codex")),
        ("macos", "x86_64") => Some(("codex-darwin-x64", "x86_64-apple-darwin", "codex")),
        ("linux" | "android", "aarch64") => {
            Some(("codex-linux-arm64", "aarch64-unknown-linux-musl", "codex"))
        }
        ("linux" | "android", "x86_64") => {
            Some(("codex-linux-x64", "x86_64-unknown-linux-musl", "codex"))
        }
        ("windows", "aarch64") => {
            Some(("codex-win32-arm64", "aarch64-pc-windows-msvc", "codex.exe"))
        }
        ("windows", "x86_64") => Some(("codex-win32-x64", "x86_64-pc-windows-msvc", "codex.exe")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn make_executable(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(
            path.parent()
                .unwrap_or_else(|| panic!("fixture parent missing")),
        )
        .unwrap_or_else(|error| panic!("create fixture directory: {error}"));
        std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write fixture: {error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path)
                .unwrap_or_else(|error| panic!("read fixture mode: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(path, permissions)
                .unwrap_or_else(|error| panic!("set fixture mode: {error}"));
        }
    }

    fn make_bundle(directory: &Path) -> PathBuf {
        let executable = directory.join(if cfg!(windows) { "codex.exe" } else { "codex" });
        make_executable(&executable, b"native-codex");
        make_executable(
            &directory.join(super::codex_companion_name()),
            b"native-code-mode-host",
        );
        executable
    }

    #[test]
    fn script_entry_resolves_platform_native_bundle() {
        let Some((platform_package, target, binary)) = super::codex_native_layout() else {
            return;
        };
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create Codex root: {error}"));
        let entry = root.path().join("bin/codex.js");
        make_executable(&entry, b"#!/usr/bin/env node\n");
        let native_dir = root
            .path()
            .join("node_modules/@openai")
            .join(platform_package)
            .join("vendor")
            .join(target)
            .join("bin");
        let native = native_dir.join(binary);
        make_executable(&native, b"native-codex");
        make_executable(
            &native_dir.join(super::codex_companion_name()),
            b"native-code-mode-host",
        );
        let resolved = super::resolve_codex_entry(&entry)
            .unwrap_or_else(|error| panic!("resolve Codex native bundle: {error}"))
            .unwrap_or_else(|| panic!("Codex native bundle was not resolved"));
        assert_eq!(
            resolved.0,
            native.canonicalize().unwrap_or(native).to_string_lossy()
        );
        assert!(resolved.1.starts_with("bundle-identity-v1-"));
    }

    #[test]
    fn bundle_identity_changes_with_companion_bytes() {
        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create bundle root: {error}"));
        let executable = make_bundle(root.path())
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize bundle executable: {error}"));
        let first = super::codex_executable_identity_sync(&executable)
            .unwrap_or_else(|error| panic!("identify bundle: {error}"));
        make_executable(
            &root.path().join(super::codex_companion_name()),
            b"changed-code-mode-host",
        );
        let changed = super::codex_executable_identity_sync(&executable)
            .unwrap_or_else(|error| panic!("reidentify bundle: {error}"));
        assert_ne!(first, changed);
    }

    #[test]
    fn missing_companion_fails_closed() {
        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create bundle root: {error}"));
        let executable = root
            .path()
            .join(if cfg!(windows) { "codex.exe" } else { "codex" });
        make_executable(&executable, b"native-codex");
        let executable = executable
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize bundle executable: {error}"));
        assert!(super::codex_executable_identity_sync(&executable).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bound_bundle_stages_verified_companion_beside_provider() {
        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create bundle root: {error}"));
        let executable = make_bundle(root.path())
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize bundle executable: {error}"));
        let identity = super::super::runtime_executable_identity(
            "codex_live_session",
            executable.to_string_lossy().into_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("identify bundle: {error:?}"));
        let bound =
            super::bind_codex_executable(executable.to_string_lossy().into_owned(), identity)
                .await
                .unwrap_or_else(|error| panic!("bind bundle: {error:?}"));
        let companion = Path::new(bound.launch_path())
            .parent()
            .unwrap_or_else(|| panic!("staged provider parent missing"))
            .join(super::codex_companion_name());
        assert_eq!(
            std::fs::read(companion)
                .unwrap_or_else(|error| panic!("read staged companion: {error}")),
            b"native-code-mode-host"
        );
        assert!(!bound.requires_inherited_executable_fd());
        assert!(bound.allows_child_processes());
    }
}
