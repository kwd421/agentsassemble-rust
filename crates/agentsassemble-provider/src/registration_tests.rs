use std::collections::BTreeSet;

use crate::registration::provider_registrations;

#[test]
fn registrations_cover_each_retained_provider_once() {
    let expected = BTreeSet::from([
        "antigravity",
        "cerebras",
        "claude",
        "codex",
        "cursor",
        "custom_api",
        "deepseek",
        "freebuff",
        "grok",
        "llmgateway",
        "lmstudio",
        "ollama",
        "opencode",
        "openrouter",
        "tokenrouter",
        "vercel",
    ]);
    let registrations = provider_registrations();
    let actual = registrations
        .iter()
        .map(|registration| registration.id)
        .collect::<BTreeSet<_>>();

    assert_eq!(registrations.len(), expected.len());
    assert_eq!(actual, expected);
}
