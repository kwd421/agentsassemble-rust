use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

fn executable(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap_or_else(|error| panic!("write executable: {error}"));
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("set executable mode: {error}"));
}

fn package(root: &Path) -> PathBuf {
    let entry = root.join("cursor-agent");
    executable(&entry, b"#!/bin/sh\nSCRIPT_DIR=$(dirname \"$0\")\nexec \"$SCRIPT_DIR/node\" \"$SCRIPT_DIR/index.js\"\n");
    executable(&root.join("node"), b"#!/bin/sh\n/bin/cat \"$1\"\n");
    fs::write(root.join("index.js"), "verified package output")
        .unwrap_or_else(|error| panic!("write entry: {error}"));
    fs::create_dir(root.join("node_modules"))
        .unwrap_or_else(|error| panic!("create dependencies: {error}"));
    fs::write(root.join("node_modules/dependency.js"), "dependency-v1")
        .unwrap_or_else(|error| panic!("write dependency: {error}"));
    entry
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize entry: {error}"))
}

#[tokio::test]
async fn staged_package_launch_preserves_siblings_and_cleanup() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
    let entry = package(root.path());
    let identity = super::cursor_executable_identity(entry.to_string_lossy().into_owned())
        .await
        .unwrap_or_else(|error| panic!("identify package: {error:?}"));
    let bound = super::bind_cursor_executable(entry.to_string_lossy().into_owned(), identity)
        .await
        .unwrap_or_else(|error| panic!("bind package: {error:?}"));
    fs::write(root.path().join("index.js"), "unverified replacement")
        .unwrap_or_else(|error| panic!("replace original: {error}"));
    let output = std::process::Command::new(bound.launch_path())
        .output()
        .unwrap_or_else(|error| panic!("launch package: {error}"));
    assert!(output.status.success());
    assert_eq!(output.stdout, b"verified package output");
    assert!(bound.allows_child_processes());
    assert!(!bound.requires_inherited_executable_fd());
    let staged = PathBuf::from(bound.launch_path());
    assert_ne!(staged, entry);
    assert_eq!(
        fs::read(
            staged
                .parent()
                .unwrap_or_else(|| panic!("staged parent"))
                .join("node_modules/dependency.js")
        )
        .unwrap_or_else(|error| panic!("read staged dependency: {error}")),
        b"dependency-v1"
    );
    drop(bound);
    assert!(!staged.exists());
}

#[tokio::test]
async fn changed_removed_or_linked_dependency_rejects_old_authority() {
    for change in ["changed", "removed", "linked"] {
        let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
        let entry = package(root.path());
        let identity = super::cursor_executable_identity(entry.to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("identify package: {error:?}"));
        let dependency = root.path().join("node_modules/dependency.js");
        if change == "changed" {
            fs::write(&dependency, "dependency-v2")
                .unwrap_or_else(|error| panic!("change dependency: {error}"));
        } else {
            fs::remove_file(&dependency)
                .unwrap_or_else(|error| panic!("remove dependency: {error}"));
            if change == "linked" {
                std::os::unix::fs::symlink(root.path().join("index.js"), &dependency)
                    .unwrap_or_else(|error| panic!("link dependency: {error}"));
            }
        }
        assert!(
            super::bind_cursor_executable(entry.to_string_lossy().into_owned(), identity)
                .await
                .is_err(),
            "accepted {change} dependency"
        );
    }
}

#[test]
fn missing_node_and_oversized_package_are_not_native_executables() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
    let entry = package(root.path());
    fs::remove_file(root.path().join("node"))
        .unwrap_or_else(|error| panic!("remove node: {error}"));
    assert!(super::identity(&entry).is_err());
    executable(&root.path().join("node"), b"#!/bin/sh\n");
    let oversized = fs::File::create(root.path().join("oversized"))
        .unwrap_or_else(|error| panic!("create oversized member: {error}"));
    oversized
        .set_len(512 * 1024 * 1024 + 1)
        .unwrap_or_else(|error| panic!("set oversized length: {error}"));
    assert!(super::identity(&entry).is_err());
}
