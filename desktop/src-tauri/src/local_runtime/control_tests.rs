use std::{
    env,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use agentsassemble_protocol::LocalControlResponse;
use url::Url;

use super::{
    HttpTicketKind, ManagerRoomAuthority, TicketFailure, control_ticket_failure,
    decode_http_ticket_response,
};
use crate::local_runtime::{RuntimeProcess, handle_ticket_result};

const PRESERVATION_CHILD_ENV: &str = "AGENTSASSEMBLE_RUNTIME_PRESERVATION_CHILD";

#[test]
fn http_ticket_response_variant_must_match_the_exact_request_purpose() {
    let response = LocalControlResponse::PreferencesWriteOk {
        request_id: "request-1".to_owned(),
        ticket: "a".repeat(64),
        ttl_seconds: 30,
    };
    assert!(matches!(
        decode_http_ticket_response(
            HttpTicketKind::PreferencesRead("general"),
            "request-1",
            response,
        ),
        Err(TicketFailure::Broken(_))
    ));

    let response = LocalControlResponse::MessagePinsWriteOk {
        request_id: "request-pin".to_owned(),
        ticket: "d".repeat(64),
        ttl_seconds: 30,
    };
    assert!(matches!(
        decode_http_ticket_response(
            HttpTicketKind::MessagePinsRead("general"),
            "request-pin",
            response,
        ),
        Err(TicketFailure::Broken(_))
    ));

    let response = LocalControlResponse::MessageAttachmentReadOk {
        request_id: "request-attachment".to_owned(),
        ticket: "e".repeat(64),
        ttl_seconds: 30,
    };
    assert!(matches!(
        decode_http_ticket_response(
            HttpTicketKind::MessageAttachmentUpload("general"),
            "request-attachment",
            response,
        ),
        Err(TicketFailure::Broken(_))
    ));

    let response = LocalControlResponse::HumanInviteRevokeOk {
        request_id: "request-2".to_owned(),
        ticket: "b".repeat(64),
        ttl_seconds: 30,
    };
    assert!(matches!(
        decode_http_ticket_response(
            HttpTicketKind::HumanInviteCreate(&ManagerRoomAuthority {
                server_id: "10000000-0000-4000-8000-000000000001".to_owned(),
                authority_lineage_id: "20000000-0000-4000-8000-000000000002".to_owned(),
                room_id: "general".to_owned(),
                room_uid: "30000000-0000-4000-8000-000000000003".to_owned(),
            }),
            "request-2",
            response,
        ),
        Err(TicketFailure::Broken(_))
    ));

    let authority = ManagerRoomAuthority {
        server_id: "10000000-0000-4000-8000-000000000001".to_owned(),
        authority_lineage_id: "20000000-0000-4000-8000-000000000002".to_owned(),
        room_id: "general".to_owned(),
        room_uid: "30000000-0000-4000-8000-000000000003".to_owned(),
    };
    let response = LocalControlResponse::AppearanceBoundReadOk {
        request_id: "request-3".to_owned(),
        ticket: "c".repeat(64),
        ttl_seconds: 30,
    };
    assert!(matches!(
        decode_http_ticket_response(
            HttpTicketKind::AppearancePendingRead(
                &authority,
                "ra_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            ),
            "request-3",
            response,
        ),
        Err(TicketFailure::Broken(_))
    ));
}

#[test]
fn application_denials_preserve_the_same_owned_runtime() {
    let executable =
        env::current_exe().unwrap_or_else(|error| panic!("locate test binary: {error}"));
    let child = Command::new(executable)
        .args([
            "--exact",
            "local_runtime::control::tests::runtime_preservation_child",
        ])
        .env(PRESERVATION_CHILD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn owned runtime fixture: {error}"));
    let child_id = child.id();
    let (_output_sender, output) = mpsc::channel();
    let mut process = Some(RuntimeProcess {
        child,
        control: None,
        output,
        address: Url::parse("http://127.0.0.1:43123")
            .unwrap_or_else(|error| panic!("parse runtime fixture address: {error}")),
    });

    for code in ["muted", "permission_denied", "room_authority_changed"] {
        let error = handle_ticket_result::<()>(
            &mut process,
            Err(control_ticket_failure(code, code.to_owned())),
        )
        .err()
        .unwrap_or_else(|| panic!("application denial must remain a rejected operation"));
        assert_eq!(error, code);
        assert_eq!(
            process.as_ref().map(|runtime| runtime.child.id()),
            Some(child_id)
        );
    }

    let mut preserved = process
        .take()
        .unwrap_or_else(|| panic!("runtime must remain owned"));
    let _ = preserved.child.kill();
    let _ = preserved.child.wait();
}

#[test]
fn runtime_preservation_child() {
    if env::var_os(PRESERVATION_CHILD_ENV).is_some() {
        thread::sleep(Duration::from_secs(10));
    }
}
