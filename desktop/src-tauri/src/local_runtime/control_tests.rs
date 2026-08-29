use std::{
    env,
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    sync::mpsc,
};

use agentsassemble_protocol::LocalControlResponse;
use url::Url;

use super::{
    HttpTicketKind, ManagerRoomAuthority, TicketFailure, control_ticket_failure,
    decode_http_ticket_response,
};
use crate::local_runtime::{RuntimeProcess, handle_ticket_result};

const PRESERVATION_CHILD_ENV: &str = "AGENTSASSEMBLE_RUNTIME_PRESERVATION_CHILD";
const PRESERVATION_CHILD_READY: &str = "agentsassemble-runtime-preservation-ready";

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
    let mut child = Command::new(executable)
        .args([
            "--exact",
            "local_runtime::control::tests::runtime_preservation_child",
        ])
        .env(PRESERVATION_CHILD_ENV, "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn owned runtime fixture: {error}"));
    let control = child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("capture owned runtime fixture control"));
    let stdout = child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("capture owned runtime fixture output"));
    let mut output_reader = BufReader::new(stdout);
    loop {
        let mut line = String::new();
        let read = output_reader
            .read_line(&mut line)
            .unwrap_or_else(|error| panic!("read owned runtime fixture barrier: {error}"));
        assert!(read > 0, "owned runtime fixture exited before its barrier");
        if line.trim() == PRESERVATION_CHILD_READY {
            break;
        }
    }
    let child_id = child.id();
    let (_output_sender, output) = mpsc::channel();
    let mut process = Some(RuntimeProcess {
        child,
        control: Some(control),
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
        let runtime = process
            .as_mut()
            .unwrap_or_else(|| panic!("application denial released the owned runtime"));
        assert_eq!(runtime.child.id(), child_id);
        assert!(
            runtime
                .child
                .try_wait()
                .unwrap_or_else(|error| panic!("inspect owned runtime fixture: {error}"))
                .is_none(),
            "application denial stopped the owned runtime"
        );
    }

    let mut preserved = process
        .take()
        .unwrap_or_else(|| panic!("runtime must remain owned"));
    let mut control = preserved
        .control
        .take()
        .unwrap_or_else(|| panic!("runtime fixture control must remain owned"));
    control
        .write_all(b"x")
        .and_then(|()| control.flush())
        .unwrap_or_else(|error| panic!("release owned runtime fixture: {error}"));
    drop(control);
    let status = preserved
        .child
        .wait()
        .unwrap_or_else(|error| panic!("join owned runtime fixture: {error}"));
    assert!(status.success());
}

#[test]
fn runtime_preservation_child() {
    if env::var_os(PRESERVATION_CHILD_ENV).is_some() {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{PRESERVATION_CHILD_READY}")
            .and_then(|()| stdout.flush())
            .unwrap_or_else(|error| panic!("publish runtime fixture barrier: {error}"));
        let mut release = [0_u8; 1];
        std::io::stdin()
            .lock()
            .read_exact(&mut release)
            .unwrap_or_else(|error| panic!("await runtime fixture release: {error}"));
    }
}
