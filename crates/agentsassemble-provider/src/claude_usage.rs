use agentsassemble_domain::{ProviderQuota, ProviderRateWindow};
use futures_util::{FutureExt, future::BoxFuture};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialStore, ProviderUsageError,
    catalog::provider_executable,
    claude::{Inspection, inspect},
};

pub(crate) fn read<'a>(
    _: &'a ProviderCredentialStore,
    cancellation: &'a CancellationToken,
) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
    async move {
        let (claude, _) = provider_executable("claude", cancellation).await?;
        let output = inspect(&claude, Inspection::Usage, cancellation).await?;
        project(&output)
    }
    .boxed()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    r#type: String,
    rate_limits_available: bool,
    rate_limits: Value,
}

#[derive(Deserialize)]
struct NativeWindow {
    utilization: Option<f64>,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn project(output: &str) -> Result<ProviderQuota, ProviderUsageError> {
    let envelope: Envelope =
        serde_json::from_str(output).map_err(|_| ProviderUsageError::InvalidResponse)?;
    if envelope.r#type != "usage" {
        return Err(ProviderUsageError::InvalidResponse);
    }
    if !envelope.rate_limits_available {
        if !envelope.rate_limits.is_null() {
            return Err(ProviderUsageError::InvalidResponse);
        }
        return Ok(ProviderQuota::RateLimits {
            available: false,
            windows: vec![],
        });
    }
    let limits = envelope
        .rate_limits
        .as_object()
        .ok_or(ProviderUsageError::InvalidResponse)?;
    let mut windows = Vec::new();
    for (key, label, minutes) in [
        ("five_hour", "5시간", 300),
        ("seven_day", "7일", 10_080),
        ("seven_day_oauth_apps", "앱 · 7일", 10_080),
        ("seven_day_opus", "Opus · 7일", 10_080),
        ("seven_day_sonnet", "Sonnet · 7일", 10_080),
    ] {
        if let Some(value) = limits.get(key) {
            windows.push(window(key.to_owned(), label.to_owned(), minutes, value)?);
        }
    }
    if let Some(models) = limits.get("model_scoped") {
        let models = models
            .as_array()
            .filter(|models| models.len() <= 16)
            .ok_or(ProviderUsageError::InvalidResponse)?;
        for (index, value) in models.iter().enumerate() {
            let name = value
                .get("display_name")
                .and_then(Value::as_str)
                .filter(|name| {
                    !name.trim().is_empty()
                        && name.len() <= 80
                        && !name.chars().any(char::is_control)
                })
                .ok_or(ProviderUsageError::InvalidResponse)?;
            windows.push(window(
                format!("model_{index}"),
                format!("{name} · 7일"),
                10_080,
                value,
            )?);
        }
    }
    Ok(ProviderQuota::RateLimits {
        available: true,
        windows,
    })
}

fn window(
    id: String,
    label: String,
    minutes: u64,
    value: &Value,
) -> Result<ProviderRateWindow, ProviderUsageError> {
    let native = if value.is_null() {
        NativeWindow {
            utilization: None,
            resets_at: None,
        }
    } else {
        serde_json::from_value::<NativeWindow>(value.clone())
            .map_err(|_| ProviderUsageError::InvalidResponse)?
    };
    if native
        .utilization
        .is_some_and(|used| !used.is_finite() || used < 0.0)
    {
        return Err(ProviderUsageError::InvalidResponse);
    }
    Ok(ProviderRateWindow {
        id,
        label,
        used_percent: native.utilization,
        resets_at: native.resets_at,
        window_minutes: Some(minutes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subscription_absence_and_unknown_windows_are_not_zero_usage() {
        let unavailable =
            json!({"type":"usage", "rate_limits_available":false, "rate_limits":null});
        assert_eq!(
            project(&unavailable.to_string()),
            Ok(ProviderQuota::RateLimits {
                available: false,
                windows: vec![]
            })
        );
        let payload = json!({"type":"usage", "rate_limits_available":true, "rate_limits":{
            "five_hour":{"utilization":12.5, "resets_at":"2026-09-09T09:00:00Z"},
            "seven_day":null,
            "model_scoped":[{"display_name":"Example", "utilization":null, "resets_at":null}]
        }});
        let ProviderQuota::RateLimits { available, windows } =
            project(&payload.to_string()).unwrap_or_else(|error| panic!("{error}"))
        else {
            panic!("expected windows")
        };
        assert!(available);
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].used_percent, Some(12.5));
        assert_eq!(windows[1].used_percent, None);
        assert_eq!(windows[2].resets_at, None);
        for invalid in [json!(-1), json!("12"), json!({"secret":"not a percentage"})] {
            let mut payload = payload.clone();
            payload["rate_limits"]["five_hour"]["utilization"] = invalid;
            assert_eq!(
                project(&payload.to_string()),
                Err(ProviderUsageError::InvalidResponse)
            );
        }
    }
}
