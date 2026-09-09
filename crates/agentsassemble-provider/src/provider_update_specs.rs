use agentsassemble_domain::ProviderUpdate;
use semver::Version;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use super::provider_update::ProviderUpdateError;
use crate::{
    process::probe,
    remote_https::{fetch_bounded_json, fixed_catalog_client},
};

type Error = ProviderUpdateError;

pub(crate) struct ProviderUpdateSpec {
    pub(crate) bundled_codex: bool,
    pub(crate) source: ReleaseSource,
    pub(crate) version_prefix: &'static str,
    pub(crate) version_suffix: &'static str,
    pub(crate) update_arguments: Option<&'static [&'static str]>,
}

pub(crate) enum ReleaseSource {
    Json {
        endpoint: &'static str,
        field: &'static str,
        tag_prefix: &'static str,
    },
    Cursor,
    Grok,
}

pub(crate) const CODEX: ProviderUpdateSpec = ProviderUpdateSpec {
    bundled_codex: true,
    source: ReleaseSource::Json {
        endpoint: "https://registry.npmjs.org/@openai/codex/latest",
        field: "version",
        tag_prefix: "",
    },
    version_prefix: "codex-cli ",
    version_suffix: "",
    update_arguments: None,
};
pub(crate) const CLAUDE: ProviderUpdateSpec = ProviderUpdateSpec {
    bundled_codex: false,
    source: ReleaseSource::Json {
        endpoint: "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
        field: "version",
        tag_prefix: "",
    },
    version_prefix: "",
    version_suffix: " (Claude Code)",
    update_arguments: None,
};
pub(crate) const OPENCODE: ProviderUpdateSpec = ProviderUpdateSpec {
    bundled_codex: false,
    source: ReleaseSource::Json {
        endpoint: "https://api.github.com/repos/anomalyco/opencode/releases/latest",
        field: "tag_name",
        tag_prefix: "v",
    },
    version_prefix: "",
    version_suffix: "",
    update_arguments: Some(&["upgrade"]),
};
pub(crate) const CURSOR: ProviderUpdateSpec = ProviderUpdateSpec {
    bundled_codex: false,
    source: ReleaseSource::Cursor,
    version_prefix: "",
    version_suffix: "",
    update_arguments: Some(&["update"]),
};
pub(crate) const GROK: ProviderUpdateSpec = ProviderUpdateSpec {
    bundled_codex: false,
    source: ReleaseSource::Grok,
    version_prefix: "",
    version_suffix: "",
    update_arguments: Some(&["update"]),
};

impl ProviderUpdateSpec {
    pub(crate) async fn read(
        &self,
        id: &str,
        executable: &str,
        cancellation: &CancellationToken,
    ) -> Result<ProviderUpdate, Error> {
        let (current, latest, available) = if let ReleaseSource::Grok = self.source {
            let text = probe(
                executable,
                &["update", "--check", "--json"],
                cancellation,
                &[],
            )
            .await
            .map_err(Error::from)?;
            let value: GrokUpdate =
                serde_json::from_str(&text).map_err(|_| Error::InvalidResponse)?;
            if value.error.is_some() {
                return Err(Error::Unavailable);
            }
            let available = newer_semver(&value.current_version, &value.latest_version)?;
            if available != value.update_available {
                return Err(Error::InvalidResponse);
            }
            (value.current_version, value.latest_version, available)
        } else {
            let text = probe(executable, &["--version"], cancellation, &[])
                .await
                .map_err(Error::from)?;
            let current = text
                .trim()
                .strip_prefix(self.version_prefix)
                .and_then(|s| s.strip_suffix(self.version_suffix))
                .ok_or(Error::InvalidResponse)?
                .to_owned();
            let channel = if matches!(self.source, ReleaseSource::Cursor) {
                let value = probe(executable, &["get-channel"], cancellation, &[])
                    .await
                    .map_err(Error::from)?;
                match value.trim() {
                    "" | "prod" | "prod-stable-internal" => "prod",
                    "lab" => "lab",
                    _ => return Err(Error::Unsupported),
                }
            } else {
                ""
            };
            let client = fixed_catalog_client().map_err(|_| Error::Unavailable)?;
            let (request, field, prefix) = match self.source {
                ReleaseSource::Json {
                    endpoint,
                    field,
                    tag_prefix,
                } => (client.get(endpoint), field, tag_prefix),
                ReleaseSource::Cursor => (
                    client
                        .post(
                            "https://api2.cursor.sh/aiserver.v1.DashboardService/GetCliDownloadUrl",
                        )
                        .header("Connect-Protocol-Version", "1")
                        .json(&serde_json::json!({"channel":channel})),
                    "version",
                    "",
                ),
                ReleaseSource::Grok => unreachable!(),
            };
            let value = fetch_bounded_json(request, 128 * 1024, cancellation)
                .await
                .map_err(|_| Error::Unavailable)?;
            let latest = value
                .get(field)
                .and_then(serde_json::Value::as_str)
                .and_then(|s| s.strip_prefix(prefix))
                .ok_or(Error::InvalidResponse)?
                .to_owned();
            let available = if matches!(self.source, ReleaseSource::Cursor) {
                newer_cursor(&current, &latest)?
            } else {
                newer_semver(&current, &latest)?
            };
            (current, latest, available)
        };
        Ok(ProviderUpdate {
            provider_id: id.to_owned(),
            current_version: current,
            latest_version: latest,
            update_available: available,
            native_update: self.update_arguments.is_some(),
            observed_at: chrono::Utc::now(),
            handoff_started: false,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrokUpdate {
    current_version: String,
    latest_version: String,
    update_available: bool,
    error: Option<serde_json::Value>,
}

fn newer_semver(current: &str, latest: &str) -> Result<bool, Error> {
    if current.len() > 64 || latest.len() > 64 {
        return Err(Error::InvalidResponse);
    }
    let current = Version::parse(current).map_err(|_| Error::InvalidResponse)?;
    let latest = Version::parse(latest).map_err(|_| Error::InvalidResponse)?;
    Ok(latest > current)
}

fn newer_cursor(current: &str, latest: &str) -> Result<bool, Error> {
    fn date(value: &str) -> Result<chrono::NaiveDate, Error> {
        let (date, hash) = value.split_once('-').ok_or(Error::InvalidResponse)?;
        if date.len() != 10 || hash.len() != 7 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::InvalidResponse);
        }
        chrono::NaiveDate::parse_from_str(date, "%Y.%m.%d").map_err(|_| Error::InvalidResponse)
    }
    let current_date = date(current)?;
    let latest_date = date(latest)?;
    // Cursor's manual-update contract accepts different builds on the same day.
    Ok(latest_date > current_date || (latest_date == current_date && latest != current))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_preserve_order_and_reject_untrusted_display_text() {
        assert_eq!(newer_semver("1.9.0", "1.10.0"), Ok(true));
        assert_eq!(newer_semver("2.0.0", "1.10.0"), Ok(false));
        assert_eq!(newer_semver("2.0.0", "2.0.0"), Ok(false));
        assert_eq!(newer_semver("2.0.0-beta.1", "2.0.0"), Ok(true));
        assert!(newer_semver("private diagnostic", "2.0.0").is_err());
        assert_eq!(
            newer_cursor("2026.08.11-e8db854", "2026.09.08-6caf4ff"),
            Ok(true)
        );
        assert_eq!(
            newer_cursor("2026.09.08-6caf4ff", "2026.08.11-e8db854"),
            Ok(false)
        );
        assert!(newer_cursor("2026.99.99-e8db854", "2026.09.08-6caf4ff").is_err());
    }
}
