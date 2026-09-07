use std::collections::BTreeSet;

use crate::registration::provider_registrations;

#[test]
fn registrations_cover_each_retained_provider_once() {
    let expected = BTreeSet::from([
        "cerebras",
        "claude",
        "codex",
        "cursor",
        "custom_api",
        "deepseek",
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
    let advertised_interrupt = registrations
        .iter()
        .map(|registration| crate::registration::loading_provider(registration))
        .filter(|provider| provider.turn_interrupt.retains_runtime())
        .map(|provider| provider.id)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        advertised_interrupt,
        BTreeSet::from([
            "codex".to_owned(),
            "cursor".to_owned(),
            "grok".to_owned(),
            "opencode".to_owned(),
        ])
    );
}
