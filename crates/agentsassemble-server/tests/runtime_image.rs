use agentsassemble_server::runtime_image::{RuntimeImage, RuntimePreflight, fingerprint};

#[tokio::test]
async fn actual_server_preflight_is_storage_free_and_corrupt_images_are_refused()
-> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let source = std::path::Path::new(env!("CARGO_BIN_EXE_agentsassemble-server"));
    let preflight = tokio::process::Command::new(source)
        .arg("--runtime-preflight")
        .current_dir(root.path())
        .output()
        .await?;
    assert!(preflight.status.success());
    assert_eq!(
        serde_json::from_slice::<RuntimePreflight>(&preflight.stdout)?,
        RuntimePreflight::current()
    );
    assert!(std::fs::read_dir(root.path())?.next().is_none());
    let image = RuntimeImage::prepare(source, root.path()).await?;
    assert_ne!(image.path(), source);
    assert_eq!(fingerprint(source)?, image.identity());
    assert_eq!(
        RuntimeImage::prepare(source, root.path()).await?.path(),
        image.path()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(image.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    std::fs::write(image.path(), b"corrupt image")?;
    assert!(RuntimeImage::prepare(source, root.path()).await.is_err());
    let invalid = root.path().join("invalid-executable");
    std::fs::write(&invalid, b"not an executable")?;
    assert!(RuntimeImage::prepare(&invalid, root.path()).await.is_err());
    Ok(())
}
