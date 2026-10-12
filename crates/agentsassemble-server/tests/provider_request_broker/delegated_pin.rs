use super::*;
use agentsassemble_persistence::RoomMutationAuthority;
use serde_json::{Value, json};

#[tokio::test]
async fn guest_companion_pin_never_acquires_owner_operator_rights_and_revokes_immediately()
-> TestResult {
    let (store, human, _) = admitted_human().await?;
    let server = human_invite::start(store.clone()).await;
    let (connection, turn, bearer) = assigned_request(&store, &human).await?;
    let owner = human.principal().participant_id.clone();
    let delegate = connection.session().principal().participant_id.clone();
    crate::delegated_pin::role(&store, &owner, "director").await?;
    let current = store.revalidate_human_session_authorization(&human).await?;
    assert!(!current.principal().capabilities.room_manage);
    assert!(
        store
            .execute_participant_role_update(
                RoomMutationAuthority::HumanSession(&current),
                &Uuid::new_v4().to_string(),
                &json!({"participant_id":delegate,"role":"director"})
            )
            .await
            .is_err()
    );
    crate::delegated_pin::role(&store, &delegate, "director").await?;
    let message = store
        .snapshot("general", 0, 200)
        .await?
        .events
        .into_iter()
        .find(|e| e.content.as_deref() == Some("Request input"))
        .ok_or("target missing")?;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/api/room-attendee/tool/pin", server.base_url);
    let request = json!({"turn_generation":turn.turn_generation,"execution_id":turn.execution_id,"request":{"channel_id":"lobby","message_id":message.id,"pinned":true,"request_id":Uuid::new_v4()}});
    let send = |body: Value| {
        client
            .post(&endpoint)
            .bearer_auth(&bearer)
            .header(
                "x-attendee-connection-id",
                connection.connection_id().to_string(),
            )
            .json(&body)
            .send()
    };
    let mut discovery = request.clone();
    discovery["request"] = Value::Null;
    assert_eq!(
        send(discovery.clone()).await?.json::<Value>().await?,
        json!(true)
    );
    let first: Value = send(request.clone())
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(first["status"], "succeeded");
    assert_eq!(first["event"]["on_behalf_of_participant_id"], owner);
    assert_eq!(
        send(request.clone())
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?,
        first
    );
    let operator = crate::delegated_pin::operator();
    store
        .execute_participant_mute(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id":owner,"muted":true}),
        )
        .await?;
    assert!(send(request.clone()).await?.status().is_client_error());
    assert_eq!(
        send(discovery.clone()).await?.json::<Value>().await?,
        json!(false)
    );
    store
        .execute_participant_mute(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id":owner,"muted":false}),
        )
        .await?;
    assert_eq!(
        send(discovery.clone()).await?.json::<Value>().await?,
        json!(true)
    );
    store
        .execute_participant_removal(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            &Uuid::new_v4().to_string(),
            "participant.kick",
            &json!({"participant_id":owner}),
        )
        .await?;
    assert!(send(request.clone()).await?.status().is_client_error());
    assert_eq!(send(discovery).await?.json::<Value>().await?, json!(false));
    let events = store.snapshot("general", 0, 200).await?.events;
    assert_eq!(
        events
            .iter()
            .filter(|e| e.extra.get("request_id") == Some(&request["request"]["request_id"]))
            .count(),
        1
    );
    server.stop().await;
    Ok(())
}
