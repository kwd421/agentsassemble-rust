pub(crate) const HOME_ENV: &str = "CODEX_HOME";

use std::{
    env,
    ffi::OsString,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    room_portal::{RoomPortal, RoomPortalError},
    runtime::DriverError,
};

const CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_MCP_SERVERS: usize = 128;
const MAX_MCP_SERVER_NAME_BYTES: usize = 256;

pub(super) struct CodexConfiguration {
    pub(super) home: String,
    pub(super) inherited_mcp_servers: Vec<String>,
}

pub(crate) fn home() -> Result<String, DriverError> {
    #[cfg(windows)]
    let default_home = env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let default_home = env::var_os("HOME");
    resolve_home(env::var_os(HOME_ENV), default_home)
}

pub(crate) fn login_environment() -> Result<Vec<(String, String)>, DriverError> {
    Ok(vec![(HOME_ENV.to_owned(), home()?)])
}

pub(super) async fn load() -> Result<CodexConfiguration, DriverError> {
    let home = home()?;
    let inherited_mcp_servers = inherited_mcp_servers(Path::new(&home)).await?;
    Ok(CodexConfiguration {
        home,
        inherited_mcp_servers,
    })
}

async fn inherited_mcp_servers(home: &Path) -> Result<Vec<String>, DriverError> {
    let path = home.join("config.toml");
    let read = async {
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(config_error()),
        };
        if !metadata.is_file() || metadata.len() > MAX_CONFIG_BYTES {
            return Err(config_error());
        }
        let bytes = tokio::fs::read(path).await.map_err(|_| config_error())?;
        parse_mcp_server_names(&bytes)
    };
    tokio::time::timeout(CONFIG_READ_TIMEOUT, read)
        .await
        .map_err(|_| config_error())?
}

pub(super) fn append_mcp_isolation(
    arguments: &mut Vec<String>,
    inherited_mcp_servers: &[String],
) -> Result<(), DriverError> {
    if inherited_mcp_servers
        .iter()
        .any(|name| name == "agentsassemble_room")
    {
        return Err(config_error());
    }
    let entries = inherited_mcp_servers
        .iter()
        .map(|name| {
            serde_json::to_string(name)
                .map(|name| format!("{name} = {{ enabled = false }}"))
                .map_err(|_| config_error())
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    arguments.push("-c".to_owned());
    arguments.push(format!("mcp_servers={{ {entries} }}"));
    Ok(())
}

pub(super) fn append_room_portal(
    arguments: &mut Vec<String>,
    room_portal: &RoomPortal,
) -> Result<(), RoomPortalError> {
    if !room_portal.is_running() {
        return Err(RoomPortalError::Mcp);
    }
    let server = "mcp_servers.agentsassemble_room";
    let endpoint =
        serde_json::to_string(room_portal.endpoint()).map_err(|_| RoomPortalError::Authority)?;
    let bearer_environment_name = serde_json::to_string(room_portal.bearer_environment_name())
        .map_err(|_| RoomPortalError::Authority)?;
    let approval = serde_json::to_string("approve").map_err(|_| RoomPortalError::Authority)?;
    super::push_raw_config(arguments, &format!("{server}.url"), &endpoint);
    super::push_raw_config(
        arguments,
        &format!("{server}.bearer_token_env_var"),
        &bearer_environment_name,
    );
    super::push_raw_config(
        arguments,
        &format!("{server}.default_tools_approval_mode"),
        &approval,
    );
    super::push_raw_config(
        arguments,
        "shell_environment_policy.ignore_default_excludes",
        "false",
    );
    super::push_raw_config(arguments, "features.plugins", "false");
    super::push_raw_config(arguments, "features.apps", "false");
    super::push_raw_config(arguments, "features.shell_snapshot", "false");
    super::push_raw_config(arguments, &format!("{server}.startup_timeout_sec"), "10");
    super::push_raw_config(arguments, &format!("{server}.tool_timeout_sec"), "30");
    Ok(())
}

fn resolve_home(
    custom_home: Option<OsString>,
    default_home: Option<OsString>,
) -> Result<String, DriverError> {
    let home = match custom_home {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        Some(_) => return Err(config_error()),
        None => default_home
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".codex"))
            .ok_or_else(config_error)?,
    };
    let absolute = std::path::absolute(home).map_err(|_| config_error())?;
    let home = absolute.to_str().ok_or_else(config_error)?;
    if home.len() > 4096 || home.contains('\0') {
        return Err(config_error());
    }
    Ok(home.to_owned())
}

fn parse_mcp_server_names(bytes: &[u8]) -> Result<Vec<String>, DriverError> {
    let text = std::str::from_utf8(bytes).map_err(|_| config_error())?;
    let document = toml::from_str::<toml::Value>(text).map_err(|_| config_error())?;
    let Some(servers) = document.get("mcp_servers").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    if servers.len() > MAX_MCP_SERVERS {
        return Err(config_error());
    }
    let mut names = servers.keys().cloned().collect::<Vec<_>>();
    if names.iter().any(|name| {
        name.is_empty()
            || name.len() > MAX_MCP_SERVER_NAME_BYTES
            || name.chars().any(char::is_control)
    }) {
        return Err(config_error());
    }
    names.sort();
    Ok(names)
}

const fn config_error() -> DriverError {
    DriverError::new(
        "provider_config_unavailable",
        "The Codex process-local tool configuration could not be isolated.",
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn unrepresentable_or_missing_home_is_rejected() {
        assert!(super::resolve_home(None, None).is_err());
        assert!(super::resolve_home(Some("".into()), Some("/home".into())).is_err());
        assert!(super::resolve_home(Some("bad\0home".into()), None).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            assert!(
                super::resolve_home(Some(std::ffi::OsString::from_vec(vec![0xff])), None).is_err()
            );
        }
    }

    #[test]
    fn inherited_mcp_names_are_parsed_without_reading_values() {
        let names = super::parse_mcp_server_names(
            br#"
                [mcp_servers.node_repl]
                command = "private-command"

                [mcp_servers."company.tools"]
                url = "https://private.example.test/mcp"
            "#,
        )
        .unwrap_or_else(|error| panic!("parse inherited MCP names: {error}"));
        assert_eq!(names, ["company.tools", "node_repl"]);
    }

    #[test]
    fn inherited_mcp_table_is_disabled_process_locally() {
        let mut arguments = Vec::new();
        super::append_mcp_isolation(
            &mut arguments,
            &["company.tools".to_owned(), "node_repl".to_owned()],
        )
        .unwrap_or_else(|error| panic!("append MCP isolation: {error}"));
        assert_eq!(
            arguments,
            [
                "-c",
                "mcp_servers={ \"company.tools\" = { enabled = false }, \"node_repl\" = { enabled = false } }"
            ]
        );
    }

    #[test]
    fn reserved_room_server_in_user_config_fails_closed() {
        let mut arguments = Vec::new();
        assert!(
            super::append_mcp_isolation(&mut arguments, &["agentsassemble_room".to_owned()])
                .is_err()
        );
    }
}
