use std::path::Path;

use super::{arguments, state_home};
use crate::test_support::durable_session;

#[tokio::test]
async fn launch_profile_and_private_state_are_exact_and_session_scoped() {
    let mut session = durable_session(
        "room",
        "session",
        "Grok",
        "grok_live_session",
        "grok-4.6",
        "acp_stdio",
    );
    session.public.permission_mode = "workspace_write".to_owned();
    assert_eq!(
        arguments(&session).unwrap_or_else(|_| panic!("build Grok arguments")),
        [
            "--permission-mode",
            "acceptEdits",
            "agent",
            "--model",
            "grok-4.6",
            "--reasoning-effort",
            "high",
            "stdio",
        ]
    );

    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create state root: {error}"));
    let first = state_home(root.path(), &session)
        .await
        .unwrap_or_else(|_| panic!("create Grok state home"));
    assert_eq!(
        state_home(root.path(), &session)
            .await
            .unwrap_or_else(|_| panic!("reuse Grok state home")),
        first
    );
    assert_private_directory(&first);

    session.runtime_profile_key = "next-profile".to_owned();
    let next = state_home(root.path(), &session)
        .await
        .unwrap_or_else(|_| panic!("create replacement profile state home"));
    assert_ne!(next, first);
    assert_private_directory(&next);
}

fn assert_private_directory(path: &Path) {
    let metadata = std::fs::symlink_metadata(path)
        .unwrap_or_else(|error| panic!("inspect Grok state home: {error}"));
    assert!(metadata.is_dir());
    assert!(!metadata.file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(metadata.permissions().mode() & 0o077, 0);
    }
}
