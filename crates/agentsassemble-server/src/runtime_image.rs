//! Immutable executable selection and fixed, provider-free replacement preflight.
use std::{
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

pub const PREFLIGHT_ARGUMENT: &str = "runtime-preflight";
const REEXEC_VERSION: u32 = 1;
const MAX_PREFLIGHT_BYTES: u16 = 4096;

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePreflight {
    handoff: u32,
    protocol: u32,
    schema: i64,
}

impl RuntimePreflight {
    #[must_use]
    pub const fn current() -> Self {
        Self {
            handoff: REEXEC_VERSION,
            protocol: agentsassemble_protocol::PROTOCOL_VERSION,
            schema: agentsassemble_persistence::CURRENT_SCHEMA_VERSION,
        }
    }
}

pub struct RuntimeImage {
    path: PathBuf,
    identity: String,
}

impl RuntimeImage {
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Prepares an immutable image and verifies its protocol without opening runtime storage.
    ///
    /// # Errors
    /// Rejects missing/changing/corrupt sources, failed child preflight and incompatible protocols.
    pub async fn prepare(source: &Path, state_root: &Path) -> anyhow::Result<Self> {
        let source = source.to_owned();
        let state_root = state_root.to_owned();
        let image = tokio::task::spawn_blocking(move || Self::snapshot(&source, &state_root))
            .await
            .context("join runtime image snapshot")??;
        image.preflight().await?;
        Ok(image)
    }

    fn snapshot(source: &Path, state_root: &Path) -> io::Result<Self> {
        let source = source.canonicalize()?;
        if !source.is_file() {
            return Err(io::Error::other("runtime source is not a file"));
        }
        let images = state_root.join("runtime-images");
        fs::create_dir_all(&images)?;
        agentsassemble_persistence::secure_private_directory(&images)?;
        let staging = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(&images)?;
        let staged = staging.path().join("agentsassemble-server");
        let mut source_file = File::open(&source)?;
        let mut output = File::create(&staged)?;
        io::copy(&mut source_file, &mut output)?;
        output.flush()?;
        output.sync_all()?;
        drop(output);
        let identity = fingerprint(&staged)?;
        if fingerprint(&source)? != identity {
            return Err(io::Error::other("runtime source changed during snapshot"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staged, fs::Permissions::from_mode(0o500))?;
        }
        let root = images.join(&identity);
        let path = root.join("agentsassemble-server");
        if root.exists() {
            if !root.symlink_metadata()?.is_dir()
                || !path.symlink_metadata()?.is_file()
                || fingerprint(&path)? != identity
            {
                return Err(io::Error::other("retained runtime image is corrupt"));
            }
        } else {
            fs::rename(staging.path(), &root)?;
        }
        Ok(Self { path, identity })
    }

    async fn preflight(&self) -> anyhow::Result<()> {
        let mut child = tokio::process::Command::new(&self.path)
            .arg(format!("--{PREFLIGHT_ARGUMENT}"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("launch runtime image preflight")?;
        let output = child
            .stdout
            .take()
            .context("runtime preflight stdout missing")?;
        let mut output = output.take(u64::from(MAX_PREFLIGHT_BYTES) + 1);
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            let mut bytes = Vec::new();
            output.read_to_end(&mut bytes).await?;
            if bytes.len() > usize::from(MAX_PREFLIGHT_BYTES) {
                anyhow::bail!("runtime preflight response exceeded its limit");
            }
            if !child.wait().await?.success() {
                anyhow::bail!("runtime image preflight failed");
            }
            let reported: RuntimePreflight = serde_json::from_slice(&bytes)
                .context("runtime image preflight response invalid")?;
            if reported != RuntimePreflight::current() {
                anyhow::bail!("runtime image protocol or schema is incompatible");
            }
            Ok(())
        })
        .await;
        let result = result.unwrap_or_else(|_| {
            Err(anyhow::anyhow!(
                "runtime image preflight exceeded its deadline"
            ))
        });
        if result.is_err() && child.try_wait()?.is_none() {
            child
                .kill()
                .await
                .context("join failed runtime preflight")?;
        }
        result
    }
}

/// Reads the content identity used by the durable restart operation.
///
/// # Errors
/// Returns file/read errors; no partial identity is published.
pub fn fingerprint(path: &Path) -> io::Result<String> {
    let mut source = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
