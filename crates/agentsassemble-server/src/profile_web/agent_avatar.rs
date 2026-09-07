use super::{
    AppState, EncodedAttachmentUpload, Json, MAX_BASE64_UPLOAD_BODY_BYTES, Path, ProfileHttpError,
    Request, Response, State, attachment_response, bearer_credential, decode_attachment_content,
    decode_json_body, ensure_empty_body, json,
};

pub(super) async fn upload(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let token = bearer_credential(request.headers()).ok_or_else(ProfileHttpError::unauthorized)?;
    let authority = state
        .tickets
        .consume_agent_avatar_upload(token, &session_id)
        .await
        .map_err(|_| ProfileHttpError::unauthorized())?;
    let payload: EncodedAttachmentUpload = decode_json_body(request, MAX_BASE64_UPLOAD_BODY_BYTES)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let content = decode_attachment_content(&payload.data_base64)?;
    let attachment = state
        .store
        .store_agent_avatar(
            &authority,
            &session_id,
            &payload.filename,
            &payload.content_type,
            content,
        )
        .await?;
    Ok(Json(json!({"attachment": attachment})))
}

pub(super) async fn read(
    State(state): State<AppState>,
    Path(asset_id): Path<String>,
    request: Request,
) -> Result<Response, ProfileHttpError> {
    ensure_empty_body(request, 0)
        .await
        .map_err(ProfileHttpError::from_body)?;
    let asset = state.store.agent_avatar(&asset_id).await?;
    attachment_response(
        &asset.metadata.filename,
        &asset.metadata.content_type,
        asset.content,
        true,
    )
}
