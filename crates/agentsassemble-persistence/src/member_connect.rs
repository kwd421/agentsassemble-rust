//! Invitation-free selection only ever mints sessions for canonical memberships.
use crate::{
    HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission, PersistenceError, SqliteStore,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;

#[derive(Serialize)]
pub struct MemberConnectRoom {
    pub room_id: String,
    pub name: String,
}

impl SqliteStore {
    /// Returns at most fifty joined active rooms after a purpose-connect redemption.
    /// # Errors
    /// Rejects stale identity and returns database errors without creating membership.
    pub async fn member_connect_rooms(
        &self,
        member: &MemberAdmission,
        now: DateTime<Utc>,
    ) -> Result<Vec<MemberConnectRoom>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        if !member.is_current(&mut tx, now).await? {
            return Err(crate::account_identity::rejected(
                "member_challenge_invalid",
                "Member challenge is no longer current.",
            ));
        }
        let binding: Option<String> = sqlx::query_scalar(
            "SELECT binding_id FROM central_identity_bindings WHERE issuer=? AND person_id=?",
        )
        .bind(&member.issuer)
        .bind(&member.person_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(binding) = binding else {
            return Ok(vec![]);
        };
        crate::member_projection::anchor(&mut tx, &binding, member, false).await?;
        let rows=sqlx::query("SELECT m.room_id, r.room_json FROM member_admissions m JOIN participants p ON p.room_id=m.room_id AND p.participant_id=m.participant_id JOIN rooms r ON r.room_id=m.room_id WHERE m.binding_id=? AND json_extract(p.participant_json,'$.status')='joined' AND json_extract(r.room_json,'$.status')='active' ORDER BY m.room_id LIMIT 50")
            .bind(binding).fetch_all(&mut *tx).await?;
        let mut rooms = Vec::with_capacity(rows.len());
        for row in rows {
            let room: agentsassemble_domain::Room =
                serde_json::from_str(row.try_get("room_json")?)?;
            rooms.push(MemberConnectRoom {
                room_id: row.try_get("room_id")?,
                name: room.label,
            });
        }
        tx.commit().await?;
        Ok(rooms)
    }

    /// Rechecks canonical membership and stored scope in the session mint transaction.
    /// # Errors
    /// Returns persistence errors; typed admission rejections commit nothing.
    pub async fn select_member_connect_room(
        &self,
        member: &MemberAdmission,
        room: &str,
        browser: &[u8; 32],
        request_id: &str,
        client_id: &str,
        now: DateTime<Utc>,
    ) -> Result<HumanAdmissionDecision, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        if !member.is_current(&mut tx, now).await? {
            return Ok(HumanAdmissionDecision::Rejected(
                HumanAdmissionRejection::SessionUnavailable,
            ));
        }
        let row=sqlx::query("SELECT m.* FROM member_admissions m JOIN central_identity_bindings b ON b.binding_id=m.binding_id AND b.user_id=m.user_id JOIN participants p ON p.room_id=m.room_id AND p.participant_id=m.participant_id JOIN rooms r ON r.room_id=m.room_id WHERE b.issuer=? AND b.person_id=? AND m.room_id=? AND json_extract(p.participant_json,'$.status')='joined' AND json_extract(r.room_json,'$.status')='active'")
            .bind(&member.issuer).bind(&member.person_id).bind(room).fetch_optional(&mut *tx).await?;
        let Some(row) = row else {
            return Ok(HumanAdmissionDecision::Rejected(
                HumanAdmissionRejection::MemberMembershipEnded,
            ));
        };
        let decision = crate::member_admission::mint_session(
            self,
            &mut tx,
            member,
            (browser, request_id, client_id),
            row,
            now,
        )
        .await?;
        if matches!(decision, HumanAdmissionDecision::Admitted(_)) {
            tx.commit().await?;
        }
        Ok(decision)
    }
}

#[cfg(test)]
#[path = "member_connect_tests.rs"]
mod tests;
