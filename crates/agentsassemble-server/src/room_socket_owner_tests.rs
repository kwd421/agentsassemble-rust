use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::{
    CentralOwnerSessionRequest, OwnerAdmission, OwnerAdmissionBinding, OwnerDeviceDescription,
    OwnerSessionAuthorization, RoomSessionAuthorization, ServerOwnerAuthority, SqliteStore,
};
use agentsassemble_provider::ProviderCatalogService;
use axum::extract::ws::Message;
use futures_util::{Sink, stream};

use crate::{
    AppState, TicketStore, owner_session_lifetime::OwnerSessionLease, ticket::ConsumedSocketTicket,
};

const ORIGIN: &str = "https://owner.example.test";

async fn fixture()
-> Result<(AppState, OwnerSessionAuthorization, ConsumedSocketTicket), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    let room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let state = AppState::local(
        store.clone(),
        TicketStore::new(Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await?
    .with_manual_public_ingress(
        "127.0.0.1:12345".parse()?,
        ORIGIN,
        "socket-owner-test-proxy-secret-0000001",
    )?;
    let binding = OwnerAdmissionBinding {
        secure: None,
        entry_fingerprint: [1; 32],
        server_id: store.local_bootstrap_status().await?.server_id,
        person_id: "person".into(),
        device_id: "device".into(),
        browser_fingerprint: [42; 32],
        origin: ORIGIN.into(),
        generation: store.next_central_endpoint_generation().await?,
    };
    let owner = store
        .create_owner_session(
            &OwnerAdmission::verified(binding, chrono::Utc::now().timestamp() + 300)?,
            &OwnerDeviceDescription::verified("Browser".into(), "Chrome".into(), "macOS".into())?,
        )
        .await?
        .authorization;
    let room_session = store
        .create_central_owner_session(
            &ServerOwnerAuthority::CentralSession(Box::new(owner.clone())),
            &CentralOwnerSessionRequest::host_owned(
                "room",
                room.room.room_uid,
                &[17; 32],
                &[42; 32],
                ORIGIN,
                chrono::Utc::now(),
            ),
        )
        .await?;
    state.owner_sessions.admit(&state, owner.clone())?;
    let ticket = state
        .tickets
        .issue_room_session_socket(RoomSessionAuthorization::Operator(
            room_session.authorization,
        ))
        .await?;
    let grant = state.tickets.consume_socket(&ticket.ticket).await?;
    Ok((state, owner, grant))
}

struct FirstFrameSink {
    state: AppState,
    owner: OwnerSessionAuthorization,
    directory: Option<OwnerSessionLease>,
    frames: Vec<Message>,
}

impl Sink<Message> for FirstFrameSink {
    type Error = axum::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(mut self: Pin<&mut Self>, frame: Message) -> Result<(), Self::Error> {
        // Exact first-send barrier: the directory closes while socket setup is sending.
        drop(self.directory.take());
        assert!(
            self.state.owner_sessions.require_live(&self.owner).is_ok(),
            "first socket frame was sent without retaining owner custody"
        );
        self.frames.push(frame);
        Ok(())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn socket_retains_owner_before_first_frame_until_subscription_drop()
-> Result<(), Box<dyn std::error::Error>> {
    let (state, owner, grant) = fixture().await?;
    let mut sender = FirstFrameSink {
        directory: Some(state.owner_sessions.retain(&owner)?),
        state: state.clone(),
        owner: owner.clone(),
        frames: Vec::new(),
    };
    let mut receiver = stream::iter([Ok(Message::Text(
        r#"{"op":"subscribe","streams":["room_events"],"resume_from_seq":0}"#.into(),
    ))]);
    let subscription =
        crate::room_socket::establish(&mut sender, &mut receiver, &state, grant).await;
    assert!(subscription.is_some());
    assert!(
        sender.frames.iter().any(
            |frame| matches!(frame, Message::Text(text) if text.contains(r#""op":"snapshot""#))
        )
    );
    assert!(state.owner_sessions.require_live(&owner).is_ok());
    drop(subscription);
    assert!(state.owner_sessions.require_live(&owner).is_err());
    state.shutdown.cancel();
    state.connections.close();
    state.connections.wait().await;
    state.rooms.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cancelled_owner_ticket_sends_no_subscription_frames()
-> Result<(), Box<dyn std::error::Error>> {
    let (state, owner, grant) = fixture().await?;
    let directory = state.owner_sessions.retain(&owner)?;
    drop(directory);
    let mut sender = FirstFrameSink {
        directory: None,
        state: state.clone(),
        owner,
        frames: Vec::new(),
    };
    let mut receiver = stream::iter([Ok(Message::Text(
        r#"{"op":"subscribe","streams":["room_events"],"resume_from_seq":0}"#.into(),
    ))]);
    assert!(
        crate::room_socket::establish(&mut sender, &mut receiver, &state, grant)
            .await
            .is_none()
    );
    assert!(sender.frames.is_empty());
    state.shutdown.cancel();
    state.connections.close();
    state.connections.wait().await;
    state.rooms.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn abandoned_subscription_releases_retained_owner() -> Result<(), Box<dyn std::error::Error>>
{
    let (state, owner, grant) = fixture().await?;
    let mut sender = FirstFrameSink {
        directory: None,
        state: state.clone(),
        owner: owner.clone(),
        frames: Vec::new(),
    };
    let mut receiver = stream::empty();
    assert!(
        crate::room_socket::establish(&mut sender, &mut receiver, &state, grant)
            .await
            .is_none()
    );
    assert!(sender.frames.is_empty());
    assert!(state.owner_sessions.require_live(&owner).is_err());
    state.shutdown.cancel();
    state.connections.close();
    state.connections.wait().await;
    state.rooms.shutdown().await?;
    Ok(())
}
