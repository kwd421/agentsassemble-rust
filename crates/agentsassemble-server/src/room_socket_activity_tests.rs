use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::{
    RoomManagerAuthority, RoomSessionAuthorization, ServerOwnerAuthority,
};
use agentsassemble_protocol::{CommandAck, CommandResolution, ProtocolError, ServerFrame};
use agentsassemble_provider::ProviderCatalogService;
use futures_util::SinkExt;
use serde_json::json;

use crate::{AppState, TicketStore, ticket_tests::HumanSessionFixture};

async fn native_fixture()
-> Result<(AppState, RoomSessionAuthorization, i64), Box<dyn std::error::Error>> {
    let fixture = HumanSessionFixture::new(0).await;
    let store = fixture.store();
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let old = chrono::Utc::now() - chrono::Duration::seconds(120);
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Local(manager),
            &[51; 32],
            "https://activity.test",
            old,
        )
        .await?;
    let paired = store
        .redeem_operator_pairing(&[51; 32], &[52; 32], "https://activity.test", old)
        .await?;
    let state = AppState::local(
        store.clone(),
        TicketStore::new(std::time::Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await?;
    Ok((
        state,
        RoomSessionAuthorization::Operator(paired.authorization),
        old.timestamp(),
    ))
}

#[tokio::test]
async fn nack_only_established_traffic_is_read_only_and_ack_pong_refresh()
-> Result<(), Box<dyn std::error::Error>> {
    for frame in [
        ServerFrame::Pong {
            nonce: json!("authorized"),
        },
        ServerFrame::Ack(CommandAck {
            request_id: "success".into(),
            accepted: true,
            resolution: CommandResolution::Committed,
            action: "room.history".into(),
            result: json!({}),
            deduplicated: false,
        }),
    ] {
        let (state, authorization, old) = native_fixture().await?;
        let store = &state.store;
        let mut principal = authorization.principal().clone();
        let mut session = Some(authorization);
        let mut sink = futures_util::sink::drain::<axum::extract::ws::Message>()
            .sink_map_err(|error| match error {});
        for code in [
            "permission_denied",
            "frame_schema_invalid",
            "ingress_limited",
        ] {
            assert!(
                super::send_authorized_nack(
                    &state,
                    &mut principal,
                    &mut session,
                    &mut sink,
                    (
                        "rejected",
                        "command",
                        CommandResolution::Rejected,
                        ProtocolError::new(code, "Rejected")
                    )
                )
                .await
                .is_some()
            );
            assert_eq!(
                store
                    .owner_device_sessions(&ServerOwnerAuthority::LocalOperator)
                    .await?[0]
                    .last_connected_at,
                Some(old)
            );
        }
        let resync = ServerFrame::ResyncRequired {
            stream: "room_events",
            reason: "capacity".into(),
            latest_seq: 0,
        };
        assert!(
            super::send_authorized_frame(&state, &mut principal, &mut session, &mut sink, &resync)
                .await
                .is_some()
        );
        assert_eq!(
            store
                .owner_device_sessions(&ServerOwnerAuthority::LocalOperator)
                .await?[0]
                .last_connected_at,
            Some(old)
        );
        assert!(
            super::send_authorized_frame(&state, &mut principal, &mut session, &mut sink, &frame)
                .await
                .is_some()
        );
        assert!(
            store
                .owner_device_sessions(&ServerOwnerAuthority::LocalOperator)
                .await?[0]
                .last_connected_at
                > Some(old)
        );
        state.rooms.shutdown().await?;
    }
    Ok(())
}
