use serde::Serialize;
use url::Url;

/// Local setup destinations. These describe installed-client help, not room authority
/// or provider availability; registration and live discovery retain those decisions.
#[derive(Debug, Serialize)]
pub struct ProviderSetupDestination {
    pub provider_id: &'static str,
    pub display_name: &'static str,
    pub help_url: &'static str,
}

/// A private local observation; release availability never gates agent creation.
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderUpdate {
    pub provider_id: String,
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub native_update: bool,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub handoff_started: bool,
}

pub const PROVIDER_SETUP_SCHEME: &str = "agentsassemble";
pub const PROVIDER_SETUP_HOST: &str = "provider-setup";

pub const PROVIDER_SETUP_DESTINATIONS: &[ProviderSetupDestination] = &[
    ProviderSetupDestination {
        provider_id: "codex",
        display_name: "Codex",
        help_url: "https://learn.chatgpt.com/docs/codex/cli",
    },
    ProviderSetupDestination {
        provider_id: "claude",
        display_name: "Claude Code",
        help_url: "https://code.claude.com/docs/en/setup",
    },
    ProviderSetupDestination {
        provider_id: "cursor",
        display_name: "Cursor",
        help_url: "https://cursor.com/docs/cli/installation",
    },
    ProviderSetupDestination {
        provider_id: "grok",
        display_name: "Grok",
        help_url: "https://docs.x.ai/build/cli/reference",
    },
    ProviderSetupDestination {
        provider_id: "opencode",
        display_name: "OpenCode",
        help_url: "https://opencode.ai/docs/",
    },
    ProviderSetupDestination {
        provider_id: "ollama",
        display_name: "Ollama",
        help_url: "https://ollama.com/download",
    },
    ProviderSetupDestination {
        provider_id: "lmstudio",
        display_name: "LM Studio",
        help_url: "https://lmstudio.ai/docs/app",
    },
];

#[must_use]
pub fn provider_setup_destination(id: &str) -> Option<&'static ProviderSetupDestination> {
    PROVIDER_SETUP_DESTINATIONS
        .iter()
        .find(|destination| destination.provider_id == id)
}

/// A deep link can select a local setup page, never convey execution authority.
#[must_use]
pub fn provider_setup_from_url(url: &Url) -> Option<&'static ProviderSetupDestination> {
    if url.scheme() != PROVIDER_SETUP_SCHEME
        || url.host_str() != Some(PROVIDER_SETUP_HOST)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    provider_setup_destination(url.path().strip_prefix('/')?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_links_accept_only_known_local_pages_without_authority_or_commands() {
        for destination in PROVIDER_SETUP_DESTINATIONS {
            let url = Url::parse(&format!(
                "{PROVIDER_SETUP_SCHEME}://{PROVIDER_SETUP_HOST}/{}",
                destination.provider_id
            ))
            .unwrap_or_else(|error| panic!("valid setup URL: {error}"));
            assert_eq!(
                provider_setup_from_url(&url).map(|target| target.provider_id),
                Some(destination.provider_id)
            );
        }
        for text in [
            "https://provider-setup/codex",
            "agentsassemble://other/codex",
            "agentsassemble://user@provider-setup/codex",
            "agentsassemble://provider-setup:123/codex",
            "agentsassemble://provider-setup/codex?run=login",
            "agentsassemble://provider-setup/codex#token",
            "agentsassemble://provider-setup/codex/",
            "agentsassemble://provider-setup/%63odex",
            "agentsassemble://provider-setup/antigravity",
            "agentsassemble://provider-setup/freebuff",
            "agentsassemble://provider-setup/unknown",
        ] {
            let url = Url::parse(text).unwrap_or_else(|error| panic!("test URL: {error}"));
            assert!(provider_setup_from_url(&url).is_none());
        }
    }
}
