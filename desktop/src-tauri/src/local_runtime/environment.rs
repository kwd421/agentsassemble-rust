use std::{
    ffi::{OsStr, OsString},
    os::unix::ffi::OsStringExt,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(super) fn configure(command: &mut Command) -> Result<(), String> {
    // Finder does not run /etc/zprofile. Use the OS's path owner directly, with
    // no shell evaluation or inherited PATH_HELPER_ROOT/configuration override.
    let mut helper = Command::new("/usr/libexec/path_helper");
    helper.env_clear().arg("-s");
    let path = configured_path(&mut helper, std::env::var_os("PATH").as_deref())?;
    command.env("PATH", path);
    Ok(())
}

fn configured_path(helper: &mut Command, inherited: Option<&OsStr>) -> Result<OsString, String> {
    let output = helper
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| "cannot read macOS system executable paths")?;
    if !output.status.success() {
        return Err("macOS system executable path helper failed".into());
    }
    let encoded = output
        .stdout
        .strip_prefix(b"PATH=\"")
        .and_then(|value| value.strip_suffix(b"\"; export PATH;\n"))
        .ok_or("macOS system executable path helper returned an invalid record")?;
    // path_helper escapes quotes/apostrophes/dollars in configured file entries.
    // Decode only its data escaping; never run the returned shell assignment.
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' && bytes.peek().is_some_and(|next| b"\"'$".contains(next)) {
            continue;
        }
        decoded.push(byte);
    }
    let system = OsString::from_vec(decoded);
    let mut paths: Vec<PathBuf> = inherited
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    for path in std::env::split_paths(&system) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    std::env::join_paths(paths).map_err(|_| "cannot assemble macOS runtime executable paths".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn gui_child_discovers_configured_tools_and_preserves_explicit_override()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let installed = root.path().join("installed tools");
        let preferred = root.path().join("preferred");
        fs::create_dir_all(root.path().join("etc/paths.d"))?;
        fs::create_dir(&installed)?;
        fs::create_dir(&preferred)?;
        fs::write(root.path().join("etc/paths"), "/usr/bin\n/bin\n")?;
        fs::write(
            root.path().join("etc/paths.d/tools"),
            installed.as_os_str().as_encoded_bytes(),
        )?;
        for path in [
            installed.join("cloudflared"),
            installed.join("node"),
            preferred.join("node"),
        ] {
            fs::write(&path, "#!/bin/sh\nexit 0\n")?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
        let mut helper = Command::new("/usr/libexec/path_helper");
        helper
            .env_clear()
            .arg("-s")
            .env("PATH_HELPER_ROOT", root.path());
        let inherited = std::env::join_paths([
            preferred.as_path(),
            std::path::Path::new("/usr/bin"),
            std::path::Path::new("/bin"),
        ])?;
        let path = configured_path(&mut helper, Some(&inherited))?;
        let found = Command::new("/usr/bin/which")
            .env_clear()
            .env("PATH", &path)
            .args(["cloudflared", "node"])
            .output()?;
        assert!(found.status.success());
        assert_eq!(
            String::from_utf8(found.stdout)?,
            format!(
                "{}\n{}\n",
                installed.join("cloudflared").display(),
                preferred.join("node").display()
            )
        );
        assert_eq!(
            std::env::split_paths(&path)
                .filter(|entry| entry == std::path::Path::new("/usr/bin"))
                .count(),
            1
        );
        Ok(())
    }

    #[test]
    fn configured_paths_are_literal_data_including_shell_metacharacters_and_non_utf8()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("etc/paths.d"))?;
        let literal = OsString::from_vec(
            b"/tools/space quote\" apostrophe' dollar$(touch unwanted) back\\slash\xff".to_vec(),
        );
        fs::write(root.path().join("etc/paths"), literal.as_encoded_bytes())?;
        let mut helper = Command::new("/usr/libexec/path_helper");
        helper
            .env_clear()
            .arg("-s")
            .env("PATH_HELPER_ROOT", root.path());
        let path = configured_path(&mut helper, None)?;
        assert_eq!(path, literal);
        Ok(())
    }

    #[test]
    fn failed_or_unexpected_helper_output_is_a_launch_error() {
        assert!(configured_path(&mut Command::new("/usr/bin/false"), None).is_err());
        assert!(configured_path(&mut Command::new("/usr/bin/true"), None).is_err());
    }
}
