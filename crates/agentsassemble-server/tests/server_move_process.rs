//! Native process + real loopback HTTP boundary; central responses are a fixture.
use agentsassemble_persistence::{
    ServerOwnerAuthority, SqliteStore,
    backup_codec::{self, BackupManifest, KdfParameters},
    backup_database,
};
use anyhow::{Context, ensure};
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::StatusCode,
    response::IntoResponse,
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Clone)]
struct CentralFixture {
    issuer: String,
    server_id: String,
    receipt: Arc<parking_lot::Mutex<Option<Value>>>,
}
async fn central(
    State(state): State<CentralFixture>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    if uri.path().ends_with("/restore") {
        let result = verify_host(&state, &body);
        assert!(
            result.is_ok(),
            "native host proof must verify at the HTTP boundary"
        );
        let fingerprint = result.unwrap_or_default();
        *state.receipt.lock() = Some(
            json!({"status":"committed", "operation_id":body["operation_id"],
            "expected_epoch":body["expected_epoch"], "new_key_fingerprint":fingerprint,
            "registration_epoch":"restored-epoch", "endpoint_generation":4_000_000_000_000_i64}),
        );
        // The mutation happened, but its response was lost. The client must not
        // invent a new operation, activate early, or retry with another key.
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"lost response"})),
        )
    } else {
        assert_eq!(body["expected_epoch"], "current-epoch");
        let receipt = state.receipt.lock().clone().unwrap_or(Value::Null);
        assert!(
            uri.path()
                .ends_with(receipt["operation_id"].as_str().unwrap_or("missing"))
        );
        (StatusCode::OK, Json(receipt))
    }
}
fn verify_host(state: &CentralFixture, body: &Value) -> anyhow::Result<String> {
    let key = &body["new_public_key_jwk"];
    let public = URL_SAFE_NO_PAD.decode(key["x"].as_str().context("key")?)?;
    let canonical_key: std::collections::BTreeMap<String, Value> =
        serde_json::from_value(key.clone())?;
    let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(serde_json::to_vec(&canonical_key)?));
    let proof = &body["host_proof"];
    let canonical = json!({"purpose":"server_restore_v1", "server_id":state.server_id,
        "issuer":state.issuer, "owner_person_id":proof["owner_person_id"],
        "expected_epoch":body["expected_epoch"], "new_key_fingerprint":fingerprint,
        "operation_id":body["operation_id"], "issued_at":proof["issued_at"], "nonce":proof["nonce"]});
    let canonical: std::collections::BTreeMap<String, Value> = serde_json::from_value(canonical)?;
    let signature = URL_SAFE_NO_PAD.decode(proof["signature"].as_str().context("signature")?)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public)
        .verify(&serde_json::to_vec(&canonical)?, &signature)
        .map_err(|_| anyhow::anyhow!("proof"))?;
    Ok(fingerprint)
}
async fn native(target: &Path, issuer: &str, request: Value) -> anyhow::Result<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
        .arg("--server-restore")
        .arg(target)
        .env("AGENTSASSEMBLE_CENTRAL_URL", issuer)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut input = child.stdin.take().context("stdin")?;
    input.write_all(&serde_json::to_vec(&request)?).await?;
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(40), child.wait_with_output()).await??;
    ensure!(output.status.success(), "native command process failed");
    ensure!(
        output.stderr.is_empty(),
        "native control must not log private input"
    );
    Ok(serde_json::from_slice(&output.stdout)?)
}
async fn backup(
    source: &SqliteStore,
    root: &Path,
    issuer: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let payload = root.join("payload.sqlite");
    source
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &root.join("snapshot.sqlite"),
            &payload,
        )
        .await?;
    let (rooms, messages, assets) = backup_database::payload_counts(&payload).await?;
    let (database_bytes, database_sha256) =
        backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
    let manifest = BackupManifest {
        schema_version: agentsassemble_persistence::CURRENT_SCHEMA_VERSION,
        server_id: source.host_identity().await?.server_id().into(),
        server_name: "Move process".into(),
        issuer: issuer.into(),
        owner_person_id: "owner".into(),
        source_epoch: "source-epoch".into(),
        source_key_fingerprint: "s".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let file = root.join("encrypted.aaserver");
    backup_codec::encode(
        &mut std::fs::File::open(payload)?,
        &mut std::fs::File::create(&file)?,
        &manifest,
        Some(b"fixture passphrase"),
        KdfParameters::default(),
    )?;
    Ok(file)
}
#[tokio::test]
async fn encrypted_move_recovers_lost_switch_response_across_native_processes() -> anyhow::Result<()>
{
    let source_root = tempfile::tempdir()?;
    let source = SqliteStore::open_path(&source_root.path().join("source.sqlite")).await?;
    let (message_id, attachment_id) = seed_product(&source).await?;
    let old = source.host_identity().await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let issuer = format!("http://{}", listener.local_addr()?);
    let fixture = CentralFixture {
        issuer: issuer.clone(),
        server_id: old.server_id().into(),
        receipt: Arc::default(),
    };
    let stop = tokio_util::sync::CancellationToken::new();
    let stopped = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(post(central)).with_state(fixture),
        )
        .with_graceful_shutdown(stopped.cancelled_owned())
        .await
    });
    let file = backup(&source, source_root.path(), &issuer).await?;
    let target_root = tempfile::tempdir()?;
    let target = target_root.path().join("runtime.sqlite3");
    let prepared = native(
        &target,
        &issuer,
        json!({"action":"prepare","file":file,"passphrase":"fixture passphrase"}),
    )
    .await?;
    assert_eq!(prepared["ok"], true);
    let operation = prepared["result"]["operation_id"].clone();
    assert!(operation.is_string());
    assert!(!target.exists());
    let bound = native(
        &target,
        &issuer,
        json!({"action":"bind","operation_id":operation,"expected_epoch":"current-epoch"}),
    )
    .await?;
    assert_eq!(bound["ok"], true);
    let draft=native(&target,&issuer,json!({"action":"draft","operation_id":operation,"owner_person_id":"owner","proof":"p".repeat(43)})).await?;
    assert_eq!(draft["ok"], true);
    let headers = json!({"content-type":"application/json","authorization":"Bearer fixture","x-aa-admission-protocol":"secure_admission_v1","x-aa-device-id":"fixture","x-aa-timestamp":"1","x-aa-nonce":"fixture","x-aa-signature":"fixture"});
    let switched=native(&target,&issuer,json!({"action":"switch","operation_id":operation,"body":draft["result"]["body"],"headers":headers})).await?;
    assert_eq!(switched["ok"], false);
    assert!(!target.exists());
    let status = native(&target, &issuer, json!({"action":"inspect"})).await?;
    assert_eq!(status["result"]["phase"], "switch_unknown");
    let recovered = native(
        &target,
        &issuer,
        json!({"action":"status","operation_id":operation,"headers":headers}),
    )
    .await?;
    assert_eq!(recovered["ok"], true);
    assert_eq!(recovered["result"]["status"], "committed");
    let restored = SqliteStore::open_path(&target).await?;
    assert!(restored.room_exists("general").await?);
    assert!(
        restored
            .snapshot("general", 0, 100)
            .await?
            .events
            .iter()
            .any(|event| event.id == message_id
                && event.content.as_deref() == Some("Move with attachment"))
    );
    let attachment = restored
        .bound_message_attachment(
            "general",
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            &attachment_id,
        )
        .await?;
    assert_eq!(attachment.content, vec![0x5a; 10 * 1024 * 1024]);

    assert_eq!(restored.host_identity().await?.server_id(), old.server_id());
    assert_ne!(
        restored.host_identity().await?.private_key_pkcs8(),
        old.private_key_pkcs8()
    );
    assert_eq!(
        restored.registration_epoch().await?.as_deref(),
        Some("restored-epoch")
    );
    assert!(restored.next_central_endpoint_generation().await? > 4_000_000_000_000);
    restored.close().await?;
    source.close().await?;
    stop.cancel();
    server.await??;
    Ok(())
}

async fn seed_product(source: &SqliteStore) -> anyhow::Result<(String, String)> {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    };
    source
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Move owner")
        .await?;
    source
        .create_room_for_local_operator(
            &uuid::Uuid::new_v4().to_string(),
            "general",
            "Preserved room",
        )
        .await?;
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
        display_name: "Owner".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let attachment = source
        .store_message_attachment(
            &principal,
            "retained.bin",
            "application/octet-stream",
            vec![0x5a; 10 * 1024 * 1024],
        )
        .await?;
    let message = source
        .execute_message(
            &principal,
            &uuid::Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Move with attachment","attachment_ids":[attachment.id]}),
        )
        .await?;
    Ok((message.event.id, attachment.id))
}
