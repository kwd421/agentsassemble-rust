use std::{env, io, path::PathBuf, time::Duration};

use crate::runtime::DriverError;

const CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_MCP_SERVERS: usize = 128;
const MAX_MCP_SERVER_NAME_BYTES: usize = 256;

pub(super) async fn inherited_mcp_servers() -> Result<Vec<String>, DriverError> {
    let Some(path) = config_path() else {
        return Ok(Vec::new());
    };
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

fn config_path() -> Option<PathBuf> {
    env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
        .map(|home| home.join("config.toml"))
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
