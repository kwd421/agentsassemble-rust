use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, public_settings};
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_server::connector::client::RoomConnectorClient;
use agentsassemble_server::connector::client::files::{
    ConnectorFileContext, download_to_workspace, upload_path,
};

fn file_context(workspace: &std::path::Path) -> ConnectorFileContext {
    let canonical = workspace.canonicalize().expect("workspace canonicalizable");
    let identity = agentsassemble_domain::stable_identity_hash(
        &same_file::Handle::from_path(&canonical).expect("workspace identity"),
    );
    ConnectorFileContext {
        workspace: canonical.to_str().expect("workspace path").to_owned(),
        identity,
    }
}

fn operator() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: super::LOCAL_OPERATOR_USER_ID.into(),
        participant_id: super::LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        room_id: "general".into(),
        display_name: "Host".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

async fn set_file_limit(
    store: &agentsassemble_persistence::SqliteStore,
    request_id: &str,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let settings = store.snapshot("general", 0, 100).await?.settings;
    store
        .execute_room_settings_update(
            TrustedPrincipal(&operator()),
            request_id,
            &json!({
                "expected_revision":public_settings(&settings)?.settings_revision,
                "attachment_file_limit_bytes":limit,
            }),
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn connector_cli_path_uploads_and_saved_reads_match_room_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    set_file_limit(&store, "file-limit-50", 50 * 1024 * 1024).await?;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let client = RoomConnectorClient::new(
        &format!("{}/join?token={}", server.base_url, invite.invite_bearer),
        "CLI transfer test",
        None,
    )?;
    // An unjoined client has no advertised capability and keeps inline behaviour.
    assert!(!client.attachment_transfers().await);
    let directory = tempfile::tempdir()?;
    let files = file_context(directory.path());
    let refused = upload_path(&client, &files, "thirty.bin", "application/octet-stream")
        .await
        .expect_err("old host keeps inline behaviour");
    assert_eq!(refused.code, "workspace_upload_unavailable");
    let joined = client.join().await?;
    assert!(joined.attachment_transfers);
    assert!(client.attachment_transfers().await);

    // A 30 MiB workspace file uploads byte-exact through the transfer owner.
    let size = 30 * 1024 * 1024;
    let expected: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
    std::fs::write(directory.path().join("thirty.bin"), &expected)?;
    let uploaded = upload_path(&client, &files, "thirty.bin", "application/octet-stream").await?;
    let id = uploaded["id"]
        .as_str()
        .ok_or("pending attachment id missing")?
        .to_owned();
    assert!(id.starts_with("ma_"));
    assert_eq!(uploaded["size"], json!(expected.len()));
    // The pending ID binds through the existing say tool and counts toward quota.
    let sent = client
        .command(
            agentsassemble_protocol::RoomAction::MessageSend,
            json!({"content":"", "attachment_ids":[id]}),
        )
        .await?;
    assert_eq!(sent["result"]["event"]["attachments"][0]["id"], json!(id));

    // A large read streams into the CLI working folder byte-exact at the returned path.
    let inline = client
        .read_attachment(&id)
        .await
        .expect_err("large read leaves inline");
    assert_eq!(inline.code, "attachment_too_large");
    assert!(inline.message.contains(&size.to_string()));
    let saved = download_to_workspace(&client, &files, &id).await?;
    let path = saved.path.clone().ok_or("large read must save a path")?;
    assert!(path.contains(".agentsassemble-attachments"));
    assert_eq!(std::fs::read(&path)?, expected);

    // Small files keep the inline representation on the same endpoint.
    let small = client
        .upload_attachment("small.txt", "text/plain", "aGVsbG8=") // "hello"
        .await?;
    let small_id = small["attachment"]["id"]
        .as_str()
        .ok_or("small attachment id missing")?;
    client
        .command(
            agentsassemble_protocol::RoomAction::MessageSend,
            json!({"content":"small", "attachment_ids":[small_id]}),
        )
        .await?;
    assert_eq!(
        client.read_attachment(small_id).await?.content,
        b"hello".to_vec()
    );

    // The bound transfer bytes count toward the room storage quota: with a
    // 500 MiB quota and 30 MiB bound, nine 50 MiB reservations from three more
    // participants (three active transfers each) leave 20 MiB, so the next
    // begin refuses at the quota instead of the transfer cap.
    let settings = store.snapshot("general", 0, 100).await?.settings;
    store
        .execute_room_settings_update(
            TrustedPrincipal(&operator()),
            "quota-limit",
            &json!({
                "expected_revision":public_settings(&settings)?.settings_revision,
                "attachment_storage_limit_bytes":500 * 1024 * 1024,
            }),
        )
        .await?;
    for participant in 0..3 {
        let invite = store
            .create_connector_invite(
                &manager,
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await?;
        let reserver = RoomConnectorClient::new(
            &format!("{}/join?token={}", server.base_url, invite.invite_bearer),
            &format!("Quota reserver {participant}"),
            None,
        )?;
        reserver.join().await?;
        for reservation in 0..3 {
            reserver
                .upload_begin(&json!({
                    "filename": "reserve.bin",
                    "content_type": "application/octet-stream",
                    "declared_size": 50 * 1024 * 1024,
                    "operation_id": format!("reserve-{participant}-{reservation}"),
                }))
                .await?;
        }
    }
    let quota = client
        .upload_begin(&json!({
            "filename": "quota.bin",
            "content_type": "application/octet-stream",
            "declared_size": 50 * 1024 * 1024,
            "operation_id": Uuid::new_v4().to_string(),
        }))
        .await
        .expect_err("quota refuses at begin");
    assert_eq!(quota.code, "room_attachment_quota_reached");

    // The room limit refuses before any byte is read: begin admits nothing.
    set_file_limit(&store, "file-limit-10", 10 * 1024 * 1024).await?;
    let denied = client
        .upload_begin(&json!({
            "filename": "too-large.bin",
            "content_type": "application/octet-stream",
            "declared_size": size,
            "operation_id": Uuid::new_v4().to_string(),
        }))
        .await
        .expect_err("room limit refuses before bytes");
    assert_eq!(denied.code, "attachment_too_large");

    // Escapes, non-regular files and a named pipe are refused without blocking.
    std::fs::write(directory.path().join("inside.bin"), b"inside")?;
    let outside = tempfile::tempdir()?;
    std::fs::write(outside.path().join("outside.bin"), b"outside")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        outside.path().join("outside.bin"),
        directory.path().join("escape"),
    )?;
    #[cfg(unix)]
    {
        let created = std::process::Command::new("mkfifo")
            .arg(directory.path().join("pipe"))
            .status()?;
        assert!(created.success());
    }
    let mut blocked = vec![
        "../escape.bin".to_owned(),
        "missing.bin".to_owned(),
        outside
            .path()
            .join("outside.bin")
            .to_str()
            .ok_or("outside path")?
            .to_owned(),
        ".".to_owned(),
    ];
    #[cfg(unix)]
    blocked.extend(["escape".to_owned(), "pipe".to_owned()]);
    for path in blocked {
        let refused = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            upload_path(&client, &files, &path, "application/octet-stream"),
        )
        .await?;
        assert_eq!(
            refused.expect_err("confined path").code,
            "workspace_file_failed",
            "{path}"
        );
    }

    // A read-only connector keeps the explicit overflow error without any download.
    let observer_invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadOnly,
            chrono::Utc::now(),
        )
        .await?;
    let observer = RoomConnectorClient::new(
        &format!(
            "{}/join?token={}",
            server.base_url, observer_invite.invite_bearer
        ),
        "CLI observer",
        None,
    )?;
    observer.join().await?;
    let readonly = observer
        .upload_begin(&json!({
            "filename": "denied.bin",
            "content_type": "application/octet-stream",
            "declared_size": 1,
            "operation_id": Uuid::new_v4().to_string(),
        }))
        .await
        .expect_err("read-only upload refused");
    assert_eq!(readonly.code, "permission_denied");
    let overflow = match observer.download_attachment(&id).await {
        Ok(_) => return Err("read-only download refused".into()),
        Err(error) => error,
    };
    assert_eq!(overflow.code, "attachment_too_large");
    server.stop().await;
    Ok(())
}
