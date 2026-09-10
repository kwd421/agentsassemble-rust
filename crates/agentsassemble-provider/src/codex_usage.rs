use std::collections::BTreeMap;

use agentsassemble_domain::{ProviderQuota, ProviderRateWindow};
use futures_util::{FutureExt, future::BoxFuture};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use super::{
    MAX_PENDING_NOTIFICATION_BYTES, MAX_PENDING_NOTIFICATIONS, config, protocol::CodexWire,
};
use crate::{
    ProviderCredentialStore, ProviderUsageError,
    catalog::resolved_codex,
    process::{ProbeFailure, inspect},
};

pub(crate) fn read<'a>(
    _: &'a ProviderCredentialStore,
    cancellation: &'a CancellationToken,
) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
    async move {
        let (executable, _) = resolved_codex(cancellation).await?;
        let configuration = config::load()
            .await
            .map_err(|_| ProviderUsageError::Unavailable)?;
        let mut arguments = vec!["app-server".to_owned(), "--stdio".to_owned()];
        config::append_mcp_isolation(&mut arguments, &configuration.inherited_mcp_servers)
            .map_err(|_| ProviderUsageError::Unavailable)?;
        let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let payload = inspect(
            &executable,
            &arguments,
            std::time::Duration::from_secs(10),
            cancellation,
            &[("CODEX_HOME".to_owned(), configuration.home)],
            exchange,
        )
        .await?;
        project(&payload)
    }
    .boxed()
}

async fn exchange<I: AsyncWrite + Unpin, O: AsyncRead + Unpin>(
    input: I,
    output: O,
) -> Result<Value, ProbeFailure> {
    let mut wire = CodexWire::new(input, output);
    wire.write_message(
        &json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
            "clientInfo":{"name":"AgentsAssemble", "version":"0"}
        }}),
    )
    .await
    .map_err(|_| ProbeFailure::Malformed)?;
    response(&mut wire, 1).await?;
    wire.write_message(&json!({"jsonrpc":"2.0", "method":"initialized", "params":{}}))
        .await
        .map_err(|_| ProbeFailure::Malformed)?;
    wire.write_message(&json!({"jsonrpc":"2.0", "id":2, "method":"account/rateLimits/read"}))
        .await
        .map_err(|_| ProbeFailure::Malformed)?;
    response(&mut wire, 2).await
}

async fn response<I: AsyncWrite + Unpin, O: AsyncRead + Unpin>(
    wire: &mut CodexWire<I, O>,
    expected: u64,
) -> Result<Value, ProbeFailure> {
    let mut bytes = 0usize;
    for _ in 0..=MAX_PENDING_NOTIFICATIONS {
        let (message, length) = wire
            .read_message()
            .await
            .map_err(|_| ProbeFailure::Malformed)?;
        bytes = bytes.saturating_add(length);
        if bytes > MAX_PENDING_NOTIFICATION_BYTES {
            return Err(ProbeFailure::CatalogTooLarge);
        }
        if message.get("method").is_some() {
            if let Some(id) = message.get("id") {
                wire.write_message(&json!({"jsonrpc":"2.0", "id":id, "error":{
                    "code":-32601, "message":"Account inspection does not support provider requests."
                }})).await.map_err(|_| ProbeFailure::Malformed)?;
            }
            continue;
        }
        if message.get("id").and_then(Value::as_u64) != Some(expected) {
            return Err(ProbeFailure::Malformed);
        }
        if message.get("error").is_some_and(|value| !value.is_null()) {
            return Err(ProbeFailure::Failed);
        }
        return message
            .get("result")
            .cloned()
            .ok_or(ProbeFailure::Malformed);
    }
    Err(ProbeFailure::CatalogTooLarge)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NativeWindow {
    used_percent: Option<f64>,
    window_duration_mins: Option<u64>,
    resets_at: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bucket {
    limit_id: Option<String>,
    limit_name: Option<String>,
    primary: Option<NativeWindow>,
    secondary: Option<NativeWindow>,
}

fn project(payload: &Value) -> Result<ProviderQuota, ProviderUsageError> {
    let invalid = || ProviderUsageError::InvalidResponse;
    if !payload.is_object() {
        return Err(invalid());
    }
    // These are two documented native projections, not an error-recovery path.
    let buckets: BTreeMap<String, Bucket> = match payload.get("rateLimitsByLimitId") {
        Some(Value::Null) | None => match payload.get("rateLimits") {
            Some(Value::Null) => {
                return Ok(ProviderQuota::RateLimits {
                    available: false,
                    windows: vec![],
                });
            }
            Some(value) => {
                let bucket: Bucket =
                    serde_json::from_value(value.clone()).map_err(|_| invalid())?;
                BTreeMap::from([(
                    bucket
                        .limit_id
                        .clone()
                        .unwrap_or_else(|| "codex".to_owned()),
                    bucket,
                )])
            }
            None => return Err(invalid()),
        },
        Some(value) => serde_json::from_value(value.clone()).map_err(|_| invalid())?,
    };
    if buckets.len() > 32 {
        return Err(invalid());
    }
    let mut windows = Vec::new();
    for (id, bucket) in buckets {
        if !bounded_label(&id) || bucket.limit_id.as_ref().is_some_and(|native| native != &id) {
            return Err(invalid());
        }
        let label = bucket.limit_name.as_deref().unwrap_or(&id);
        if !bounded_label(label) {
            return Err(invalid());
        }
        for (slot, native) in [("primary", bucket.primary), ("secondary", bucket.secondary)] {
            if let Some(native) = native {
                windows.push(window(&id, label, slot, &native)?);
            }
        }
    }
    Ok(ProviderQuota::RateLimits {
        available: true,
        windows,
    })
}

fn bounded_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 80 && !value.chars().any(char::is_control)
}

fn window(
    id: &str,
    label: &str,
    slot: &str,
    native: &NativeWindow,
) -> Result<ProviderRateWindow, ProviderUsageError> {
    if native
        .used_percent
        .is_some_and(|used| !used.is_finite() || used < 0.0)
        || native.window_duration_mins == Some(0)
    {
        return Err(ProviderUsageError::InvalidResponse);
    }
    let resets_at = native
        .resets_at
        .map(|seconds| {
            chrono::DateTime::from_timestamp(seconds, 0).ok_or(ProviderUsageError::InvalidResponse)
        })
        .transpose()?;
    let label = native.window_duration_mins.map_or_else(
        || format!("{label} · {slot}"),
        |minutes| format!("{label} · {minutes}분"),
    );
    Ok(ProviderRateWindow {
        id: format!("{id}/{slot}"),
        label,
        used_percent: native.used_percent,
        resets_at,
        window_minutes: native.window_duration_mins,
    })
}

#[cfg(test)]
#[path = "codex_usage_tests.rs"]
mod tests;
