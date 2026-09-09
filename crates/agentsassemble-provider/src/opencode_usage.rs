use agentsassemble_domain::{ProviderQuota, ProviderRateWindow};
use futures_util::{FutureExt, future::BoxFuture};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialStore, ProviderUsageError,
    remote_https::{RemoteReadError, fetch_bounded_json, fixed_catalog_client},
};

pub(crate) fn read<'a>(
    _: &'a ProviderCredentialStore,
    cancellation: &'a CancellationToken,
) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
    async move {
        let key = crate::opencode_usage_auth::read(cancellation).await?;
        let client = fixed_catalog_client().map_err(|_| ProviderUsageError::Unavailable)?;
        let payload = fetch_bounded_json(
            client
                .get("https://opencode.ai/zen/go/v1/usage")
                .bearer_auth(key),
            16_384,
            cancellation,
        )
        .await
        .map_err(|error| match error {
            RemoteReadError::Cancelled => ProviderUsageError::Cancelled,
            RemoteReadError::Timeout => ProviderUsageError::Timeout,
            RemoteReadError::Authentication => ProviderUsageError::Authentication,
            RemoteReadError::Malformed | RemoteReadError::TooLarge => {
                ProviderUsageError::InvalidResponse
            }
            RemoteReadError::Failed => ProviderUsageError::Unavailable,
        })?;
        project(payload)
    }
    .boxed()
}

#[derive(Deserialize)]
struct Envelope {
    usage: Windows,
}

#[derive(Deserialize)]
struct Windows {
    rolling: Window,
    weekly: Window,
    monthly: Window,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Window {
    #[serde(rename = "status")]
    _status: Status,
    percent: f64,
    resets_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Status {
    Ok,
    RateLimited,
}

fn project(payload: Value) -> Result<ProviderQuota, ProviderUsageError> {
    let Envelope { usage } =
        serde_json::from_value(payload).map_err(|_| ProviderUsageError::InvalidResponse)?;
    let mut windows = Vec::with_capacity(3);
    for (id, label, minutes, window) in [
        ("go-rolling", "Go · 5시간", Some(300), usage.rolling),
        ("go-weekly", "Go · 주간", Some(10_080), usage.weekly),
        ("go-monthly", "Go · 월간", None, usage.monthly),
    ] {
        if !window.percent.is_finite() || window.percent < 0.0 {
            return Err(ProviderUsageError::InvalidResponse);
        }
        windows.push(ProviderRateWindow {
            id: id.to_owned(),
            label: label.to_owned(),
            used_percent: Some(window.percent),
            resets_at: Some(window.resets_at),
            window_minutes: minutes,
        });
    }
    Ok(ProviderQuota::RateLimits {
        available: true,
        windows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn go_measurements_preserve_overage_and_reject_missing_or_malformed_data() {
        let window = json!({"status":"ok", "percent":12.5, "resetsAt":"2026-09-09T13:00:00Z"});
        let mut payload = json!({"usage":{"rolling":window,"weekly":window,"monthly":window}, "private":"excluded"});
        payload["usage"]["monthly"]["percent"] = json!(123.0);
        payload["usage"]["monthly"]["status"] = json!("rate-limited");
        let quota = project(payload.clone()).unwrap_or_else(|error| panic!("{error}"));
        let ProviderQuota::RateLimits { windows, .. } = &quota else {
            panic!("expected windows")
        };
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].used_percent, Some(12.5));
        assert_eq!(windows[2].used_percent, Some(123.0));
        assert_eq!(windows[2].window_minutes, None);
        assert!(
            !serde_json::to_string(&quota)
                .unwrap_or_default()
                .contains("excluded")
        );
        for (field, invalid) in [
            ("percent", Value::Null),
            ("percent", json!(-1)),
            ("percent", json!("0")),
            ("status", json!("unknown")),
            ("resetsAt", json!("invalid")),
        ] {
            let mut invalid_payload = payload.clone();
            invalid_payload["usage"]["rolling"][field] = invalid;
            assert_eq!(
                project(invalid_payload),
                Err(ProviderUsageError::InvalidResponse)
            );
        }
        payload["usage"]
            .as_object_mut()
            .unwrap_or_else(|| panic!("object"))
            .remove("weekly");
        assert_eq!(project(payload), Err(ProviderUsageError::InvalidResponse));
    }
}
