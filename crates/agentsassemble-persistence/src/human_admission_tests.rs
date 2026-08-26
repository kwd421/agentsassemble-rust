use crate::{
    HumanAdmissionInput, HumanAdmissionInputError, HumanInviteCredentialEvidence,
    PreparedHumanAdmission,
};

fn input(request_id: &str, participant_type: &str, avatar_image_url: &str) -> HumanAdmissionInput {
    HumanAdmissionInput {
        request_id: request_id.to_owned(),
        meeting_id_assertion: "general".to_owned(),
        display_name: "홍길동\tGuest".to_owned(),
        participant_type: participant_type.to_owned(),
        owner_display_name: "Host".to_owned(),
        client_id: "client-α".to_owned(),
        avatar_image_url: avatar_image_url.to_owned(),
    }
}

fn prepared(participant_type: &str, avatar_image_url: &str) -> PreparedHumanAdmission {
    PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: [0x11; 32],
        },
        [0x22; 32],
        &input(
            "123e4567-e89b-12d3-a456-426614174000",
            participant_type,
            avatar_image_url,
        ),
    )
    .unwrap_or_else(|error| panic!("prepare admission: {error}"))
}

#[test]
fn fixed_vectors_pin_every_admission_transcript() {
    let request = prepared("human", "/api/attachments/avatar_1234?view=1");

    assert_eq!(
        hex::encode(request.one_use_admission_key()),
        "febebeab644e0344588b8c81db98427d32b8afdf4053cceadb92f8097ee24648"
    );
    assert_eq!(
        hex::encode(request.reusable_admission_key()),
        "dd65660dafacc7aad639e45d2236713262bccc4148710e264960e92306eca539"
    );
    assert_eq!(
        hex::encode(request.payload_hash()),
        "243c9e5901c07a27c4bd10abc081a1e6283e6a3f14c5c7a996d010a2ea375e65"
    );
    assert_eq!(
        hex::encode(request.avatar_custody_fingerprint()),
        "494d2f51448299cf9656e2744ca39252f4691a9f9c05f76d0f35604800f34f41"
    );
    assert_eq!(request.avatar_attachment_id(), Some("avatar_1234"));
}

#[test]
fn canonical_input_preserves_original_text_and_human_alias_contracts() {
    let mut source = input(
        "  123e4567-e89b-12d3-a456-426614174000  ",
        " PERSON ",
        " /api/attachments/avatar_1234?view=1 ",
    );
    source.meeting_id_assertion = " general\r\n ".to_owned();
    source.display_name = " Name\t With  Spaces\n ".to_owned();
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::Signed {
            fingerprint: [0x11; 32],
            room_id: "general".to_owned(),
            base_participant_id: "guest".to_owned(),
            display_name: "Guest".to_owned(),
            invite_scope: agentsassemble_domain::InviteScope::ReadWrite,
            issued_at: chrono::DateTime::UNIX_EPOCH,
            expires_at: chrono::DateTime::UNIX_EPOCH + chrono::TimeDelta::hours(1),
        },
        [0x22; 32],
        &source,
    )
    .unwrap_or_else(|error| panic!("prepare canonical admission: {error}"));

    assert_eq!(request.request_id().to_string(), source.request_id.trim());
    assert_eq!(request.meeting_id_assertion(), "general");
    assert_eq!(request.display_name(), "Name\t With  Spaces");
    assert_eq!(request.participant_type_input(), "PERSON");
    assert_eq!(request.avatar_attachment_id(), Some("avatar_1234"));

    let human = prepared("human", "/api/attachments/avatar_1234?view=1");
    let person = prepared("person", "/api/attachments/avatar_1234?view=1");
    let browser = prepared("browser", "/api/attachments/avatar_1234?view=1");
    let unknown_token = prepared("some-unknown-value", "/api/attachments/avatar_1234?view=1");
    assert_eq!(human.display_name(), person.display_name());
    assert_eq!(human.avatar_attachment_id(), person.avatar_attachment_id());
    assert_ne!(human.payload_hash(), person.payload_hash());
    assert_ne!(human.payload_hash(), browser.payload_hash());
    assert_ne!(browser.payload_hash(), unknown_token.payload_hash());
}

#[test]
fn invalid_identity_input_fails_and_invalid_optional_avatar_is_omitted() {
    for request_id in [
        "00000000-0000-0000-0000-000000000000",
        "123E4567-E89B-12D3-A456-426614174000",
        "123e4567e89b12d3a456426614174000",
        "not-a-uuid",
    ] {
        assert_eq!(
            PreparedHumanAdmission::prepare(
                HumanInviteCredentialEvidence::JoinCode {
                    fingerprint: [0x11; 32],
                },
                [0x22; 32],
                &input(request_id, "human", ""),
            )
            .err(),
            Some(HumanAdmissionInputError::RequestId)
        );
    }
    for participant_type in [
        "agent",
        "ai",
        "bot",
        "subscription_ai",
        "api",
        "local",
        "remote",
        "unknown",
    ] {
        assert_eq!(
            PreparedHumanAdmission::prepare(
                HumanInviteCredentialEvidence::JoinCode {
                    fingerprint: [0x11; 32],
                },
                [0x22; 32],
                &input("123e4567-e89b-12d3-a456-426614174000", participant_type, "",),
            )
            .err(),
            Some(HumanAdmissionInputError::ParticipantType)
        );
    }

    let malformed = prepared("human", "/api/attachments/not valid?view=1");
    let absent = prepared("human", "");
    assert_eq!(malformed.avatar_attachment_id(), None);
    assert_eq!(malformed.payload_hash(), absent.payload_hash());
}
