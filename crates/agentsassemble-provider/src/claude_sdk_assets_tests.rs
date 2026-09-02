#[test]
fn stages_exact_bounded_regular_resources() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create source: {error}"));
    let sdk = root.path().join(super::SDK_RELATIVE_PATH);
    std::fs::create_dir_all(sdk.parent().unwrap_or_else(|| panic!("SDK parent")))
        .unwrap_or_else(|error| panic!("create SDK parent: {error}"));
    std::fs::write(root.path().join(super::BRIDGE_NAME), b"bridge")
        .unwrap_or_else(|error| panic!("write bridge: {error}"));
    std::fs::write(&sdk, b"sdk").unwrap_or_else(|error| panic!("write SDK: {error}"));

    let staged = super::PrivateClaudeSdkBundle::stage_from(root.path())
        .unwrap_or_else(|error| panic!("stage SDK bundle: {error}"));
    assert_eq!(std::fs::read(staged.bridge).unwrap_or_default(), b"bridge");
    assert_eq!(std::fs::read(staged.sdk).unwrap_or_default(), b"sdk");
}
