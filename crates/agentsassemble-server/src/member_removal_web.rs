//! Self-removal uses the existing bounded member challenge and pinned redemption.
use super::*;
use crate::secure_client::SecureClient;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemovalRequest {
    challenge_id: String,
    grant_token: String,
    request_id: String,
}

fn removal_client(request: &Request) -> Result<SecureClient, HumanInviteHttpError> {
    crate::secure_client::from_request(request)
        .filter(|client| {
            client.hello().purpose == "account_deletion" && !client.closed().is_cancelled()
        })
        .ok_or_else(invalid_challenge)
}

pub(crate) async fn start(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let client = removal_client(&request)?;
    let browser = browser(request.headers())?;
    let _: Empty = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let epoch = state
        .store
        .registration_epoch()
        .await?
        .ok_or_else(invalid_challenge)?;
    let (id, expires) = state.member_challenges.issue_for(
        ChallengePurpose::Deletion(Arc::new(tokio::sync::Mutex::new(false))),
        browser,
        epoch.clone(),
        Utc::now(),
        Some(client.binding().clone()),
    )?;
    Ok(Json(
        json!({"challenge_id":id,"challenge_hash":URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes())),"server_id":state.central_host_identity.server_id(),"registration_epoch":epoch,"expires_at":expires.timestamp()}),
    ))
}

pub(crate) async fn execute(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<Value>, HumanInviteHttpError> {
    let client = removal_client(&request)?;
    let browser = browser(request.headers())?;
    let body: RemovalRequest = decode_json_body(request, MAX_ADMISSION_BODY_BYTES)
        .await
        .map_err(HumanInviteHttpError::from_body)?;
    let entry = state
        .member_challenges
        .0
        .lock()
        .get(&body.challenge_id)
        .cloned()
        .ok_or_else(invalid_challenge)?;
    if entry.secure.as_ref() != Some(client.binding())
        || entry.browser != browser
        || entry.expires <= Utc::now()
        || state.store.registration_epoch().await?.as_deref() != Some(&entry.epoch)
    {
        return Err(invalid_challenge());
    }
    let ChallengePurpose::Deletion(record) = entry.purpose else {
        return Err(invalid_challenge());
    };
    let mut claimed = record.lock().await;
    if *claimed {
        return Err(invalid_challenge());
    }
    *claimed = true;
    let principal = state
        .central_directory
        .removal_admission(
            crate::central::directory::RedeemHost {
                identity: &state.central_host_identity,
                store: &state.store,
            },
            &body.grant_token,
            &URL_SAFE_NO_PAD.encode(Sha256::digest(body.challenge_id.as_bytes())),
            &body.request_id,
            &client,
        )
        .await
        .map_err(|error| redeem_error(&error))?;
    let key = state
        .store
        .begin_member_account_removal(&principal, client.binding())
        .await?;
    crate::member_removal_runtime::wait_for_completion(&state.store, &key, &state.shutdown).await?;
    Ok(Json(json!({"status":"account_removed"})))
}
