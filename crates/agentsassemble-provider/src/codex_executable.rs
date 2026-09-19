use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Seek},
    path::Path,
};

use agentsassemble_domain::{
    codex_bundle_identity, codex_code_mode_host_name, stable_content_identity,
};
use same_file::Handle;
#[cfg(unix)]
use std::{os::unix::fs::PermissionsExt, path::PathBuf};

use super::{BoundExecutable, FilesystemFailure};

#[cfg(unix)]
pub(crate) fn codex_code_mode_host_path(
    executable: &BoundExecutable,
) -> io::Result<Option<PathBuf>> {
    if executable.companion_files.is_empty() {
        #[cfg(test)]
        return Ok(None);
        #[cfg(not(test))]
        return Err(io::Error::other(
            "Codex code-mode host authority is unavailable",
        ));
    }
    if executable.companion_files.len() != 1 {
        return Err(io::Error::other(
            "Codex code-mode host authority is ambiguous",
        ));
    }
    let path = Path::new(executable.launch_path())
        .parent()
        .ok_or_else(|| io::Error::other("Codex bundle directory is unavailable"))?
        .join(codex_code_mode_host_name());
    let staged = Handle::from_path(&path)?;
    let held = Handle::from_file(executable.companion_files[0].try_clone()?)?;
    if staged != held {
        return Err(io::Error::other(
            "Codex code-mode host authority changed after binding",
        ));
    }
    Ok(Some(path))
}

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
    // PATHEXT lookup selects npm's `codex.cmd` wrapper on Windows. It is not a native
    // bundle and it does not sit beside the package, so follow it to the package script.
    #[cfg(windows)]
    if entry
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("cmd"))
    {
        return match npm_cmd_shim_script(entry)? {
            Some(script) => resolve_script_entry(&script),
            None => Ok(None),
        };
    }
    let mut prefix = [0_u8; 2];
    let script = File::open(entry)?.read_exact(&mut prefix).is_ok() && prefix == *b"#!";
    if !script {
        return codex_authority(entry).map(Some);
    }
    resolve_script_entry(entry)
}

/// Resolves the package script an npm `cmd-shim` wrapper launches.
///
/// npm writes the target as `"%dp0%\<relative path>.js"`, relative to the wrapper's own
/// directory. Only a relative path made of plain segments is followed.
#[cfg(windows)]
pub(crate) fn npm_cmd_shim_script(shim: &Path) -> io::Result<Option<std::path::PathBuf>> {
    npm_cmd_shim_file(shim, ".js")
}

/// Resolves the native executable an npm `cmd-shim` wrapper launches directly.
///
/// Packages that ship a platform binary as their `bin` (Claude Code does) get a wrapper that
/// runs `"%dp0%\<relative path>.exe"`. Node refuses to spawn `.cmd` files without a shell, so
/// callers that hand the launcher to a Node child must use this target instead.
#[cfg(windows)]
pub(crate) fn npm_cmd_shim_native(shim: &Path) -> io::Result<Option<std::path::PathBuf>> {
    npm_cmd_shim_file(shim, ".exe")
}

#[cfg(windows)]
fn npm_cmd_shim_file(shim: &Path, extension: &str) -> io::Result<Option<std::path::PathBuf>> {
    const MAX_SHIM_BYTES: u64 = 16 * 1024;
    let mut text = String::new();
    File::open(shim)?
        .take(MAX_SHIM_BYTES)
        .read_to_string(&mut text)?;
    let Some(directory) = shim.parent() else {
        return Ok(None);
    };
    let Some(target) = npm_cmd_shim_target(&text, extension, project_local_bin(directory)) else {
        return Ok(None);
    };
    let file = target
        .split(['\\', '/'])
        .fold(directory.to_path_buf(), |path, segment| path.join(segment));
    Ok(file.is_file().then_some(file))
}

/// Whether this wrapper is npm's project-local launcher directory, `node_modules/.bin`.
#[cfg(any(windows, test))]
fn project_local_bin(directory: &Path) -> bool {
    directory.file_name().is_some_and(|name| name == ".bin")
        && directory
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "node_modules")
}

/// Resolves the package file an npm `cmd-shim` names, relative to the wrapper's own directory.
///
/// A global wrapper sits beside its `node_modules`, so its target starts there. A project-local
/// wrapper sits inside `node_modules/.bin`, so its target starts with one `..` that leads back to
/// the same `node_modules`. Nothing else may leave the wrapper's tree: a second `..`, a `.`, an
/// empty segment, or a bare `node.exe` beside the wrapper is refused.
#[cfg(any(windows, test))]
fn npm_cmd_shim_target<'a>(text: &'a str, extension: &str, local_bin: bool) -> Option<&'a str> {
    const DP0: &str = "\"%dp0%\\";
    text.match_indices(DP0)
        .filter_map(|(start, _)| {
            let rest = &text[start + DP0.len()..];
            let target = &rest[..rest.find('"')?];
            let mut segments = target.split(['\\', '/']).peekable();
            let packaged = match segments.peek() {
                Some(&"node_modules") => {
                    segments.next();
                    true
                }
                Some(&"..") if local_bin => {
                    segments.next();
                    true
                }
                _ => false,
            };
            let plain =
                segments.all(|segment| !segment.is_empty() && segment != "." && segment != "..");
            (packaged && plain && target.to_ascii_lowercase().ends_with(extension))
                .then_some(target)
        })
        .last()
}

fn resolve_script_entry(entry: &Path) -> io::Result<Option<(String, String)>> {
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
    Ok(codex_bundle_identity(
        &executable_identity,
        &companion_identity,
    ))
}

fn bind_codex_executable_sync(path: &Path, expected_identity: &str) -> io::Result<BoundExecutable> {
    let (mut executable, executable_handle, mut companion, companion_handle) =
        open_codex_bundle(path)?;
    let executable_identity = stable_content_identity(&executable_handle, &mut executable)?;
    let companion_identity = stable_content_identity(&companion_handle, &mut companion)?;
    if codex_bundle_identity(&executable_identity, &companion_identity) != expected_identity {
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
        .join(codex_code_mode_host_name());
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
    let companion_path = staging.path().join(codex_code_mode_host_name());
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
            &directory.join(super::codex_code_mode_host_name()),
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
            &native_dir.join(super::codex_code_mode_host_name()),
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

    const NPM_CMD_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\r\nIF EXIST \"%dp0%\\node.exe\" (\r\n  SET \"_prog=%dp0%\\node.exe\"\r\n) ELSE (\r\n  SET \"_prog=node\"\r\n  SET PATHEXT=%PATHEXT:;.JS;=;%\r\n)\r\n\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*\r\n";

    #[test]
    fn npm_cmd_shim_target_is_the_package_script_not_node() {
        assert_eq!(
            super::npm_cmd_shim_target(NPM_CMD_SHIM, ".js", false),
            Some("node_modules\\@openai\\codex\\bin\\codex.js")
        );
        for escaping in [
            "\"%dp0%\\..\\evil\\codex.js\"",
            "\"%dp0%\\node_modules\\.\\codex.js\"",
            "\"%dp0%\\node_modules\\\\codex.js\"",
            "\"%dp0%\\node.exe\"",
        ] {
            assert_eq!(
                super::npm_cmd_shim_target(escaping, ".js", false),
                None,
                "{escaping}"
            );
        }
    }

    #[test]
    fn npm_cmd_shim_target_follows_a_project_local_bin_wrapper_one_level_up() {
        const LOCAL_SHIM: &str =
            concat!("@ECHO off", r#""%dp0%\..\@openai\codex\bin\codex.js" %*"#);
        assert_eq!(
            super::npm_cmd_shim_target(LOCAL_SHIM, ".js", true),
            Some(r"..\@openai\codex\bin\codex.js")
        );
        // Only node_modules/.bin gets that allowance, and only for one level.
        assert_eq!(super::npm_cmd_shim_target(LOCAL_SHIM, ".js", false), None);
        for escaping in [r#""%dp0%\..\..\evil\codex.js""#, r#""%dp0%\..\.\codex.js""#] {
            assert_eq!(
                super::npm_cmd_shim_target(escaping, ".js", true),
                None,
                "{escaping}"
            );
        }
    }

    #[test]
    fn project_local_bin_is_only_npms_own_launcher_directory() {
        use std::path::Path;

        assert!(super::project_local_bin(Path::new(
            r"C:\app\node_modules\.bin"
        )));
        assert!(!super::project_local_bin(Path::new(r"C:\app\.bin")));
        assert!(!super::project_local_bin(Path::new(r"C:\app\node_modules")));
    }

    #[test]
    fn npm_cmd_shim_target_follows_a_native_package_binary() {
        const NATIVE_SHIM: &str = "@ECHO off\r\nGOTO start\r\n:find_dp0\r\nSET dp0=%~dp0\r\nEXIT /b\r\n:start\r\nSETLOCAL\r\nCALL :find_dp0\r\n\"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe\"   %*\r\n";
        assert_eq!(
            super::npm_cmd_shim_target(NATIVE_SHIM, ".exe", false),
            Some("node_modules\\@anthropic-ai\\claude-code\\bin\\claude.exe")
        );
        assert_eq!(super::npm_cmd_shim_target(NATIVE_SHIM, ".js", false), None);
        assert_eq!(
            super::npm_cmd_shim_target(NPM_CMD_SHIM, ".exe", false),
            None
        );
        assert_eq!(
            super::npm_cmd_shim_target("\"%dp0%\\..\\evil\\claude.exe\"", ".exe", false),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn npm_cmd_shim_resolves_platform_native_bundle() {
        let Some((platform_package, target, binary)) = super::codex_native_layout() else {
            return;
        };
        let prefix =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create npm prefix: {error}"));
        let shim = prefix.path().join("codex.cmd");
        std::fs::write(&shim, NPM_CMD_SHIM).unwrap_or_else(|error| panic!("write shim: {error}"));
        let package = prefix.path().join("node_modules/@openai/codex");
        make_executable(&package.join("bin/codex.js"), b"#!/usr/bin/env node\n");
        let native_dir = package
            .join("node_modules/@openai")
            .join(platform_package)
            .join("vendor")
            .join(target)
            .join("bin");
        let native = native_dir.join(binary);
        make_executable(&native, b"native-codex");
        make_executable(
            &native_dir.join(super::codex_code_mode_host_name()),
            b"native-code-mode-host",
        );
        let resolved = super::resolve_codex_entry(&shim)
            .unwrap_or_else(|error| panic!("resolve npm Codex wrapper: {error}"))
            .unwrap_or_else(|| panic!("npm Codex wrapper was not resolved"));
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
            &root.path().join(super::codex_code_mode_host_name()),
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
            .join(super::codex_code_mode_host_name());
        assert_eq!(
            std::fs::read(companion)
                .unwrap_or_else(|error| panic!("read staged companion: {error}")),
            b"native-code-mode-host"
        );
        assert!(!bound.requires_inherited_executable_fd());
        assert!(bound.allows_child_processes());
    }
}
