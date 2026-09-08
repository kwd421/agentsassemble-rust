use super::*;

#[tokio::test]
async fn account_exchange_has_no_thread_or_turn_and_keeps_multiple_nullable_buckets() {
    let (client, server) = tokio::io::duplex(8192);
    let (input, output) = tokio::io::split(client);
    let (native_input, native_output) = tokio::io::split(server);
    let payload = json!({
        "rateLimits": {"primary":{"usedPercent":99}},
        "rateLimitsByLimitId": {
            "codex":{"limitId":"codex", "primary":{"usedPercent":25, "windowDurationMins":300, "resetsAt":1_788_915_600}, "secondary":null},
            "codex_other":{"limitId":"codex_other", "limitName":"Other", "primary":{"usedPercent":null, "windowDurationMins":60, "resetsAt":null}}
        },
        "rateLimitResetCredits":{"credits":[{"id":"private-reset-id"}]}
    });
    let native = async {
        let mut wire = CodexWire::new(native_output, native_input);
        let (initialize, _) = wire
            .read_message()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(initialize["method"], "initialize");
        wire.write_message(&json!({"id":1,"result":{}}))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        let (initialized, _) = wire
            .read_message()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(initialized["method"], "initialized");
        let (read, _) = wire
            .read_message()
            .await
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            read,
            json!({"jsonrpc":"2.0", "id":2, "method":"account/rateLimits/read"})
        );
        wire.write_message(
            &json!({"method":"account/updated","params":{"private_account":"never projected"}}),
        )
        .await
        .unwrap_or_else(|error| panic!("{error}"));
        wire.write_message(&json!({"id":2,"result":payload}))
            .await
            .unwrap_or_else(|error| panic!("{error}"));
    };
    let (result, ()) = tokio::join!(exchange(output, input), native);
    let quota = project(&result.unwrap_or_else(|error| panic!("{error:?}")))
        .unwrap_or_else(|error| panic!("{error}"));
    let ProviderQuota::RateLimits { available, windows } = &quota else {
        panic!("expected windows")
    };
    assert!(*available);
    assert_eq!(windows.len(), 2);
    assert_eq!(windows[0].used_percent, Some(25.0));
    assert_eq!(windows[0].window_minutes, Some(300));
    assert_eq!(windows[1].used_percent, None);
    assert_eq!(windows[1].resets_at, None);
    assert!(
        !serde_json::to_string(&quota)
            .unwrap_or_default()
            .contains("private")
    );
}

#[test]
fn invalid_multibucket_view_does_not_recover_from_the_single_bucket_view() {
    let payload =
        json!({"rateLimits": {"primary":{"usedPercent":25}}, "rateLimitsByLimitId":"invalid"});
    assert_eq!(project(&payload), Err(ProviderUsageError::InvalidResponse));
    assert_eq!(
        project(&json!({"rateLimits":null})),
        Ok(ProviderQuota::RateLimits {
            available: false,
            windows: vec![]
        })
    );
    let quota = project(&json!({"rateLimits":{"primary":{"usedPercent":20,"resetsAt":null}}}))
        .unwrap_or_else(|error| panic!("{error}"));
    let ProviderQuota::RateLimits { windows, .. } = quota else {
        panic!("expected windows")
    };
    assert_eq!(windows[0].used_percent, Some(20.0));
    assert_eq!(windows[0].window_minutes, None);
}
