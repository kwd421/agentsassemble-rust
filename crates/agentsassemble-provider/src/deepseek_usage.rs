use agentsassemble_domain::{ProviderBalance, ProviderQuota};
use futures_util::{FutureExt, future::BoxFuture};
use serde::Deserialize;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore, ProviderUsageError,
    remote_https::{RemoteReadError, fetch_bounded_json, fixed_catalog_client},
};

pub(crate) fn read<'a>(
    credentials: &'a ProviderCredentialStore,
    cancellation: &'a CancellationToken,
) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
    async move {
        let secret = credentials
            .secret(ProviderCredentialId::DeepSeek)
            .await
            .map_err(|error| match error {
                ProviderCredentialError::MissingSecret => ProviderUsageError::Missing,
                ProviderCredentialError::SecureStoreUnavailable => {
                    ProviderUsageError::CredentialUnavailable
                }
                ProviderCredentialError::InvalidSecret => ProviderUsageError::Authentication,
            })?;
        let client = fixed_catalog_client().map_err(|_| ProviderUsageError::Unavailable)?;
        let payload = fetch_bounded_json(
            client
                .get("https://api.deepseek.com/user/balance")
                .bearer_auth(secret.expose()),
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
struct BalanceResponse {
    is_available: bool,
    balance_infos: Vec<ProviderBalance>,
}

fn project(payload: Value) -> Result<ProviderQuota, ProviderUsageError> {
    let response: BalanceResponse =
        serde_json::from_value(payload).map_err(|_| ProviderUsageError::InvalidResponse)?;
    if response.balance_infos.is_empty()
        || response.balance_infos.len() > 2
        || response
            .balance_infos
            .iter()
            .enumerate()
            .any(|(index, balance)| {
                !matches!(balance.currency.as_str(), "CNY" | "USD")
                    || response.balance_infos[..index]
                        .iter()
                        .any(|other| other.currency == balance.currency)
                    || [
                        &balance.total_balance,
                        &balance.granted_balance,
                        &balance.topped_up_balance,
                    ]
                    .into_iter()
                    .any(|value| !decimal(value))
            })
    {
        return Err(ProviderUsageError::InvalidResponse);
    }
    Ok(ProviderQuota::Balance {
        is_available: response.is_available,
        balances: response.balance_infos,
    })
}

fn decimal(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    let mut parts = digits.split('.');
    let integer = parts.next().unwrap_or_default();
    value.len() <= 128
        && !integer.is_empty()
        && integer.bytes().all(|byte| byte.is_ascii_digit())
        && parts.next().is_none_or(|fraction| {
            !fraction.is_empty() && fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
        && parts.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn balance_projection_preserves_exact_decimals_and_rejects_missing_or_nonmonetary_values() {
        let payload = json!({"is_available": false, "balance_infos": [{
            "currency": "USD", "total_balance": "-0.000000000000000001",
            "granted_balance": "0.00", "topped_up_balance": "123456789123456789.123456789"
        }]});
        let ProviderQuota::Balance {
            is_available,
            balances,
        } = project(payload.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert!(!is_available);
        assert_eq!(balances[0].total_balance, "-0.000000000000000001");
        assert_eq!(
            balances[0].topped_up_balance,
            "123456789123456789.123456789"
        );
        for invalid in [
            Value::Null,
            json!(0),
            json!("NaN"),
            json!("1e9"),
            json!("1.2.3"),
            json!("<secret>"),
        ] {
            let mut invalid_payload = payload.clone();
            invalid_payload["balance_infos"][0]["total_balance"] = invalid;
            assert_eq!(
                project(invalid_payload),
                Err(ProviderUsageError::InvalidResponse)
            );
        }
        assert_eq!(
            project(json!({"is_available":true})),
            Err(ProviderUsageError::InvalidResponse)
        );
    }
}
