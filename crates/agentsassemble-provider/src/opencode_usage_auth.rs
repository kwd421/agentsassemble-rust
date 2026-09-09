use std::{env, path::Path};

use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::ProviderUsageError;

const MAX_AUTH_BYTES: usize = 1_048_576;

pub(crate) async fn read(cancellation: &CancellationToken) -> Result<String, ProviderUsageError> {
    if cancellation.is_cancelled() {
        return Err(ProviderUsageError::Cancelled);
    }
    if let Some(key) = environment("OPENCODE_API_KEY")?.filter(|key| !key.is_empty()) {
        return validate_key(key);
    }
    if let Some(content) = environment("OPENCODE_AUTH_CONTENT")?.filter(|value| !value.is_empty()) {
        return select(content.as_bytes());
    }
    // OpenCode uses xdg-basedir on every platform, including macOS and Windows.
    let data = env::var_os("XDG_DATA_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| env::home_dir().map(|home| home.join(".local/share")))
        .ok_or(ProviderUsageError::CredentialUnavailable)?;
    let path = data.join("opencode/auth.json");
    tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProviderUsageError::Cancelled),
        result = tokio::time::timeout(std::time::Duration::from_secs(8), read_file(&path)) => {
            result.map_err(|_| ProviderUsageError::Timeout)?
        }
    }
}

fn environment(name: &str) -> Result<Option<String>, ProviderUsageError> {
    match env::var(name) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => Err(ProviderUsageError::CredentialUnavailable),
    }
}

async fn read_file(path: &Path) -> Result<String, ProviderUsageError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| file_error(&error))?;
    if !metadata.is_file() || metadata.len() > MAX_AUTH_BYTES as u64 {
        return Err(ProviderUsageError::CredentialUnavailable);
    }
    let file = tokio::fs::File::open(path)
        .await
        .map_err(|error| file_error(&error))?;
    let mut bytes = Vec::new();
    file.take(MAX_AUTH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| file_error(&error))?;
    select(&bytes)
}

fn file_error(error: &std::io::Error) -> ProviderUsageError {
    if error.kind() == std::io::ErrorKind::NotFound {
        ProviderUsageError::Missing
    } else {
        ProviderUsageError::CredentialUnavailable
    }
}

#[derive(Deserialize)]
struct NativeAuth {
    #[serde(rename = "opencode-go")]
    go: Option<GoAuth>,
}

#[derive(Deserialize)]
struct GoAuth {
    r#type: String,
    key: String,
}

fn select(bytes: &[u8]) -> Result<String, ProviderUsageError> {
    if bytes.len() > MAX_AUTH_BYTES {
        return Err(ProviderUsageError::CredentialUnavailable);
    }
    let auth: NativeAuth =
        serde_json::from_slice(bytes).map_err(|_| ProviderUsageError::CredentialUnavailable)?;
    let go = auth.go.ok_or(ProviderUsageError::Missing)?;
    if go.r#type != "api" {
        return Err(ProviderUsageError::Authentication);
    }
    validate_key(go.key)
}

fn validate_key(key: String) -> Result<String, ProviderUsageError> {
    if key.is_empty() || key.len() > 8_192 || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(ProviderUsageError::Authentication);
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn native_go_auth_is_exact_bounded_and_errors_do_not_expose_credentials() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = directory.path().join("auth.json");
        assert_eq!(read_file(&path).await, Err(ProviderUsageError::Missing));
        for (content, expected) in [
            (
                r#"{"opencode-go":{"type":"api","key":"fixture-key"},"opencode":{"type":"api","key":"other"}}"#,
                Ok("fixture-key".to_owned()),
            ),
            (
                r#"{"opencode":{"type":"api","key":"other"}}"#,
                Err(ProviderUsageError::Missing),
            ),
            (
                r#"{"opencode-go":{"type":"api","key":"bad\nkey"}}"#,
                Err(ProviderUsageError::Authentication),
            ),
            (
                r#"{"opencode-go":{"type":"oauth","key":"private"}}"#,
                Err(ProviderUsageError::Authentication),
            ),
            (
                r#"{"opencode-go":{"type":"api"}}"#,
                Err(ProviderUsageError::CredentialUnavailable),
            ),
            (
                "invalid-private-content",
                Err(ProviderUsageError::CredentialUnavailable),
            ),
        ] {
            tokio::fs::write(&path, content)
                .await
                .unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(read_file(&path).await, expected);
        }
        tokio::fs::write(&path, vec![b' '; MAX_AUTH_BYTES + 1])
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            read_file(&path).await,
            Err(ProviderUsageError::CredentialUnavailable)
        );
        assert_eq!(
            read_file(directory.path()).await,
            Err(ProviderUsageError::CredentialUnavailable)
        );
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        assert_eq!(
            read(&cancellation).await,
            Err(ProviderUsageError::Cancelled)
        );
    }
}
