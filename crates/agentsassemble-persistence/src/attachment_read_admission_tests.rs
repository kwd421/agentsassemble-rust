//! Trace BLOB selection at the driver boundary, without logging bound values.
use crate::{PersistenceError, ProviderAttachmentReadAuthority, RoomMutationAuthority};
use agentsassemble_domain::{ClientKind, public_settings};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tracing::{
    Instrument,
    field::{Field, Visit},
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct BlobQueries(Arc<AtomicUsize>);
impl<S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>> Layer<S>
    for BlobQueries
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        if event.metadata().target() == "sqlx::query"
            && ctx
                .lookup_current()
                .is_some_and(|span| span.scope().any(|s| s.name() == "attachment_read_probe"))
        {
            event.record(&mut BlobSelection(&self.0));
        }
    }
}
struct BlobSelection<'a>(&'a AtomicUsize);
impl Visit for BlobSelection<'_> {
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
    fn record_str(&mut self, field: &Field, sql: &str) {
        if field.name() == "db.statement"
            && sql
                .trim_start()
                .starts_with("SELECT substr(chunk.content,1,65536)")
        {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn download_refusal_and_oversized_ai_reads_never_select_the_blob()
-> Result<(), Box<dyn std::error::Error>> {
    let queries = Arc::new(AtomicUsize::new(0));
    tracing_subscriber::registry()
        .with(BlobQueries(queries.clone()))
        .try_init()?;
    let (store, principal, _directory) = super::super::fixture().await;
    let settings = store.snapshot("general", 0, 100).await?.settings;
    store.execute_room_settings_update(RoomMutationAuthority::TrustedPrincipal(&principal), "large-policy",
        &json!({"expected_revision":public_settings(&settings)?.settings_revision,"attachment_file_limit_bytes":52428800})).await?;
    let attachment = store
        .store_message_attachment(
            &principal,
            "large.bin",
            "application/octet-stream",
            vec![42; 50 * 1024 * 1024],
        )
        .await?;
    let small = store
        .store_message_attachment(
            &principal,
            "small.bin",
            "application/octet-stream",
            vec![7; 65537],
        )
        .await?;
    let sent = store
        .execute_message_with_turn(
            &principal,
            "large-message",
            "message.send",
            &json!({"content":"@Terra inspect", "attachment_ids":[attachment.id, small.id]}),
        )
        .await?;
    let assignment = sent.assignments.first().ok_or("assignment")?;
    store
        .authorize_provider_turn_start(
            &assignment.session.public.room_id,
            &assignment.session.public.session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await?;
    let span = tracing::info_span!("attachment_read_probe");
    let refused = store
        .bound_message_attachment(
            "general",
            &principal.principal_id,
            &principal.participant_id,
            &attachment.id,
            |id, size| {
                assert_eq!(id, principal.principal_id);
                assert_eq!(size, 52428800);
                Err::<(), _>(PersistenceError::CommandRejected {
                    code: "attachment_download_budget_exhausted".into(),
                    message: "Busy".into(),
                })
            },
        )
        .instrument(span.clone())
        .await;
    assert!(
        matches!(refused, Err(PersistenceError::CommandRejected { code, .. }) if code == "attachment_download_budget_exhausted")
    );
    let provider = store
        .bound_provider_message_attachment(
            ProviderAttachmentReadAuthority {
                room_id: &assignment.session.public.room_id,
                session_id: &assignment.session.public.session_id,
                turn_id: &assignment.turn_id,
                input_up_to_seq: assignment.session.input_up_to_seq,
                turn_generation: assignment.turn_generation,
                execution_id: &assignment.execution_id,
            },
            &attachment.id,
            |_, _| Ok(()),
        )
        .instrument(span.clone())
        .await;
    assert_oversized(provider.err().ok_or("provider accepted")?);
    let provider_refused = store
        .bound_provider_message_attachment(
            ProviderAttachmentReadAuthority {
                room_id: &assignment.session.public.room_id,
                session_id: &assignment.session.public.session_id,
                turn_id: &assignment.turn_id,
                input_up_to_seq: assignment.session.input_up_to_seq,
                turn_generation: assignment.turn_generation,
                execution_id: &assignment.execution_id,
            },
            &small.id,
            |id, size| {
                assert_eq!(id, assignment.session.public.session_id);
                assert_eq!(size, small.size);
                Err::<(), _>(busy())
            },
        )
        .instrument(span.clone())
        .await;
    assert_busy(provider_refused.err().ok_or("provider budget accepted")?);
    let mut connector = principal.clone();
    connector.client_kind = ClientKind::RoomConnector;
    let result = store
        .bound_authorized_message_attachment(
            RoomMutationAuthority::TrustedPrincipal(&connector),
            &attachment.id,
            |_, _| -> Result<(), PersistenceError> {
                panic!("reader size gate precedes content admission")
            },
        )
        .instrument(span.clone())
        .await;
    assert_oversized(result.err().ok_or("connector accepted")?);
    let refused = store
        .bound_authorized_message_attachment(
            RoomMutationAuthority::TrustedPrincipal(&connector),
            &small.id,
            |id, size| {
                assert_eq!(id, connector.principal_id);
                assert_eq!(size, small.size);
                Err::<(), _>(busy())
            },
        )
        .instrument(span.clone())
        .await;
    assert_busy(refused.err().ok_or("connector budget accepted")?);
    attendee_refusal(span.clone()).await?;
    assert_eq!(
        queries.load(Ordering::SeqCst),
        0,
        "no BLOB query or response allocation on refusal"
    );
    let (read, ()) = store
        .bound_message_attachment(
            "general",
            &principal.principal_id,
            &principal.participant_id,
            &attachment.id,
            |_, _| Ok(()),
        )
        .instrument(span)
        .await?;
    assert_eq!(read.content.len(), 52428800);
    assert!(read.content.iter().all(|byte| *byte == 42));
    assert_eq!(
        queries.load(Ordering::SeqCst),
        800,
        "successful read proves driver probe is live"
    );
    Ok(())
}

fn assert_oversized(error: PersistenceError) {
    let PersistenceError::CommandRejected { code, message } = error else {
        panic!("specific size rejection required");
    };
    assert_eq!(code, "attachment_too_large");
    assert!(message.contains("52428800") && message.contains("10485760"));
}

fn busy() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "attachment_download_budget_exhausted".into(),
        message: "Busy".into(),
    }
}
fn assert_busy(error: PersistenceError) {
    assert!(
        matches!(error, PersistenceError::CommandRejected { code, .. } if code == "attachment_download_budget_exhausted")
    );
}
async fn attendee_refusal(span: tracing::Span) -> Result<(), Box<dyn std::error::Error>> {
    use crate::{AttendeeToolRead, AttendeeToolReadRequest};
    let (store, session, now) = crate::attendee::connection_tests::fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, uuid::Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &crate::attendee::ready_tests::report(), now)
        .await?;
    let human = store
        .authorize_human_session(
            &crate::human_session_authority_tests::session_fingerprint(&store).await,
        )
        .await?;
    let attachment = store
        .store_authorized_message_attachment(
            RoomMutationAuthority::HumanSession(&human),
            "input.txt",
            "text/plain",
            b"input".to_vec(),
        )
        .await?;
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &uuid::Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Read", "attachment_ids":[attachment.id]}),
        )
        .await?;
    let turn = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn")?;
    let request = AttendeeToolReadRequest {
        turn_generation: turn.authority.turn_generation,
        execution_id: turn.authority.execution_id,
        tool: AttendeeToolRead::Attachment {
            attachment_id: attachment.id,
        },
    };
    let refused = store
        .read_attendee_tool(
            session.session_fingerprint(),
            connection.connection_id(),
            &request,
            now,
            |id, size| {
                assert_eq!(id, session.principal().principal_id);
                assert_eq!(size, 5);
                Err::<(), _>(busy())
            },
        )
        .instrument(span)
        .await;
    assert_busy(refused.err().ok_or("attendee budget accepted")?);
    Ok(())
}
