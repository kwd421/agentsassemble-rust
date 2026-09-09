use agentsassemble_domain::{ProviderQuota, ProviderRateWindow};
use futures_util::{FutureExt, future::BoxFuture};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialStore, ProviderUsageError,
    acp_client::{AcpClient, AcpPermissionPolicy},
    catalog::provider_executable,
    process::{ProbeFailure, inspect},
};

pub(crate) fn read<'a>(
    _: &'a ProviderCredentialStore,
    cancellation: &'a CancellationToken,
) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
    async move {
        let (executable, _) = provider_executable("grok", cancellation).await?;
        let home = tempfile::tempdir().map_err(|_| ProviderUsageError::Unavailable)?;
        let auth = crate::grok_acp::auth_path().ok_or(ProviderUsageError::Missing)?;
        let environment = [
            (
                "GROK_HOME".to_owned(),
                home.path()
                    .to_str()
                    .ok_or(ProviderUsageError::Unavailable)?
                    .to_owned(),
            ),
            (
                "GROK_AUTH_PATH".to_owned(),
                auth.to_str()
                    .ok_or(ProviderUsageError::Unavailable)?
                    .to_owned(),
            ),
            (
                "GROK_DEFAULT_SELECTED_PERMISSION".to_owned(),
                "reject".to_owned(),
            ),
        ];
        let payload = inspect(
            &executable,
            &["agent", "stdio"],
            cancellation,
            &environment,
            exchange,
        )
        .await?;
        project(&payload)
    }
    .boxed()
}

async fn exchange<I, O>(input: I, output: O) -> Result<Value, ProbeFailure>
where
    I: AsyncWrite + Unpin + Send + 'static,
    O: AsyncRead + Unpin + Send + 'static,
{
    let mut client = AcpClient::connect(input, output, AcpPermissionPolicy::Reject)
        .await
        .map_err(|_| ProbeFailure::Failed)?;
    let result = client
        .request_extension("x.ai/billing", serde_json::json!({}))
        .await;
    client.shutdown().await;
    result.map_err(|_| ProbeFailure::Failed)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Period {
    #[serde(rename = "type")]
    kind: Option<String>,
    end: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Credits {
    credit_usage_percent: Option<f64>,
    current_period: Option<Period>,
}

#[derive(Deserialize)]
struct Cent {
    // Native proto3 JSON explicitly represents zero cents as an empty object.
    #[serde(default)]
    val: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BillingBudget {
    monthly_limit: Cent,
    used: Option<Cent>,
    billing_period_end: Option<chrono::DateTime<chrono::Utc>>,
}

fn project(payload: &Value) -> Result<ProviderQuota, ProviderUsageError> {
    let invalid = || ProviderUsageError::InvalidResponse;
    let config = payload.get("config").ok_or_else(invalid)?;
    if config.is_null() {
        return Ok(ProviderQuota::RateLimits {
            available: false,
            windows: vec![],
        });
    }
    // Both shapes are defined by the native BillingConfig contract. Invalid current
    // credits never switch to the deprecated monthly projection.
    let (id, label, percent, reset, minutes) =
        if config.get("creditUsagePercent").is_some() || config.get("currentPeriod").is_some() {
            let credits: Credits = serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            let kind = credits
                .current_period
                .as_ref()
                .and_then(|period| period.kind.as_deref());
            let (id, label, minutes) = match kind {
                Some("USAGE_PERIOD_TYPE_WEEKLY") => ("weekly", "주간", Some(10_080)),
                Some("USAGE_PERIOD_TYPE_MONTHLY") => ("monthly", "월간", None),
                Some(_) | None => ("current", "현재 기간", None),
            };
            (
                id,
                label,
                credits.credit_usage_percent,
                credits.current_period.and_then(|period| period.end),
                minutes,
            )
        } else {
            let monthly: BillingBudget =
                serde_json::from_value(config.clone()).map_err(|_| invalid())?;
            if monthly.monthly_limit.val < 0.0
                || monthly.monthly_limit.val.fract() != 0.0
                || monthly
                    .used
                    .as_ref()
                    .is_some_and(|used| used.val < 0.0 || used.val.fract() != 0.0)
            {
                return Err(invalid());
            }
            let percent = monthly
                .used
                .filter(|_| monthly.monthly_limit.val > 0.0)
                .map(|used| 100.0 * used.val / monthly.monthly_limit.val);
            ("monthly", "월간", percent, monthly.billing_period_end, None)
        };
    if percent.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return Err(invalid());
    }
    Ok(ProviderQuota::RateLimits {
        available: true,
        windows: vec![ProviderRateWindow {
            id: format!("grok-{id}"),
            label: label.to_owned(),
            used_percent: percent,
            resets_at: reset,
            window_minutes: minutes,
        }],
    })
}

#[cfg(test)]
#[path = "grok_usage_tests.rs"]
mod tests;
