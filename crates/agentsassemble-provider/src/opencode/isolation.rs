use std::path::Path;

use super::{SERVER_USERNAME, config_error};
use crate::launch_error::DriverLaunchError;

/// Builds the child environment that keeps OpenCode's discovery, state and
/// watchers inside this launch.
pub(super) fn isolated_environment(
    root: &Path,
    server_password: &str,
) -> Result<Vec<(String, String)>, DriverLaunchError> {
    // OpenCode 2 discovers `~/.agents` and `~/.claude` skills and the machine config
    // through the OS home, not only XDG_CONFIG_HOME. A private HOME and private
    // config/state/cache roots keep skill and config discovery inside the launch
    // that owns them. File watching is disabled because config discovery otherwise
    // watches the parent of the machine config even when no file exists.
    let path = |name: &str| -> Result<String, DriverLaunchError> {
        let directory = root.join(name);
        std::fs::create_dir_all(&directory).map_err(|_| DriverLaunchError::safe(config_error()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| DriverLaunchError::safe(config_error()))?;
        }
        directory
            .to_str()
            .filter(|value| !value.is_empty() && value.len() <= 4096)
            .map(str::to_owned)
            .ok_or_else(|| DriverLaunchError::safe(config_error()))
    };
    // The driver writes `opencode.json` and `room-instructions.md` into the config
    // root, so both OpenCode config switches must name that same directory.
    let config = root
        .to_str()
        .filter(|value| !value.is_empty() && value.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| DriverLaunchError::safe(config_error()))?;
    let mut environment = vec![
        ("HOME".to_owned(), path("home")?),
        ("TMPDIR".to_owned(), path("tmp")?),
        ("XDG_CONFIG_HOME".to_owned(), config.clone()),
        ("XDG_STATE_HOME".to_owned(), path("state")?),
        ("XDG_CACHE_HOME".to_owned(), path("cache")?),
        ("OPENCODE_CONFIG_DIR".to_owned(), config),
        (
            "OPENCODE_CONFIG_CONTENT".to_owned(),
            "{\"plugin\":[],\"mcp\":{}}".to_owned(),
        ),
        ("OPENCODE_DISABLE_PROJECT_CONFIG".to_owned(), "1".to_owned()),
        (
            "OPENCODE_DISABLE_DEFAULT_PLUGINS".to_owned(),
            "1".to_owned(),
        ),
        (
            "OPENCODE_DISABLE_EXTERNAL_SKILLS".to_owned(),
            "1".to_owned(),
        ),
        (
            "OPENCODE_DISABLE_CLAUDE_CODE_SKILLS".to_owned(),
            "1".to_owned(),
        ),
        ("OPENCODE_DISABLE_FILEWATCHER".to_owned(), "1".to_owned()),
        ("OPENCODE_DISABLE_FFF".to_owned(), "1".to_owned()),
        (
            "OPENCODE_SERVER_USERNAME".to_owned(),
            SERVER_USERNAME.to_owned(),
        ),
        (
            "OPENCODE_SERVER_PASSWORD".to_owned(),
            server_password.to_owned(),
        ),
    ];
    // On Windows Node resolves the home from USERPROFILE and app data from
    // APPDATA/LOCALAPPDATA, and temp files from TMP/TEMP; all of them would
    // otherwise come from the owner's real profile through the inherited allowlist.
    #[cfg(windows)]
    {
        let temp = path("tmp")?;
        environment.push(("USERPROFILE".to_owned(), path("home")?));
        environment.push(("APPDATA".to_owned(), path("appdata")?));
        environment.push(("LOCALAPPDATA".to_owned(), path("localappdata")?));
        environment.push(("TMP".to_owned(), temp.clone()));
        environment.push(("TEMP".to_owned(), temp));
    }
    // OpenCode 2 keeps interactive credentials, sessions, snapshots and its log in
    // the data root. Keep the owner's existing data authority so an OpenCode login
    // and its diagnostics survive; config, skills and watchers never read it.
    if let Some(data) = inherited_data_root() {
        environment.push(("XDG_DATA_HOME".to_owned(), data));
    }
    Ok(environment)
}

fn inherited_data_root() -> Option<String> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .filter(|value| !value.is_empty())
                .map(|home| std::path::PathBuf::from(home).join(".local/share"))
        })?;
    if !data.is_absolute() {
        return None;
    }
    data.to_str()
        .filter(|value| value.len() <= 4096)
        .map(str::to_owned)
}
