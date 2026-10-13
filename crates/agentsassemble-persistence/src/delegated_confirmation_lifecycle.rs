//! Request transitions, exact replay and owner-private durable delivery.
use crate::delegated_confirmation::{Binding, rejected};
use crate::delegated_pin::DelegatedRoomActionAuthority;
use crate::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, ClientKind, ConfirmationState, ConfirmedRoomAction,
    DelegatedActionRequest, RoomEvent,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

pub(crate) async fn load_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    id: &str,
) -> Result<SqliteRow, PersistenceError> {
    sqlx::query("SELECT * FROM delegated_action_requests WHERE room_id=? AND confirmation_id=?")
        .bind(room_id)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| rejected("confirmation_missing", "Confirmation is unavailable."))
}

pub(crate) fn item(row: &SqliteRow) -> Result<DelegatedActionRequest, PersistenceError> {
    Ok(serde_json::from_str(row.get("request_json"))?)
}

pub(crate) async fn validate_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    row: &SqliteRow,
) -> Result<DelegatedRoomActionAuthority, PersistenceError> {
    let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
    let room = crate::authority::load_active_room(tx, room_id).await?;
    if room.room_uid != binding.room_uid {
        return Err(rejected(
            "delegate_authority_expired",
            "Room incarnation ended.",
        ));
    }
    let request = item(row)?;
    let name = match request.action {
        ConfirmedRoomAction::DeleteTextChannel(_) => "request_delete_text_channel",
    };
    let proof = DelegatedRoomActionAuthority::direct(tx, &binding.authority, name).await?;
    let (reference, _) = crate::delegation_owner::confirmation_provenance(
        tx,
        room_id,
        binding.authority.delegate_id(),
    )
    .await?;
    if proof.owner.participant_id != request.owner_participant_id
        || proof.delegate.participant_id != request.delegate_participant_id
        || reference != binding.owner_reference
    {
        return Err(rejected(
            "owner_authority_unavailable",
            "The original owner binding ended.",
        ));
    }
    Ok(proof)
}

pub(crate) async fn refresh_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    row: &SqliteRow,
) -> Result<(), PersistenceError> {
    if row.get::<&str, _>("state") != "pending" {
        return Ok(());
    }
    let (state, code) = if row.get::<i64, _>("expires_at") <= Utc::now().timestamp_millis() {
        (ConfirmationState::Expired, "confirmation_expired")
    } else {
        match validate_in(tx, room_id, row).await {
            Ok(_) => return Ok(()),
            Err(
                PersistenceError::CommandRejected { .. }
                | PersistenceError::ParticipantMissing
                | PersistenceError::RoomMissing,
            ) => (ConfirmationState::Cancelled, "confirmation_cancelled"),
            Err(error) => return Err(error),
        }
    };
    finish_in(
        tx,
        room_id,
        row,
        state,
        &json!({"status":"rejected","code":code,"retryable":false}),
    )
    .await
}

pub(crate) async fn retry_in(
    tx: &mut Transaction<'_, Sqlite>,
    authority: &DelegateRoomAuthority,
    request_id: &str,
    action: &ConfirmedRoomAction,
) -> Result<Option<Value>, PersistenceError> {
    let Some(row) = sqlx::query("SELECT * FROM delegated_action_requests WHERE room_id=? AND delegate_id=? AND request_id=?")
        .bind(authority.room_id()).bind(authority.delegate_id()).bind(request_id).fetch_optional(&mut **tx).await? else { return Ok(None); };
    let binding: Binding = serde_json::from_str(row.get("binding_json"))?;
    if item(&row)?.action != *action
        || serde_json::to_value(&binding.authority)? != serde_json::to_value(authority)?
    {
        return Err(rejected(
            "request_conflict",
            "Request identity is bound to a different action or execution.",
        ));
    }
    refresh_in(tx, authority.room_id(), &row).await?;
    let row = load_in(tx, authority.room_id(), row.get("confirmation_id")).await?;
    Ok(Some(receipt(&row)?))
}

pub(crate) fn receipt(row: &SqliteRow) -> Result<Value, PersistenceError> {
    if let Some(encoded) = row.get::<Option<String>, _>("receipt_json") {
        return Ok(serde_json::from_str(&encoded)?);
    }
    let request = item(row)?;
    Ok(
        json!({"status":"pending_confirmation","request_id":row.get::<String,_>("request_id"),"confirmation_id":request.confirmation_id,"expires_at":request.expires_at}),
    )
}

pub(crate) async fn finish_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    row: &SqliteRow,
    state: ConfirmationState,
    receipt: &Value,
) -> Result<(), PersistenceError> {
    let mut request = item(row)?;
    request.state = state;
    sqlx::query("UPDATE delegated_action_requests SET state=?,request_json=?,receipt_json=? WHERE room_id=? AND confirmation_id=?")
        .bind(serde_json::to_value(state)?.as_str()).bind(serde_json::to_string(&request)?).bind(serde_json::to_string(receipt)?)
        .bind(room_id).bind(&request.confirmation_id).execute(&mut **tx).await?;
    event_in(tx, room_id, &request, false).await?;
    // Only the correlated outcome reaches the AI; the owner card never does.
    let mut event = base_event(tx, room_id, &request, "delegated_action_outcome").await?;
    event.participant_id = Some(request.delegate_participant_id.clone());
    event.extra = [
        ("visibility".into(), json!("owner")),
        ("owner_id".into(), json!(request.delegate_participant_id)),
        (
            "request_id".into(),
            json!(row.get::<String, _>("request_id")),
        ),
        ("receipt".into(), receipt.clone()),
    ]
    .into();
    crate::room_turns::support::insert_event(tx, &event).await
}

async fn base_event(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request: &DelegatedActionRequest,
    kind: &str,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: uuid::Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(tx, room_id).await?,
        created_at: Utc::now(),
        room_id: room_id.into(),
        event_type: kind.into(),
        actor: Actor {
            participant_id: request.delegate_participant_id.clone(),
            participant_type: "agent".into(),
        },
        participant_id: Some(request.owner_participant_id.clone()),
        participant_type: Some("human".into()),
        actor_id: Some(request.delegate_participant_id.clone()),
        actor_type: Some("agent".into()),
        display_name: None,
        content: None,
        message_kind: None,
        extra: Default::default(),
    })
}

pub(crate) async fn event_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request: &DelegatedActionRequest,
    created: bool,
) -> Result<(), PersistenceError> {
    let mut event = base_event(
        tx,
        room_id,
        request,
        if created {
            "delegated_action_requested"
        } else {
            "delegated_action_updated"
        },
    )
    .await?;
    event.extra = [
        ("visibility".into(), json!("owner")),
        ("owner_id".into(), json!(request.owner_participant_id)),
        ("delegated_action_request".into(), json!(request)),
    ]
    .into();
    crate::room_turns::support::insert_event(tx, &event).await
}

pub(crate) async fn snapshot_in(
    tx: &mut Transaction<'_, Sqlite>,
    principal: Option<&AuthenticatedPrincipal>,
) -> Result<Vec<DelegatedActionRequest>, PersistenceError> {
    let Some(p) = principal.filter(|p| p.client_kind == ClientKind::Browser) else {
        return Ok(vec![]);
    };
    let rows = sqlx::query("SELECT * FROM delegated_action_requests WHERE room_id=? AND owner_id=? AND state='pending'")
        .bind(&p.room_id).bind(&p.participant_id).fetch_all(&mut **tx).await?;
    let mut requests = Vec::new();
    for row in rows {
        refresh_in(tx, &p.room_id, &row).await?;
        let current = load_in(tx, &p.room_id, row.get("confirmation_id")).await?;
        if current.get::<&str, _>("state") == "pending" {
            requests.push(item(&current)?);
        }
    }
    Ok(requests)
}

impl SqliteStore {
    /// Reconciles bounded pending requests on room transitions and their exact expiry wake.
    /// # Errors
    /// Propagates storage failures, never silently retaining ended authority.
    pub async fn reconcile_delegated_confirmations(
        &self,
        room_id: &str,
    ) -> Result<Option<DateTime<Utc>>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let rows = sqlx::query(
            "SELECT * FROM delegated_action_requests WHERE room_id=? AND state='pending'",
        )
        .bind(room_id)
        .fetch_all(&mut *tx)
        .await?;
        for row in rows {
            refresh_in(&mut tx, room_id, &row).await?;
        }
        let deadline: Option<i64> = sqlx::query_scalar("SELECT MIN(expires_at) FROM delegated_action_requests WHERE room_id=? AND state='pending'")
            .bind(room_id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(deadline.and_then(DateTime::from_timestamp_millis))
    }
}
