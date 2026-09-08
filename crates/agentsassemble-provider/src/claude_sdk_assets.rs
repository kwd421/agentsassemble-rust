pub(crate) const RUNTIME_ENV: &str = "AGENTSASSEMBLE_PROVIDER_RUNTIME";

use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const BRIDGE_NAME: &str = "claude-agent-sdk-bridge.mjs";
const DELIVERY_NAME: &str = "claude-native-delivery.mjs";
const SDK_RELATIVE_PATH: &str = "node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs";
const MAX_BRIDGE_BYTES: u64 = 512 * 1024;
const MAX_SDK_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) struct PrivateClaudeSdkBundle {
    _directory: tempfile::TempDir,
    pub(crate) bridge: PathBuf,
    pub(crate) sdk: PathBuf,
}

impl PrivateClaudeSdkBundle {
    pub(crate) async fn stage() -> io::Result<Self> {
        let root = source_root();
        tokio::task::spawn_blocking(move || Self::stage_from(&root))
            .await
            .map_err(io::Error::other)?
    }

    fn stage_from(root: &Path) -> io::Result<Self> {
        let directory = tempfile::Builder::new()
            .prefix("agentsassemble-claude-sdk-")
            .tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        let bridge = copy_regular_file(
            &root.join(BRIDGE_NAME),
            &directory.path().join(BRIDGE_NAME),
            MAX_BRIDGE_BYTES,
        )?;
        copy_regular_file(
            &root.join(DELIVERY_NAME),
            &directory.path().join(DELIVERY_NAME),
            MAX_BRIDGE_BYTES,
        )?;
        let sdk = copy_regular_file(
            &root.join(SDK_RELATIVE_PATH),
            &directory.path().join("claude-agent-sdk.mjs"),
            MAX_SDK_BYTES,
        )?;
        Ok(Self {
            _directory: directory,
            bridge,
            sdk,
        })
    }
}

fn source_root() -> PathBuf {
    std::env::var_os(RUNTIME_ENV).map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../provider-runtime"),
        PathBuf::from,
    )
}

fn copy_regular_file(source: &Path, destination: &Path, limit: u64) -> io::Result<PathBuf> {
    let metadata = std::fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(io::Error::other("Claude SDK resource is invalid"));
    }
    let source_file = File::open(source)?;
    let mut expected = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    source_file.take(limit + 1).read_to_end(&mut expected)?;
    if expected.len() as u64 != metadata.len() {
        return Err(io::Error::other(
            "Claude SDK resource changed while reading",
        ));
    }
    let mut destination_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(destination)?;
    destination_file.write_all(&expected)?;
    destination_file.sync_all()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(destination, std::fs::Permissions::from_mode(0o400))?;
    }
    let actual = std::fs::read(destination)?;
    if actual != expected {
        return Err(io::Error::other("Claude SDK resource copy was not exact"));
    }
    Ok(destination.to_path_buf())
}

#[cfg(test)]
#[path = "claude_sdk_assets_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
pub(crate) fn fixture_bundle() -> PrivateClaudeSdkBundle {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("SDK fixture: {error}"));
    let sdk = root.path().join(SDK_RELATIVE_PATH);
    std::fs::create_dir_all(sdk.parent().unwrap_or_else(|| panic!("SDK parent")))
        .unwrap_or_else(|error| panic!("SDK directory: {error}"));
    for path in [
        root.path().join(BRIDGE_NAME),
        root.path().join(DELIVERY_NAME),
        sdk,
    ] {
        std::fs::write(path, "// unused fixture resource\n")
            .unwrap_or_else(|error| panic!("SDK resource: {error}"));
    }
    PrivateClaudeSdkBundle::stage_from(root.path())
        .unwrap_or_else(|error| panic!("stage SDK fixture: {error}"))
}
