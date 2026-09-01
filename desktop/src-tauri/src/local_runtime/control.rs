use std::{io::Write, time::Duration};

use agentsassemble_protocol::{LocalBootstrapGrant, LocalControlRequest, LocalControlResponse};
use uuid::Uuid;

use super::{
    CentralRegistrationTicketGrant, HttpTicketGrant, ManagerRoomAuthority, RuntimeOutput,
    RuntimeProcess, TicketGrant,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) enum TicketFailure {
    Rejected(String),
    Broken(String),
}

pub(super) fn request_bootstrap_status(
    runtime: &mut RuntimeProcess,
) -> Result<LocalBootstrapGrant, TicketFailure> {
    let request_id = Uuid::new_v4().to_string();
    let request = LocalControlRequest::InspectBootstrap {
        request_id: request_id.clone(),
    };
    request_bootstrap(runtime, &request_id, &request)
}

pub(super) fn request_bootstrap_initialize(
    runtime: &mut RuntimeProcess,
    request_id: &str,
    display_name: &str,
) -> Result<LocalBootstrapGrant, TicketFailure> {
    let request = LocalControlRequest::InitializeBootstrap {
        request_id: request_id.to_owned(),
        display_name: display_name.to_owned(),
    };
    request_bootstrap(runtime, request_id, &request)
}

fn request_bootstrap(
    runtime: &mut RuntimeProcess,
    request_id: &str,
    request: &LocalControlRequest,
) -> Result<LocalBootstrapGrant, TicketFailure> {
    match request_control(runtime, request)? {
        LocalControlResponse::BootstrapOk {
            request_id: response_id,
            bootstrap,
        } if response_id == request_id => Ok(*bootstrap),
        LocalControlResponse::Error {
            request_id: response_id,
            code,
            message,
        } if response_id == request_id => {
            if is_bootstrap_rejection(&code) {
                Err(TicketFailure::Rejected(message))
            } else {
                Err(TicketFailure::Broken(message))
            }
        }
        _ => Err(TicketFailure::Broken(
            "local runtime bootstrap response did not match the request".to_owned(),
        )),
    }
}

pub(super) fn request_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<TicketGrant, TicketFailure> {
    let request_id = Uuid::new_v4().to_string();
    let request = LocalControlRequest::IssueTicket {
        request_id: request_id.clone(),
        meeting_id: room_id.to_owned(),
    };
    let response = request_control(runtime, &request)?;
    let (ticket, ttl_seconds) = match response {
        LocalControlResponse::Ok {
            request_id: response_id,
            ticket,
            ttl_seconds,
        } if response_id == request_id => (ticket, ttl_seconds),
        LocalControlResponse::Error {
            request_id: response_id,
            code,
            message,
        } if response_id == request_id => {
            return if is_application_rejection(&code) {
                Err(TicketFailure::Rejected(message))
            } else {
                Err(TicketFailure::Broken(message))
            };
        }
        _ => {
            return Err(TicketFailure::Broken(
                "local runtime ticket response id did not match the request".to_owned(),
            ));
        }
    };
    if ticket.is_empty() || ttl_seconds == 0 {
        return Err(TicketFailure::Broken(
            "local runtime returned an invalid ticket grant".to_owned(),
        ));
    }
    let port = runtime
        .address
        .port()
        .ok_or_else(|| TicketFailure::Broken("local runtime address has no port".to_owned()))?;
    Ok(TicketGrant {
        ticket,
        ttl_seconds,
        websocket_base_url: format!("ws://127.0.0.1:{port}"),
    })
}

pub(super) fn request_operator_http_ticket(
    runtime: &mut RuntimeProcess,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::Operator)
}

pub(super) fn request_preferences_read_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::PreferencesRead(room_id))
}

pub(super) fn request_preferences_write_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::PreferencesWrite(room_id))
}

pub(super) fn request_message_pins_read_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::MessagePinsRead(room_id))
}

pub(super) fn request_message_pins_write_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::MessagePinsWrite(room_id))
}

pub(super) fn request_message_search_read_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::MessageSearchRead(room_id))
}

pub(super) fn request_message_attachment_upload_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::MessageAttachmentUpload(room_id))
}

pub(super) fn request_message_attachment_read_ticket(
    runtime: &mut RuntimeProcess,
    room_id: &str,
    attachment_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(
        runtime,
        HttpTicketKind::MessageAttachmentRead(room_id, attachment_id),
    )
}

pub(super) fn request_human_invite_create_ticket(
    runtime: &mut RuntimeProcess,
    authority: &ManagerRoomAuthority,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::HumanInviteCreate(authority))
}

pub(super) fn request_human_invite_revoke_ticket(
    runtime: &mut RuntimeProcess,
    authority: &ManagerRoomAuthority,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::HumanInviteRevoke(authority))
}

pub(super) fn request_appearance_upload_ticket(
    runtime: &mut RuntimeProcess,
    authority: &ManagerRoomAuthority,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::AppearanceUpload(authority))
}

pub(super) fn request_appearance_pending_read_ticket(
    runtime: &mut RuntimeProcess,
    authority: &ManagerRoomAuthority,
    asset_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(
        runtime,
        HttpTicketKind::AppearancePendingRead(authority, asset_id),
    )
}

pub(super) fn request_appearance_bound_read_ticket(
    runtime: &mut RuntimeProcess,
    authority: &ManagerRoomAuthority,
    asset_id: &str,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(
        runtime,
        HttpTicketKind::AppearanceBoundRead(authority, asset_id),
    )
}

pub(super) fn request_settings_directory_read_ticket(
    runtime: &mut RuntimeProcess,
) -> Result<HttpTicketGrant, TicketFailure> {
    request_http_ticket(runtime, HttpTicketKind::SettingsDirectoryRead)
}

#[derive(Clone, Copy)]
enum HttpTicketKind<'a> {
    Operator,
    PreferencesRead(&'a str),
    PreferencesWrite(&'a str),
    MessagePinsRead(&'a str),
    MessagePinsWrite(&'a str),
    MessageSearchRead(&'a str),
    MessageAttachmentUpload(&'a str),
    MessageAttachmentRead(&'a str, &'a str),
    HumanInviteCreate(&'a ManagerRoomAuthority),
    HumanInviteRevoke(&'a ManagerRoomAuthority),
    AppearanceUpload(&'a ManagerRoomAuthority),
    AppearancePendingRead(&'a ManagerRoomAuthority, &'a str),
    AppearanceBoundRead(&'a ManagerRoomAuthority, &'a str),
    SettingsDirectoryRead,
}

fn request_http_ticket(
    runtime: &mut RuntimeProcess,
    kind: HttpTicketKind<'_>,
) -> Result<HttpTicketGrant, TicketFailure> {
    let request_id = Uuid::new_v4().to_string();
    let request = message_attachment_request(kind, &request_id).unwrap_or_else(|| match kind {
        HttpTicketKind::Operator => LocalControlRequest::IssueOperatorHttpTicket {
            request_id: request_id.clone(),
        },
        HttpTicketKind::PreferencesRead(room_id) => {
            LocalControlRequest::IssuePreferencesReadTicket {
                request_id: request_id.clone(),
                meeting_id: room_id.to_owned(),
            }
        }
        HttpTicketKind::PreferencesWrite(room_id) => {
            LocalControlRequest::IssuePreferencesWriteTicket {
                request_id: request_id.clone(),
                meeting_id: room_id.to_owned(),
            }
        }
        HttpTicketKind::MessagePinsRead(room_id) => {
            LocalControlRequest::IssueMessagePinsReadTicket {
                request_id: request_id.clone(),
                meeting_id: room_id.to_owned(),
            }
        }
        HttpTicketKind::MessagePinsWrite(room_id) => {
            LocalControlRequest::IssueMessagePinsWriteTicket {
                request_id: request_id.clone(),
                meeting_id: room_id.to_owned(),
            }
        }
        HttpTicketKind::MessageSearchRead(room_id) => {
            LocalControlRequest::IssueMessageSearchReadTicket {
                request_id: request_id.clone(),
                meeting_id: room_id.to_owned(),
            }
        }
        HttpTicketKind::MessageAttachmentUpload(_)
        | HttpTicketKind::MessageAttachmentRead(_, _) => {
            unreachable!("message attachment requests are decoded above")
        }
        HttpTicketKind::HumanInviteCreate(authority) => {
            LocalControlRequest::IssueHumanInviteCreateTicket {
                request_id: request_id.clone(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        HttpTicketKind::HumanInviteRevoke(authority) => {
            LocalControlRequest::IssueHumanInviteRevokeTicket {
                request_id: request_id.clone(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        HttpTicketKind::AppearanceUpload(authority) => {
            LocalControlRequest::IssueAppearanceUploadTicket {
                request_id: request_id.clone(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
            }
        }
        HttpTicketKind::AppearancePendingRead(authority, asset_id) => {
            LocalControlRequest::IssueAppearancePendingReadTicket {
                request_id: request_id.clone(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
                asset_id: asset_id.to_owned(),
            }
        }
        HttpTicketKind::AppearanceBoundRead(authority, asset_id) => {
            LocalControlRequest::IssueAppearanceBoundReadTicket {
                request_id: request_id.clone(),
                server_id: authority.server_id.clone(),
                authority_lineage_id: authority.authority_lineage_id.clone(),
                meeting_id: authority.room_id.clone(),
                room_uid: authority.room_uid.clone(),
                asset_id: asset_id.to_owned(),
            }
        }
        HttpTicketKind::SettingsDirectoryRead => {
            LocalControlRequest::IssueSettingsDirectoryReadTicket {
                request_id: request_id.clone(),
            }
        }
    });
    let response = request_control(runtime, &request)?;
    let (ticket, ttl_seconds) = decode_http_ticket_response(kind, &request_id, response)?;
    validate_http_ticket_grant(&ticket, ttl_seconds)?;
    Ok(HttpTicketGrant {
        ticket,
        ttl_seconds,
        http_base_url: runtime.address.to_string().trim_end_matches('/').to_owned(),
    })
}

fn validate_http_ticket_grant(ticket: &str, ttl_seconds: u64) -> Result<(), TicketFailure> {
    if ticket.len() != 64
        || !ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
        || ttl_seconds == 0
    {
        return Err(TicketFailure::Broken(
            "local runtime returned an invalid HTTP ticket grant".to_owned(),
        ));
    }
    Ok(())
}

fn message_attachment_request(
    kind: HttpTicketKind<'_>,
    request_id: &str,
) -> Option<LocalControlRequest> {
    match kind {
        HttpTicketKind::MessageAttachmentUpload(room_id) => {
            Some(LocalControlRequest::IssueMessageAttachmentUploadTicket {
                request_id: request_id.to_owned(),
                meeting_id: room_id.to_owned(),
            })
        }
        HttpTicketKind::MessageAttachmentRead(room_id, attachment_id) => {
            Some(LocalControlRequest::IssueMessageAttachmentReadTicket {
                request_id: request_id.to_owned(),
                meeting_id: room_id.to_owned(),
                attachment_id: attachment_id.to_owned(),
            })
        }
        _ => None,
    }
}

fn decode_http_ticket_response(
    kind: HttpTicketKind<'_>,
    request_id: &str,
    response: LocalControlResponse,
) -> Result<(String, u64), TicketFailure> {
    if matches!(kind, HttpTicketKind::MessageSearchRead(_)) {
        return decode_message_search_ticket_response(request_id, response);
    }
    if matches!(
        kind,
        HttpTicketKind::MessagePinsRead(_) | HttpTicketKind::MessagePinsWrite(_)
    ) {
        return decode_message_pin_ticket_response(kind, request_id, response);
    }
    if matches!(
        kind,
        HttpTicketKind::MessageAttachmentUpload(_) | HttpTicketKind::MessageAttachmentRead(_, _)
    ) {
        return decode_message_attachment_ticket_response(kind, request_id, response);
    }
    match (kind, response) {
        (
            HttpTicketKind::Operator,
            LocalControlResponse::OperatorHttpOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::PreferencesRead(_),
            LocalControlResponse::PreferencesReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::PreferencesWrite(_),
            LocalControlResponse::PreferencesWriteOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::HumanInviteCreate(_),
            LocalControlResponse::HumanInviteCreateOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::HumanInviteRevoke(_),
            LocalControlResponse::HumanInviteRevokeOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::AppearanceUpload(_),
            LocalControlResponse::AppearanceUploadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::AppearancePendingRead(_, _),
            LocalControlResponse::AppearancePendingReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::AppearanceBoundRead(_, _),
            LocalControlResponse::AppearanceBoundReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            HttpTicketKind::SettingsDirectoryRead,
            LocalControlResponse::SettingsDirectoryReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            _,
            LocalControlResponse::Error {
                request_id: response_id,
                code,
                message,
            },
        ) if response_id == request_id => Err(control_ticket_failure(&code, message)),
        _ => Err(TicketFailure::Broken(
            "local runtime HTTP ticket response did not match the request".to_owned(),
        )),
    }
}

fn decode_message_attachment_ticket_response(
    kind: HttpTicketKind<'_>,
    request_id: &str,
    response: LocalControlResponse,
) -> Result<(String, u64), TicketFailure> {
    match (kind, response) {
        (
            HttpTicketKind::MessageAttachmentUpload(_),
            LocalControlResponse::MessageAttachmentUploadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        )
        | (
            HttpTicketKind::MessageAttachmentRead(_, _),
            LocalControlResponse::MessageAttachmentReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            _,
            LocalControlResponse::Error {
                request_id: response_id,
                code,
                message,
            },
        ) if response_id == request_id => Err(control_ticket_failure(&code, message)),
        _ => Err(TicketFailure::Broken(
            "local runtime message-attachment response did not match the request".to_owned(),
        )),
    }
}

fn decode_message_search_ticket_response(
    request_id: &str,
    response: LocalControlResponse,
) -> Result<(String, u64), TicketFailure> {
    match response {
        LocalControlResponse::MessageSearchReadOk {
            request_id: response_id,
            ticket,
            ttl_seconds,
        } if response_id == request_id => Ok((ticket, ttl_seconds)),
        LocalControlResponse::Error {
            request_id: response_id,
            code,
            message,
        } if response_id == request_id => Err(control_ticket_failure(&code, message)),
        _ => Err(TicketFailure::Broken(
            "local runtime message-search response did not match the request".to_owned(),
        )),
    }
}

fn decode_message_pin_ticket_response(
    kind: HttpTicketKind<'_>,
    request_id: &str,
    response: LocalControlResponse,
) -> Result<(String, u64), TicketFailure> {
    match (kind, response) {
        (
            HttpTicketKind::MessagePinsRead(_),
            LocalControlResponse::MessagePinsReadOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        )
        | (
            HttpTicketKind::MessagePinsWrite(_),
            LocalControlResponse::MessagePinsWriteOk {
                request_id: response_id,
                ticket,
                ttl_seconds,
            },
        ) if response_id == request_id => Ok((ticket, ttl_seconds)),
        (
            _,
            LocalControlResponse::Error {
                request_id: response_id,
                code,
                message,
            },
        ) if response_id == request_id => Err(control_ticket_failure(&code, message)),
        _ => Err(TicketFailure::Broken(
            "local runtime message-pin ticket response did not match the request".to_owned(),
        )),
    }
}

fn control_ticket_failure(code: &str, message: String) -> TicketFailure {
    if is_application_rejection(code) {
        TicketFailure::Rejected(message)
    } else {
        TicketFailure::Broken(message)
    }
}

pub(super) fn request_central_registration_ticket(
    runtime: &mut RuntimeProcess,
) -> Result<CentralRegistrationTicketGrant, TicketFailure> {
    let request_id = Uuid::new_v4().to_string();
    let request = LocalControlRequest::IssueCentralRegistrationTicket {
        request_id: request_id.clone(),
    };
    let response = request_control(runtime, &request)?;
    let (ticket, ttl_seconds, server_id, host_public_key_x, host_key_fingerprint) = match response {
        LocalControlResponse::CentralRegistrationOk {
            request_id: response_id,
            ticket,
            ttl_seconds,
            server_id,
            host_public_key_x,
            host_key_fingerprint,
        } if response_id == request_id => (
            ticket,
            ttl_seconds,
            server_id,
            host_public_key_x,
            host_key_fingerprint,
        ),
        LocalControlResponse::Error {
            request_id: response_id,
            code,
            message,
        } if response_id == request_id => {
            return if is_application_rejection(&code) {
                Err(TicketFailure::Rejected(message))
            } else {
                Err(TicketFailure::Broken(message))
            };
        }
        _ => {
            return Err(TicketFailure::Broken(
                "local runtime central registration ticket response did not match the request"
                    .to_owned(),
            ));
        }
    };
    if ticket.len() != 64
        || !ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
        || ttl_seconds == 0
        || Uuid::parse_str(&server_id).is_err()
        || !valid_base64url_32(&host_public_key_x)
        || !valid_base64url_32(&host_key_fingerprint)
    {
        return Err(TicketFailure::Broken(
            "local runtime returned an invalid central registration ticket grant".to_owned(),
        ));
    }
    Ok(CentralRegistrationTicketGrant {
        ticket,
        ttl_seconds,
        http_base_url: runtime.address.to_string().trim_end_matches('/').to_owned(),
        server_id,
        host_public_key_x,
        host_key_fingerprint,
    })
}

fn valid_base64url_32(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn request_control(
    runtime: &mut RuntimeProcess,
    request: &LocalControlRequest,
) -> Result<LocalControlResponse, TicketFailure> {
    if runtime
        .child
        .try_wait()
        .map_err(|error| TicketFailure::Broken(format!("cannot inspect local runtime: {error}")))?
        .is_some()
    {
        return Err(TicketFailure::Broken(
            "the owned Rust runtime exited before ticket issuance".to_owned(),
        ));
    }
    let mut encoded = serde_json::to_vec(request).map_err(|error| {
        TicketFailure::Broken(format!("cannot encode local ticket request: {error}"))
    })?;
    encoded.push(b'\n');
    let control = runtime
        .control
        .as_mut()
        .ok_or_else(|| TicketFailure::Broken("local runtime control pipe is closed".to_owned()))?;
    control
        .write_all(&encoded)
        .and_then(|()| control.flush())
        .map_err(|error| {
            TicketFailure::Broken(format!("cannot write local ticket request: {error}"))
        })?;
    let response = runtime
        .output
        .recv_timeout(REQUEST_TIMEOUT)
        .map_err(|error| {
            TicketFailure::Broken(format!("local runtime ticket response timed out: {error}"))
        })?
        .map_err(TicketFailure::Broken)?;
    let RuntimeOutput::Control(response) = response else {
        return Err(TicketFailure::Broken(
            "local runtime returned a duplicate startup record".to_owned(),
        ));
    };
    Ok(*response)
}

pub(super) fn is_application_rejection(code: &str) -> bool {
    matches!(
        code,
        "bad_request"
            | "room_not_found"
            | "session_revoked"
            | "bootstrap_required"
            | "room_authority_changed"
            | "muted"
            | "permission_denied"
    )
}

fn is_bootstrap_rejection(code: &str) -> bool {
    matches!(
        code,
        "bootstrap_request_invalid"
            | "bootstrap_profile_invalid"
            | "bootstrap_already_complete"
            | "bootstrap_request_conflict"
            | "bootstrap_repair_required"
    )
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;
