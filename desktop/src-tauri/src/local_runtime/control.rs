use std::{io::Write, time::Duration};

use agentsassemble_protocol::{LocalBootstrapGrant, LocalControlRequest, LocalControlResponse};
use uuid::Uuid;

use super::{
    CentralRegistrationTicketGrant, OperatorHttpTicketGrant, RuntimeOutput, RuntimeProcess,
    TicketGrant,
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
    let (ticket, ttl_seconds, server_proof_key) = match response {
        LocalControlResponse::Ok {
            request_id: response_id,
            ticket,
            ttl_seconds,
            server_proof_key,
        } if response_id == request_id => (ticket, ttl_seconds, server_proof_key),
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
    if ticket.is_empty()
        || ttl_seconds == 0
        || server_proof_key.len() != 64
        || !server_proof_key
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
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
        server_proof_key,
    })
}

pub(super) fn request_operator_http_ticket(
    runtime: &mut RuntimeProcess,
) -> Result<OperatorHttpTicketGrant, TicketFailure> {
    let request_id = Uuid::new_v4().to_string();
    let request = LocalControlRequest::IssueOperatorHttpTicket {
        request_id: request_id.clone(),
    };
    let response = request_control(runtime, &request)?;
    let (ticket, ttl_seconds) = match response {
        LocalControlResponse::OperatorHttpOk {
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
                "local runtime operator ticket response did not match the request".to_owned(),
            ));
        }
    };
    if ticket.len() != 64
        || !ticket.bytes().all(|byte| byte.is_ascii_hexdigit())
        || ttl_seconds == 0
    {
        return Err(TicketFailure::Broken(
            "local runtime returned an invalid operator ticket grant".to_owned(),
        ));
    }
    Ok(OperatorHttpTicketGrant {
        ticket,
        ttl_seconds,
        http_base_url: runtime.address.to_string().trim_end_matches('/').to_owned(),
    })
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
        "bad_request" | "room_not_found" | "session_revoked" | "bootstrap_required"
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
