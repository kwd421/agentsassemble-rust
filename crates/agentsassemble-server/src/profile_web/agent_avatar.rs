use crate::room_session_http_authority::{
    RoomSessionBearerError, RoomSessionBearerResolution, resolve_room_session_bearer,
};
use agentsassemble_persistence::{RoomManagerAuthority, RoomSessionAuthorization};

use super::{
    AppState, EncodedAttachmentUpload, Json, MAX_BASE64_UPLOAD_BODY_BYTES, Path, ProfileHttpError,
    Request, Response, State, avatar_admission, bearer_credential, decode_attachment_content,
    decode_upload_body, ensure_empty_body, json,
};

pub(super) async fn upload(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    policy: axum::Extension<crate::http_body_policy::Policy>,
    request: Request,
) -> Result<Json<serde_json::Value>, ProfileHttpError> {
    let token = bearer_credential(request.headers()).ok_or_else(ProfileHttpError::unauthorized)?;
    let authority = match resolve_room_session_bearer(
        &state,
        request.headers(),
        request.extensions().get(),
        token,
    )
    .await
    {
        Ok(RoomSessionBearerResolution::Authorized(session)) => match *session {
            RoomSessionAuthorization::Operator(session) => {
                RoomManagerAuthority::Operator(Box::new(session))
            }
            RoomSessionAuthorization::Human(_) => return Err(ProfileHttpError::unauthorized()),
        },
        Ok(RoomSessionBearerResolution::Other) => RoomManagerAuthority::Local(
            state
                .tickets
                .consume_agent_avatar_upload(token, &session_id)
                .await
                .map_err(|_| ProfileHttpError::unauthorized())?,
        ),
        Err(RoomSessionBearerError::Invalid) => return Err(ProfileHttpError::unauthorized()),
        Err(RoomSessionBearerError::Persistence(error)) => return Err(error.into()),
    };
    let _permit = avatar_admission::acquire(&state.avatar_uploads)?;
    let principal = match &authority {
        RoomManagerAuthority::Local(binding) => &binding.manager.user_id,
        RoomManagerAuthority::Operator(session) => &session.principal().principal_id,
    };
    let (payload, reservation): (EncodedAttachmentUpload, _) = decode_upload_body(
        request,
        MAX_BASE64_UPLOAD_BODY_BYTES,
        &policy,
        &state.upload_bodies,
        principal,
    )
    .await?;
    let attachment = state
        .connections
        .clone()
        .spawn(async move {
            let _reservation = reservation;
            let _permit = _permit;
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
            Ok::<_, ProfileHttpError>(attachment)
        })
        .await
        .map_err(|_| ProfileHttpError::internal())??;
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
    let permit = avatar_admission::acquire(&state.avatar_reads)?;
    let (asset, lease) = state
        .store
        .agent_avatar(&asset_id, |size| state.downloads.reserve(None, size))
        .await?;
    avatar_admission::response(
        &asset.metadata.filename,
        &asset.metadata.content_type,
        lease.retain(asset.content),
        true,
        permit,
    )
}
