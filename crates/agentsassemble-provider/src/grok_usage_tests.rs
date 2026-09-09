use super::*;
use serde_json::json;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn native_billing_exchange_never_creates_a_session_or_turn()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (client_input, peer_read) = tokio::io::duplex(4096);
    let (mut peer_write, client_output) = tokio::io::duplex(4096);
    let peer = tokio::spawn(async move {
        let mut lines = BufReader::new(peer_read).lines();
        let initialized: Value =
            serde_json::from_str(&lines.next_line().await?.ok_or("missing initialize")?)?;
        assert_eq!(initialized["method"], "initialize");
        peer_write.write_all(format!("{}\n", json!({"jsonrpc":"2.0", "id":initialized["id"], "result":{"protocolVersion":1, "agentCapabilities":{}}})).as_bytes()).await?;
        let billing: Value =
            serde_json::from_str(&lines.next_line().await?.ok_or("missing billing")?)?;
        assert_eq!(billing["method"], "_x.ai/billing");
        assert_eq!(billing["params"], json!({}));
        peer_write.write_all(format!("{}\n", json!({"jsonrpc":"2.0", "id":billing["id"], "result":{"config":{"creditUsagePercent":12.5, "currentPeriod":{"type":"USAGE_PERIOD_TYPE_WEEKLY", "end":"2026-09-12T00:00:00Z"}}, "subscription_tier":"private-account-detail"}})).as_bytes()).await?;
        assert!(lines.next_line().await?.is_none());
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    });
    let quota = project(
        &exchange(client_input, client_output)
            .await
            .map_err(|error| format!("{error:?}"))?,
    )?;
    peer.await??;
    let ProviderQuota::RateLimits { available, windows } = &quota else {
        return Err("wrong quota".into());
    };
    assert!(*available);
    assert_eq!(windows[0].used_percent, Some(12.5));
    assert_eq!(windows[0].window_minutes, Some(10_080));
    assert!(!serde_json::to_string(&quota)?.contains("private-account-detail"));
    Ok(())
}

#[test]
fn native_credit_shapes_preserve_unknown_values_and_reject_malformed_current_data()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for (config, expected) in [
        (
            json!({"monthlyLimit":{"val":2000},"used":{"val":500}}),
            Some(25.0),
        ),
        (json!({"monthlyLimit":{"val":2000},"used":{}}), Some(0.0)),
        (json!({"monthlyLimit":{},"used":{}}), None),
        (
            json!({"creditUsagePercent":null,"currentPeriod":{"type":"USAGE_PERIOD_TYPE_MONTHLY"}}),
            None,
        ),
    ] {
        let ProviderQuota::RateLimits { windows, .. } = project(&json!({"config":config}))? else {
            return Err("wrong quota".into());
        };
        assert_eq!(windows[0].used_percent, expected);
        assert!(windows[0].resets_at.is_none());
    }
    assert!(project(&json!({"config":{"creditUsagePercent":"invalid","monthlyLimit":{"val":1},"used":{"val":0}}})).is_err());
    assert!(project(&json!({"config":{"creditUsagePercent":-1}})).is_err());
    assert!(project(&json!({"config":{"currentPeriod":{"end":"invalid"}}})).is_err());
    assert!(project(&json!({})).is_err());
    assert!(
        matches!(project(&json!({"config":null}))?, ProviderQuota::RateLimits { available:false, windows } if windows.is_empty())
    );
    Ok(())
}
