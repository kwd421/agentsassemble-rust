use std::{path::Path, time::Duration};

use agentsassemble_domain::ProviderInstall;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::provider_executable,
    process::{ProbeFailure, probe, probe_with_timeout},
    provider_update::ProviderUpdateError,
    registration::provider_registration_by_id,
    remote_https::{fetch_bounded_json, fixed_catalog_client},
};

type Error = ProviderUpdateError;

const INSTALL_TIMEOUT: Duration = Duration::from_mins(10);

/// A provider whose official CLI is published as one npm package.
///
/// The launcher is the registration's own `probe_executable`, so a completed install is
/// found by the same search that reported it missing. Other providers keep official help.
pub(crate) struct NpmInstall {
    pub(crate) provider_id: &'static str,
    pub(crate) package: &'static str,
    pub(crate) latest_endpoint: &'static str,
}

pub(crate) const NPM_INSTALLS: &[NpmInstall] = &[
    NpmInstall {
        provider_id: "codex",
        package: "@openai/codex",
        latest_endpoint: "https://registry.npmjs.org/@openai/codex/latest",
    },
    NpmInstall {
        provider_id: "claude",
        package: "@anthropic-ai/claude-code",
        latest_endpoint: "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
    },
    NpmInstall {
        provider_id: "opencode",
        package: "opencode-ai",
        latest_endpoint: "https://registry.npmjs.org/opencode-ai/latest",
    },
];

/// Whether the catalog should offer an in-app install for this provider's missing CLI.
pub(crate) fn supports(provider_id: &str) -> bool {
    NPM_INSTALLS
        .iter()
        .any(|install| install.provider_id == provider_id)
}

/// Everything the confirmed run needs, re-derived for every request.
pub(crate) struct InstallPlan {
    launcher: &'static str,
    npm: String,
    prefix: String,
    pub(crate) offer: ProviderInstall,
}

/// Builds an offer only for a registered npm provider whose launcher is currently missing.
pub(crate) async fn plan(
    provider_id: &str,
    cancellation: &CancellationToken,
) -> Result<InstallPlan, Error> {
    let install = NPM_INSTALLS
        .iter()
        .find(|install| install.provider_id == provider_id)
        .ok_or(Error::Unsupported)?;
    let launcher = provider_registration_by_id(provider_id)
        .map(|registration| registration.probe_executable)
        .filter(|launcher| !launcher.is_empty())
        .ok_or(Error::Unsupported)?;
    match provider_executable(launcher, cancellation).await {
        Ok(_) => return Err(Error::AlreadyInstalled),
        Err(ProbeFailure::Missing) => {}
        Err(error) => return Err(error.into()),
    }
    let (npm, _) = provider_executable("npm", cancellation)
        .await
        .map_err(|error| match error {
            ProbeFailure::Missing => Error::NpmMissing,
            error => error.into(),
        })?;
    let prefix = probe(&npm, &["prefix", "--global"], cancellation, &[]).await?;
    let prefix = prefix.trim().to_owned();
    if !Path::new(&prefix).is_absolute()
        || prefix.len() > 4096
        || prefix.contains(['\n', '\r', '\0'])
    {
        return Err(Error::InvalidResponse);
    }
    let version = latest_version(install.latest_endpoint, cancellation).await?;
    let offer = ProviderInstall {
        provider_id: install.provider_id.to_owned(),
        package: install.package.to_owned(),
        command: install_arguments(&prefix, install.package, &version)
            .into_iter()
            .fold(vec!["npm".to_owned()], |mut command, argument| {
                command.push(argument);
                command
            }),
        version,
        completed: false,
    };
    Ok(InstallPlan {
        launcher,
        npm,
        prefix,
        offer,
    })
}

/// Runs the exact offered command and confirms discovery finds the launcher npm just wrote.
pub(crate) async fn run(
    plan: InstallPlan,
    expected_version: &str,
    cancellation: &CancellationToken,
) -> Result<ProviderInstall, Error> {
    if plan.offer.version != expected_version {
        return Err(Error::OfferChanged);
    }
    let arguments = install_arguments(&plan.prefix, &plan.offer.package, &plan.offer.version);
    let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    probe_with_timeout(&plan.npm, &arguments, INSTALL_TIMEOUT, cancellation, &[])
        .await
        .map_err(|error| match error {
            ProbeFailure::Cancelled => Error::Cancelled,
            ProbeFailure::CleanupUnconfirmed => Error::CleanupUnconfirmed,
            _ => Error::InstallationUnconfirmed,
        })?;
    let (installed, _) = provider_executable(plan.launcher, cancellation)
        .await
        .map_err(|error| match error {
            // npm finished, but its launcher directory is not on the runtime's PATH.
            ProbeFailure::Missing => Error::InstalledOutsidePath,
            error => error.into(),
        })?;
    let prefix = Path::new(&plan.prefix)
        .canonicalize()
        .map_err(|_| Error::InstallationUnconfirmed)?;
    let installed = Path::new(&installed)
        .canonicalize()
        .map_err(|_| Error::InstallationUnconfirmed)?;
    if !installed.starts_with(&prefix) {
        return Err(Error::InstallationUnconfirmed);
    }
    Ok(ProviderInstall {
        completed: true,
        ..plan.offer
    })
}

fn install_arguments(prefix: &str, package: &str, version: &str) -> Vec<String> {
    vec![
        "install".to_owned(),
        "--global".to_owned(),
        "--prefix".to_owned(),
        prefix.to_owned(),
        format!("{package}@{version}"),
    ]
}

async fn latest_version(endpoint: &str, cancellation: &CancellationToken) -> Result<String, Error> {
    let client = fixed_catalog_client().map_err(|_| Error::Unavailable)?;
    let value = fetch_bounded_json(client.get(endpoint), 128 * 1024, cancellation)
        .await
        .map_err(|_| Error::Unavailable)?;
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::InvalidResponse)?;
    if version.len() > 64 || semver::Version::parse(version).is_err() {
        return Err(Error::InvalidResponse);
    }
    Ok(version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_npm_install_targets_a_registered_launcher_and_its_own_package() {
        for install in NPM_INSTALLS {
            let registration = provider_registration_by_id(install.provider_id)
                .unwrap_or_else(|| panic!("{} is not registered", install.provider_id));
            assert!(!registration.probe_executable.is_empty());
            assert_eq!(
                install.latest_endpoint,
                format!("https://registry.npmjs.org/{}/latest", install.package)
            );
        }
    }

    #[test]
    fn install_arguments_pin_the_confirmed_version_and_prefix() {
        assert_eq!(
            install_arguments("C:\\npm", "@anthropic-ai/claude-code", "2.1.0"),
            [
                "install",
                "--global",
                "--prefix",
                "C:\\npm",
                "@anthropic-ai/claude-code@2.1.0"
            ]
        );
    }
}
