use std::path::Path;

use tokio_util::sync::CancellationToken;

use crate::{catalog::provider_executable, process::probe, provider_update::ProviderUpdateError};

type Error = ProviderUpdateError;

pub(crate) enum UpdateMethod {
    Native {
        arguments: &'static [&'static str],
        version_argument: bool,
    },
    GlobalNpm {
        package: &'static str,
        launcher: &'static str,
        entry: &'static str,
    },
}

pub(crate) struct UpdateCommand {
    pub(crate) executable: String,
    pub(crate) arguments: Vec<String>,
}

impl UpdateMethod {
    pub(crate) async fn command(
        &self,
        executable: &str,
        version: &str,
        cancellation: &CancellationToken,
    ) -> Result<UpdateCommand, Error> {
        match self {
            Self::Native {
                arguments,
                version_argument,
            } => {
                let mut arguments = arguments
                    .iter()
                    .map(|arg| (*arg).to_owned())
                    .collect::<Vec<_>>();
                if *version_argument {
                    arguments.push(version.to_owned());
                }
                Ok(UpdateCommand {
                    executable: executable.to_owned(),
                    arguments,
                })
            }
            Self::GlobalNpm {
                package,
                launcher,
                entry,
            } => {
                let (npm, _) = provider_executable("npm", cancellation)
                    .await
                    .map_err(package_error)?;
                npm_command(&npm, package, launcher, entry, version, cancellation).await
            }
        }
    }
}

async fn npm_command(
    npm: &str,
    package: &str,
    launcher: &str,
    entry: &str,
    version: &str,
    cancellation: &CancellationToken,
) -> Result<UpdateCommand, Error> {
    let prefix = probe(npm, &["prefix", "--global"], cancellation, &[]).await?;
    let prefix = prefix.trim();
    if !Path::new(prefix).is_absolute()
        || prefix.len() > 4096
        || prefix.contains(['\n', '\r', '\0'])
    {
        return Err(Error::InvalidResponse);
    }
    let (installed, _) = provider_executable(launcher, cancellation)
        .await
        .map_err(package_error)?;
    let expected = npm_entry(Path::new(prefix), package, entry);
    let expected = expected.to_str().ok_or(Error::Unsupported)?;
    let (expected, _) = provider_executable(expected, cancellation)
        .await
        .map_err(package_error)?;
    if installed != expected {
        return Err(Error::Unsupported);
    }
    Ok(UpdateCommand {
        executable: npm.to_owned(),
        arguments: vec![
            "install".into(),
            "--global".into(),
            "--prefix".into(),
            prefix.into(),
            format!("{package}@{version}"),
        ],
    })
}

fn npm_entry(prefix: &Path, package: &str, entry: &str) -> std::path::PathBuf {
    #[cfg(windows)]
    let root = prefix.join("node_modules");
    #[cfg(not(windows))]
    let root = prefix.join("lib").join("node_modules");
    root.join(package).join(entry)
}

fn package_error(error: crate::process::ProbeFailure) -> Error {
    if error == crate::process::ProbeFailure::Missing {
        Error::Unsupported
    } else {
        Error::from(error)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn npm_update_targets_only_the_installed_prefix_and_pins_the_offer()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let entry = npm_entry(root.path(), "@openai/codex", "bin/codex.js");
        std::fs::create_dir_all(entry.parent().ok_or("fixture parent")?)?;
        let npm = root.path().join("npm-fixture");
        let other = root.path().join("other-codex");
        let prefix = root.path().to_str().ok_or("fixture prefix")?;
        std::fs::write(
            &npm,
            format!(
                "#!/bin/sh\n[ \"$*\" = 'prefix --global' ] || exit 3\nprintf '%s\\n' {}\n",
                shlex::try_quote(prefix)?
            ),
        )?;
        for file in [&entry, &other] {
            std::fs::write(file, "#!/bin/sh\nexit 0\n")?;
        }
        for file in [&entry, &other, &npm] {
            std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o700))?;
        }
        let cancellation = CancellationToken::new();
        let npm = npm.to_str().ok_or("fixture npm")?;
        let command = npm_command(
            npm,
            "@openai/codex",
            entry.to_str().ok_or("fixture entry")?,
            "bin/codex.js",
            "1.2.3",
            &cancellation,
        )
        .await?;
        assert_eq!(command.executable, npm);
        assert_eq!(
            command.arguments,
            [
                "install",
                "--global",
                "--prefix",
                prefix,
                "@openai/codex@1.2.3"
            ]
        );
        assert!(matches!(
            npm_command(
                npm,
                "@openai/codex",
                other.to_str().ok_or("fixture other")?,
                "bin/codex.js",
                "1.2.3",
                &cancellation
            )
            .await,
            Err(Error::Unsupported)
        ));
        Ok(())
    }
}
