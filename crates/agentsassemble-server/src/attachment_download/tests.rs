use super::*;
use crate::download_budget::Budget;
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_persistence::{
    MessageAttachmentMetadata, MessageAttachmentReadAuthority, SqliteStore,
};
use futures_util::StreamExt;
use serde_json::json;
type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(crate) async fn fixture(
    size: usize,
) -> (
    SqliteStore,
    MessageAttachmentMetadata,
    AuthenticatedPrincipal,
) {
    let store = SqliteStore::open("sqlite::memory:").await.expect("store");
    store
        .bootstrap_local_authority("1468c3c6-a7f5-4311-b262-a4e492db1032", "Host")
        .await
        .expect("bootstrap");
    store
        .create_room_for_local_operator(
            "6f218ed3-8fe3-4487-86e4-3120c41fdd9e",
            "general",
            "General",
        )
        .await
        .expect("room");
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    store
        .execute_room_settings_update(
            agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&principal),
            "limit",
            &json!({"expected_revision":agentsassemble_domain::public_settings(&store.snapshot("general", 0, 100).await.expect("snapshot").settings).expect("settings").settings_revision,"attachment_file_limit_bytes":50*1024*1024}),
        )
        .await
        .expect("limit");
    let metadata = store
        .store_message_attachment(
            &principal,
            "stream.bin",
            "application/octet-stream",
            (0..size).map(|n| (n % 251) as u8).collect(),
        )
        .await
        .expect("file");
    store
        .execute_message(
            &principal,
            "bind",
            "message.send",
            &json!({"content":"", "attachment_ids":[metadata.id]}),
        )
        .await
        .expect("bind");
    (store, metadata, principal)
}

pub(crate) async fn response(
    store: &SqliteStore,
    metadata: &MessageAttachmentMetadata,
    budget: &Budget,
    connection: Option<&HttpConnectionAdmission>,
) -> axum::response::Response {
    let (reader, lease) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Local {
                room_id: "general".into(),
                user_id: LOCAL_OPERATOR_USER_ID.into(),
                participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
            },
            &metadata.id,
            |principal| budget.reserve_stream(principal),
        )
        .await
        .expect("admitted reader");
    let (body, progress) = body(reader, lease, connection);
    let mut response = axum::response::Response::new(body);
    response
        .headers_mut()
        .insert("content-length", metadata.size.into());
    response.extensions_mut().insert(progress);
    response
}

pub(crate) fn assert_piece(bytes: &[u8], offset: usize) {
    assert!(bytes.len() <= 65536);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, byte)| *byte == ((offset + i) % 251) as u8)
    );
}

#[tokio::test]
async fn ten_large_streams_share_fixed_admission_and_allow_writes_while_stalled() -> TestResult {
    let (store, metadata, principal) = fixture(50 * 1024 * 1024).await;
    let budget = Budget::default();
    let mut bodies = Vec::new();
    for _ in 0..10 {
        bodies.push(
            response(&store, &metadata, &budget, None)
                .await
                .into_body()
                .into_data_stream(),
        );
    }
    assert_eq!(budget.available(), 502 * 1024 * 1024);
    for body in &mut bodies {
        assert_piece(&body.next().await.ok_or("piece")??, 0);
    }
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.execute_message(
            &principal,
            "unrelated",
            "message.send",
            &json!({"content":"write beside stalled downloads"}),
        ),
    )
    .await??;
    for mut body in bodies {
        let mut offset = 65536;
        while let Some(bytes) = body.next().await {
            let bytes = bytes?;
            assert_piece(&bytes, offset);
            offset += bytes.len();
        }
        assert_eq!(offset, metadata.size);
    }
    assert_eq!(budget.available(), 512 * 1024 * 1024);
    Ok(())
}

#[tokio::test]
async fn unpolled_stall_expires_and_deletion_errors_release_admission() -> TestResult {
    let (store, metadata, principal) = fixture(65536 + 19).await;
    let budget = Budget::default();
    let admission = crate::http_admission::HttpAdmission::default()
        .admit()
        .ok_or("connection")?;
    let mut stream = response(&store, &metadata, &budget, Some(&admission))
        .await
        .into_body()
        .into_data_stream();
    let first = stream.next().await.ok_or("piece")??;
    assert_eq!(first.len(), 65536);
    // The socket owner runs this concurrently even if it never polls the body again.
    tokio::time::pause();
    admission.download_expired().await;
    tokio::time::resume();
    drop(stream);
    assert_eq!(
        budget.available(),
        511 * 1024 * 1024,
        "transport-owned bytes retain admission"
    );
    drop(first);
    assert_eq!(budget.available(), 512 * 1024 * 1024);
    let mut stream = response(&store, &metadata, &budget, None)
        .await
        .into_body()
        .into_data_stream();
    drop(stream.next().await.ok_or("piece")??);
    let event_id = store
        .snapshot("general", 0, 100)
        .await?
        .events
        .iter()
        .find(|event| event.event_type == "message_final")
        .ok_or("bound event")?
        .id
        .clone();
    store
        .execute_message_mutation(
            &principal,
            "delete",
            "message.delete",
            &json!({"event_id":event_id}),
        )
        .await?;
    assert!(stream.next().await.ok_or("error")?.is_err());
    assert!(stream.next().await.is_none());
    assert_eq!(budget.available(), 512 * 1024 * 1024);
    Ok(())
}
