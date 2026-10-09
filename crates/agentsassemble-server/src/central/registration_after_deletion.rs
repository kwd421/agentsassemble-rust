use super::{AppState, Deserialize, Json, RegistrationHttpError, json, valid_owner_person_id};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AccountStopRequest {
    server_id: String,
    account_deletion: bool,
    expected_owner_person_id: String,
    registration_epoch: String,
}

pub(super) async fn stop_account_host(
    state: &AppState,
    request: AccountStopRequest,
) -> Result<Json<serde_json::Value>, RegistrationHttpError> {
    if !request.account_deletion
        || request.server_id != state.central_host_identity.server_id()
        || !valid_owner_person_id(&request.expected_owner_person_id)
    {
        return Err(RegistrationHttpError::bad_request(
            "account stop host binding is invalid",
        ));
    }
    let issuer = state
        .central_directory
        .issuer()
        .map_err(|_| RegistrationHttpError::persistence())?;
    let key = if let Some(key) = state
        .store
        .account_deleted_host_job(
            &issuer,
            &request.expected_owner_person_id,
            &request.registration_epoch,
        )
        .await
        .map_err(|_| RegistrationHttpError::persistence())?
    {
        key
    } else {
        let person = state
            .central_directory
            .current_host_owner(
                crate::central::directory::RedeemHost {
                    identity: &state.central_host_identity,
                    store: &state.store,
                },
                &request.expected_owner_person_id,
                &request.registration_epoch,
            )
            .await
            .map_err(|_| RegistrationHttpError::persistence())?;
        state
            .store
            .account_deleted_host(&issuer, &person, &request.registration_epoch)
            .await
            .map_err(|_| RegistrationHttpError::persistence())?
    };
    state
        .public_ingress()
        .demote()
        .await
        .map_err(|_| RegistrationHttpError::persistence())?;
    crate::member_removal_runtime::wait_for_completion(&state.store, &key, &state.shutdown)
        .await
        .map_err(|_| RegistrationHttpError::persistence())?;
    Ok(Json(json!({"status":"account_host_stopped"})))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FreshEpochRequest {
    server_id: String,
    new_owner_person_id: String,
    expected_registration_epoch: String,
    registration_epoch: String,
}

pub(super) async fn commit_epoch(
    state: &AppState,
    request: FreshEpochRequest,
) -> Result<Json<serde_json::Value>, RegistrationHttpError> {
    if request.server_id != state.central_host_identity.server_id()
        || !valid_owner_person_id(&request.new_owner_person_id)
    {
        return Err(RegistrationHttpError::bad_request(
            "fresh registration host binding is invalid",
        ));
    }
    state
        .store
        .register_after_account_deletion(
            &state
                .central_directory
                .issuer()
                .map_err(|_| RegistrationHttpError::persistence())?,
            &request.new_owner_person_id,
            &request.expected_registration_epoch,
            &request.registration_epoch,
        )
        .await
        .map_err(|_| RegistrationHttpError::persistence())?;
    state
        .public_ingress()
        .resume_after_account_deletion()
        .await
        .map_err(|_| RegistrationHttpError::persistence())?;
    Ok(Json(json!({"status":"ok"})))
}
