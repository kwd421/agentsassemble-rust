//! One-shot native restore owner. No runtime, listener or provider is constructed.
use crate::CentralHostIdentity;
use agentsassemble_persistence::{
    backup_codec,
    restore_activation::RestoreReceipt,
    restore_job::{RestoreJob, RestorePhase},
};
use anyhow::{Context, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Inspect,
    Probe {
        file: PathBuf,
    },
    Prepare {
        file: PathBuf,
        passphrase: Option<String>,
    },
    Bind {
        operation_id: uuid::Uuid,
        expected_epoch: String,
    },
    Draft {
        operation_id: uuid::Uuid,
        owner_person_id: String,
        proof: String,
    },
    Switch {
        operation_id: uuid::Uuid,
        body: String,
        headers: BTreeMap<String, String>,
    },
    StatusDraft {
        operation_id: uuid::Uuid,
    },
    Status {
        operation_id: uuid::Uuid,
        headers: BTreeMap<String, String>,
    },
    Activate {
        operation_id: uuid::Uuid,
    },
    Cancel {
        operation_id: uuid::Uuid,
    },
}

/// Bounded private stdin protocol for the bundled native app.
/// # Errors
/// Fails explicitly without exposing request credentials or untrusted payloads.
pub async fn run(target: &Path) -> anyhow::Result<()> {
    let mut bytes = Zeroizing::new(Vec::new());
    std::io::stdin().take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16384, "restore request too large");
    let request: Request = serde_json::from_slice(&bytes).context("invalid restore request")?;
    let result = execute(target, request).await;
    let output = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        // Neither provider content nor credential-bearing request/response bytes
        // are emitted by the private control protocol.
        Err(error) => json!({"ok":false,"error":error_code(&error)}),
    };
    let encoded = serde_json::to_vec(&output)?;
    ensure!(encoded.len() <= 32768, "restore response too large");
    std::io::stdout().write_all(&encoded)?;
    Ok(())
}

fn error_code(error: &anyhow::Error) -> &'static str {
    use agentsassemble_persistence::PersistenceError;
    use backup_codec::BackupError;
    if let Some(error) = error.downcast_ref::<PersistenceError>() {
        return match error {
            PersistenceError::InitializationNotAllowed => "restore_target_not_empty",
            PersistenceError::RestorePending => "restore_pending",
            PersistenceError::InvalidBackup => "backup_invalid",
            _ => "server_restore_failed",
        };
    }
    match error.downcast_ref::<BackupError>() {
        Some(BackupError::Authentication) => "backup_authentication_failed",
        Some(BackupError::Invalid | BackupError::Manifest) => "backup_invalid",
        Some(BackupError::Io(_)) => "backup_io_failed",
        _ => "server_restore_failed",
    }
}

async fn execute(target: &Path, request: Request) -> anyhow::Result<Value> {
    match request {
        Request::Inspect => Ok(serde_json::to_value(RestoreJob::inspect(target)?)?),
        Request::Probe { file } => Ok(
            json!({"encrypted": backup_codec::encryption_required(&mut std::fs::File::open(file)?)?}),
        ),
        Request::Prepare { file, passphrase } => prepare(target, file, passphrase).await,
        Request::Bind {
            operation_id,
            expected_epoch,
        } => {
            let mut job = RestoreJob::resume(target, operation_id)?;
            let identity = identity(&job).await?;
            job.bind_switch(&expected_epoch, identity.fingerprint())?;
            // Match Worker canonicalJson even when serde_json/preserve_order is enabled.
            let tuple: BTreeMap<String, Value> = serde_json::from_value(tuple(&job)?)?;
            let request_id = URL_SAFE_NO_PAD.encode(Sha256::digest(serde_json::to_vec(&tuple)?));
            Ok(json!({"journal":job.journal(),"request_id":request_id,
                "path":path(&job,"restore-proof")?,"body":base(&job,&identity)?}))
        }
        Request::Draft {
            operation_id,
            owner_person_id,
            proof,
        } => {
            let job = RestoreJob::resume(target, operation_id)?;
            ensure!(
                matches!(
                    job.journal().phase,
                    RestorePhase::Prepared | RestorePhase::SwitchUnknown
                ),
                "invalid phase"
            );
            ensure!(
                !owner_person_id.is_empty() && owner_person_id.len() <= 256 && proof.len() == 43,
                "invalid proof"
            );
            let identity = identity(&job).await?;
            let issued_at = chrono::Utc::now().timestamp();
            let nonce = URL_SAFE_NO_PAD.encode(uuid::Uuid::new_v4().as_bytes());
            let canonical = host_canonical(&job, &owner_person_id, issued_at, &nonce)?;
            let mut body = base(&job, &identity)?;
            body["host_proof"] = json!({"issued_at":issued_at,"nonce":nonce,
                "signature":identity.sign_restore(&canonical),"owner_person_id":owner_person_id});
            body["proof"] = json!(proof);
            Ok(json!({"path":path(&job,"restore")?,"body":serde_json::to_string(&body)?}))
        }
        Request::Switch {
            operation_id,
            body,
            headers,
        } => {
            let body = Zeroizing::new(body);
            let mut job = RestoreJob::resume(target, operation_id)?;
            validate_switch(&job, &body).await?;
            let first_attempt = job.journal().phase == RestorePhase::Prepared;
            job.mark_switch_unknown()?;
            let response = send(&path(&job, "restore")?, Some(&body), headers).await?;
            record_disabled_request(&mut job, first_attempt, &response)?;
            accept_receipt(&mut job, &response).await?;
            Ok(response)
        }
        Request::StatusDraft { operation_id } => {
            let job = RestoreJob::resume(target, operation_id)?;
            Ok(
                json!({"path":path(&job,&format!("restore-operations/{operation_id}"))?,"body":status_body(&job)?}),
            )
        }
        Request::Status {
            operation_id,
            headers,
        } => recover_status(target, operation_id, headers).await,
        Request::Activate { operation_id } => {
            let mut job = RestoreJob::resume(target, operation_id)?;
            job.activate().await?;
            Ok(serde_json::to_value(job.journal())?)
        }
        Request::Cancel { operation_id } => {
            RestoreJob::resume(target, operation_id)?.cancel()?;
            Ok(Value::Null)
        }
    }
}

async fn recover_status(
    target: &Path,
    operation_id: uuid::Uuid,
    headers: std::collections::BTreeMap<String, String>,
) -> anyhow::Result<Value> {
    let mut job = RestoreJob::resume(target, operation_id)?;
    let response = send(
        &path(&job, &format!("restore-operations/{operation_id}"))?,
        Some(&status_body(&job)?),
        headers,
    )
    .await?;
    if response.get("status").and_then(Value::as_str) == Some("not_committed") {
        job.record_not_committed(
            response["operation_id"]
                .as_str()
                .context("missing operation")?,
            response["expected_epoch"]
                .as_str()
                .context("missing epoch")?,
        )?;
    } else {
        accept_receipt(&mut job, &response).await?;
    }
    Ok(response)
}

async fn prepare(
    target: &Path,
    file: PathBuf,
    passphrase: Option<String>,
) -> anyhow::Result<Value> {
    let passphrase = passphrase.map(Zeroizing::new);
    let mut job = RestoreJob::begin(target)?;
    let result = async {
        let input = std::fs::File::open(file)?;
        ensure!(input.metadata()?.is_file(), "backup must be a file");
        let decoded = job.stage().join("decoded.sqlite");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&decoded)?;
        let manifest = backup_codec::decode_with_preflight(
            &mut std::io::BufReader::new(input),
            &mut output,
            passphrase.as_ref().map(|value| value.as_bytes()),
            |manifest| {
                job.check_decode_space(manifest)
                    .map_err(backup_codec::BackupError::Io)
            },
        )?;
        output.sync_all()?;
        drop(output);
        let issuer = central()?;
        ensure!(
            manifest.issuer == issuer.origin().ascii_serialization(),
            "backup central mismatch"
        );
        job.prepare(&decoded, manifest).await?;
        Ok::<_, anyhow::Error>(serde_json::to_value(job.journal())?)
    }
    .await;
    if result.is_err() {
        job.cancel()?;
    }
    result
}

fn central() -> anyhow::Result<url::Url> {
    Ok(crate::central::directory::normalize_base_url(
        &std::env::var("AGENTSASSEMBLE_CENTRAL_URL")?,
    )?)
}
fn status_body(job: &RestoreJob) -> anyhow::Result<String> {
    ensure!(
        job.journal().phase == RestorePhase::SwitchUnknown,
        "invalid status phase"
    );
    Ok(serde_json::to_string(
        &json!({"expected_epoch":job.journal().expected_epoch}),
    )?)
}
async fn identity(job: &RestoreJob) -> anyhow::Result<CentralHostIdentity> {
    Ok(CentralHostIdentity::from_persistent(
        &job.host_identity().await?,
    )?)
}
fn tuple(job: &RestoreJob) -> anyhow::Result<Value> {
    let journal = job.journal();
    let manifest = journal.manifest.as_ref().context("manifest missing")?;
    ensure!(
        !journal.expected_epoch.is_empty() && !journal.new_key_fingerprint.is_empty(),
        "unbound job"
    );
    Ok(
        json!({"purpose":"server_restore_v1","server_id":manifest.server_id,
        "expected_epoch":journal.expected_epoch,"new_key_fingerprint":journal.new_key_fingerprint,
        "operation_id":journal.operation_id.to_string()}),
    )
}
fn base(job: &RestoreJob, identity: &CentralHostIdentity) -> anyhow::Result<Value> {
    ensure!(
        identity.fingerprint() == job.journal().new_key_fingerprint,
        "key changed"
    );
    Ok(
        json!({"operation_id":job.journal().operation_id.to_string(),
        "expected_epoch":job.journal().expected_epoch,"new_public_key_jwk":identity.restore_key()?}),
    )
}
fn path(job: &RestoreJob, suffix: &str) -> anyhow::Result<String> {
    let id = &job
        .journal()
        .manifest
        .as_ref()
        .context("manifest missing")?
        .server_id;
    uuid::Uuid::parse_str(id)?;
    Ok(format!("/v1/servers/{id}/{suffix}"))
}
fn host_canonical(
    job: &RestoreJob,
    owner: &str,
    issued_at: i64,
    nonce: &str,
) -> anyhow::Result<Vec<u8>> {
    let mut value = tuple(job)?;
    value["issuer"] = json!(central()?.origin().ascii_serialization());
    value["owner_person_id"] = json!(owner);
    value["issued_at"] = json!(issued_at);
    value["nonce"] = json!(nonce);
    // ACP enables serde_json/preserve_order transitively. Worker canonicalJson
    // sorts keys; a Value's insertion order must never define this signed wire form.
    let canonical: BTreeMap<String, Value> = serde_json::from_value(value)?;
    Ok(serde_json::to_vec(&canonical)?)
}
async fn validate_switch(job: &RestoreJob, bytes: &str) -> anyhow::Result<()> {
    let body: Value = serde_json::from_str(bytes)?;
    let identity = identity(job).await?;
    let mut actual = body.clone();
    let proof = actual
        .as_object_mut()
        .context("invalid body")?
        .remove("host_proof")
        .context("missing proof")?;
    let step_up = actual
        .as_object_mut()
        .context("invalid body")?
        .remove("proof")
        .context("missing step up")?;
    ensure!(
        step_up.as_str().is_some_and(|s| s.len() == 43) && actual == base(job, &identity)?,
        "changed body"
    );
    let issued = proof["issued_at"].as_i64().context("invalid time")?;
    ensure!(
        chrono::Utc::now().timestamp().abs_diff(issued) <= 300,
        "expired draft"
    );
    let canonical = host_canonical(
        job,
        proof["owner_person_id"].as_str().context("missing owner")?,
        issued,
        proof["nonce"].as_str().context("missing nonce")?,
    )?;
    ensure!(
        identity.verify_restore(
            &canonical,
            proof["signature"].as_str().context("missing signature")?
        ),
        "changed host proof"
    );
    Ok(())
}
async fn accept_receipt(job: &mut RestoreJob, response: &Value) -> anyhow::Result<()> {
    if response.get("status").and_then(Value::as_str) == Some("committed") {
        job.record_commit(&serde_json::from_value::<RestoreReceipt>(response.clone())?)?;
        job.activate().await?;
    }
    Ok(())
}
const ALLOWED: [&str; 7] = [
    "content-type",
    "authorization",
    "x-aa-admission-protocol",
    "x-aa-device-id",
    "x-aa-timestamp",
    "x-aa-nonce",
    "x-aa-signature",
];

async fn send(
    path: &str,
    body: Option<&str>,
    headers: BTreeMap<String, String>,
) -> anyhow::Result<Value> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?;
    let url = central()?.join(path)?;
    let mut request = if body.is_some() {
        client.post(url)
    } else {
        client.get(url)
    };
    ensure!(headers.len() == ALLOWED.len(), "invalid device headers");
    for (name, value) in headers {
        ensure!(
            ALLOWED.contains(&name.as_str()) && value.len() <= 4096,
            "invalid device header"
        );
        request = request.header(name, value);
    }
    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .body(body.to_owned());
    }
    let mut response = request.send().await?;
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= 32768,
            "central response too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    decode_response(response.status(), &bytes)
}

fn record_disabled_request(
    job: &mut RestoreJob,
    first_attempt: bool,
    response: &Value,
) -> anyhow::Result<()> {
    // A rejected retry cannot disprove a previous request's uncertain outcome.
    if first_attempt && disabled_request(response) {
        let epoch = job.journal().expected_epoch.clone();
        let operation = job.journal().operation_id.to_string();
        job.record_not_committed(&operation, &epoch)?;
    }
    Ok(())
}

fn disabled_request(response: &Value) -> bool {
    response["error"]["code"] == "server_restore_not_available"
        && response["error"]["request_not_committed"] == true
}

fn decode_response(status: reqwest::StatusCode, bytes: &[u8]) -> anyhow::Result<Value> {
    let value: Value = serde_json::from_slice(bytes)?;
    ensure!(
        !disabled_request(&value) || status == reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "invalid feature-disabled response"
    );
    if !status.is_success()
        && status != reqwest::StatusCode::NOT_FOUND
        && !(status == reqwest::StatusCode::SERVICE_UNAVAILABLE && disabled_request(&value))
    {
        bail!("central restore rejected");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsassemble_persistence::{ServerOwnerAuthority, SqliteStore};

    #[tokio::test]
    async fn native_binding_uses_fresh_key_and_rejects_changed_intent() -> anyhow::Result<()> {
        binding_fixture(false).await
    }

    #[tokio::test]
    #[ignore = "requires Node and IDENTITY_DIRECTORY checkout; see scripts/check_restore_fixture.mjs"]
    async fn native_restore_worker_shared_fixture() -> anyhow::Result<()> {
        binding_fixture(true).await
    }

    async fn binding_fixture(cross_implementation: bool) -> anyhow::Result<()> {
        let source_dir = tempfile::tempdir()?;
        let source = SqliteStore::open_path(&source_dir.path().join("source.sqlite")).await?;
        source
            .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
            .await?;
        let old = CentralHostIdentity::from_persistent(&source.host_identity().await?)?;
        let payload = source_dir.path().join("payload.sqlite");
        source
            .export_backup_payload(
                &ServerOwnerAuthority::LocalOperator,
                &source_dir.path().join("snapshot.sqlite"),
                &payload,
            )
            .await?;
        let (database_bytes, database_sha256) =
            backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
        let manifest = backup_codec::BackupManifest {
            schema_version: agentsassemble_persistence::CURRENT_SCHEMA_VERSION,
            server_id: old.server_id().into(),
            server_name: "Owner".into(),
            issuer: "https://central.example".into(),
            owner_person_id: "owner".into(),
            source_epoch: "old-epoch".into(),
            source_key_fingerprint: old.fingerprint().into(),
            snapshot_at: chrono::Utc::now(),
            rooms: 0,
            messages: 0,
            assets: 0,
            database_bytes,
            database_sha256,
        };
        let target_dir = tempfile::tempdir()?;
        let target = target_dir.path().join("runtime.sqlite3");
        let mut job = RestoreJob::begin(&target)?;
        let operation = job.journal().operation_id;
        job.prepare(&payload, manifest).await?;
        let fresh = identity(&job).await?;
        job.bind_switch("current-epoch", fresh.fingerprint())?;
        let tuple = tuple(&job)?;
        let canonical = serde_json::to_vec(&tuple)?;
        let signature = fresh.sign_restore(&canonical);
        assert!(fresh.verify_restore(&canonical, &signature));
        assert!(!old.verify_restore(&canonical, &signature));
        for field in [
            "server_id",
            "expected_epoch",
            "new_key_fingerprint",
            "operation_id",
            "purpose",
        ] {
            let mut changed = tuple.clone();
            changed[field] = json!("changed");
            assert!(!fresh.verify_restore(&serde_json::to_vec(&changed)?, &signature));
        }
        let expected_base = base(&job, &fresh)?;
        assert_ne!(fresh.fingerprint(), old.fingerprint());
        assert!(base(&job, &old).is_err());
        drop(job);
        let resumed = execute(
            &target,
            Request::Bind {
                operation_id: operation,
                expected_epoch: "current-epoch".into(),
            },
        )
        .await?;
        assert_eq!(resumed["body"], expected_base);
        if cross_implementation {
            assert_worker_fixture(&target, operation, &resumed).await?;
        }
        assert!(
            execute(
                &target,
                Request::Bind {
                    operation_id: operation,
                    expected_epoch: "different-epoch".into()
                }
            )
            .await
            .is_err()
        );
        assert_fenced_cancellation(&target, operation)?;
        execute(
            &target,
            Request::Cancel {
                operation_id: operation,
            },
        )
        .await?;
        assert!(execute(&target, Request::Inspect).await?.is_null());
        assert!(!target.exists());
        source.close().await?;
        Ok(())
    }
    async fn assert_worker_fixture(
        target: &Path,
        operation: uuid::Uuid,
        bound: &Value,
    ) -> anyhow::Result<()> {
        let draft = execute(
            target,
            Request::Draft {
                operation_id: operation,
                owner_person_id: "fixture-owner".into(),
                proof: "f".repeat(43),
            },
        )
        .await?;
        let job = RestoreJob::resume(target, operation)?;
        let body = draft["body"].as_str().context("missing draft")?;
        validate_switch(&job, body).await?;
        let fixture = target.with_extension("fixture.json");
        std::fs::write(
            &fixture,
            serde_json::to_vec(&json!({"bound":bound,"draft":draft,
                "issuer":central()?.origin().ascii_serialization()}))?,
        )?;
        let result = std::process::Command::new("node")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../scripts/check_restore_fixture.mjs"
            ))
            .arg(&fixture)
            .output()?;
        ensure!(
            result.status.success(),
            "Worker fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        Ok(())
    }
    fn assert_fenced_cancellation(target: &Path, operation: uuid::Uuid) -> anyhow::Result<()> {
        let mut job = RestoreJob::resume(target, operation)?;
        job.mark_switch_unknown()?;
        assert!(
            job.record_not_committed(&uuid::Uuid::new_v4().to_string(), "current-epoch")
                .is_err()
        );
        assert!(
            job.record_not_committed(&operation.to_string(), "different-epoch")
                .is_err()
        );
        drop(job);
        assert!(RestoreJob::resume(target, operation)?.cancel().is_err());
        let mut job = RestoreJob::resume(target, operation)?;
        for body in [
            json!({"error":{"code":"temporary_unavailable"}}),
            json!({"error":{"code":"server_restore_not_available"}}),
        ] {
            assert!(
                decode_response(
                    reqwest::StatusCode::SERVICE_UNAVAILABLE,
                    &serde_json::to_vec(&body)?
                )
                .is_err()
            );
        }
        let response = decode_response(
            reqwest::StatusCode::SERVICE_UNAVAILABLE,
            br#"{"error":{"code":"server_restore_not_available","request_not_committed":true}}"#,
        )?;
        record_disabled_request(&mut job, false, &response)?;
        drop(job);
        assert!(RestoreJob::resume(target, operation)?.cancel().is_err());
        let mut job = RestoreJob::resume(target, operation)?;
        record_disabled_request(&mut job, true, &response)?;
        assert_eq!(
            RestoreJob::inspect(target)?
                .ok_or_else(|| anyhow::anyhow!("missing job"))?
                .phase,
            RestorePhase::NotCommitted
        );
        Ok(())
    }
}
