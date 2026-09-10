//! One exact external execution survives socket replacement until its server receipt commits.
use super::{AttendeeClientError, AttendeeRuntime, provider_error};
use crate::AttendeeSocketRequest;
use agentsassemble_domain::{AgentRuntimeStatus, AgentTurnPhase, RoomInputDeliveryKind};
use agentsassemble_persistence::{AttendeeTurnDelivery, AttendeeTurnOutcome, AttendeeTurnReport};
use agentsassemble_provider::{
    ProviderAdapterError, ProviderAttachmentReadIngress, ProviderExactTurnAuthority,
    ProviderRoomObservation, ProviderRoomToolIngress, ProviderTurnCompleted, ProviderTurnOutcome,
    ProviderTurnRequest,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub struct AttendeeExecution {
    delivery: AttendeeTurnDelivery,
    authority: ProviderExactTurnAuthority,
    task: Option<JoinHandle<Result<ProviderTurnCompleted, ProviderAdapterError>>>,
    started_id: Uuid,
    report: Option<AttendeeTurnReport>,
}

impl AttendeeRuntime {
    /// Enters a newly delivered execution once under this client's existing runtime custody.
    ///
    /// # Errors
    /// Rejects another runtime, a retired generation, or an unowned reported provider turn.
    pub async fn execute(
        &mut self,
        delivery: AttendeeTurnDelivery,
        tools: ProviderRoomToolIngress,
        attachments: ProviderAttachmentReadIngress,
        requests: Option<agentsassemble_provider::ProviderRequestIngress>,
    ) -> Result<AttendeeExecution, AttendeeClientError> {
        let start = &delivery.authority;
        if self.stopped
            || self.ready.is_none()
            || !self.owns_runtime(start)
            || start.turn_generation <= self.session.turn_generation
            || start.start_dispatch_nonce.is_empty()
            || !delivery.provider_turn_id.is_empty()
        {
            return Err(AttendeeClientError::local(
                "attendee_execution_authority_mismatch",
            ));
        }
        // A recovered dispatch with no locally entered turn may start here. The live local
        // owner and strictly increasing generation prove that this process never entered it.
        self.session.turn_generation = start.turn_generation;
        self.session
            .public
            .active_turn_id
            .clone_from(&start.turn_id);
        self.session.public.turn_phase = AgentTurnPhase::Thinking;
        self.session.public.runtime_status = AgentRuntimeStatus::Busy;
        self.session.input_up_to_seq = delivery.input_up_to_seq;
        let room_observation = matches!(
            delivery.input.delivery_kind,
            RoomInputDeliveryKind::OrderedObservation | RoomInputDeliveryKind::AmbientObservation
        )
        .then(|| ProviderRoomObservation {
            session_id: start.session_id.clone(),
            input_up_to_seq: delivery.input_up_to_seq,
            view: delivery.input.room_view.clone(),
            attachment_ids: delivery.input.attachment_ids.clone(),
            attachment_ingress: (!delivery.input.attachment_ids.is_empty()).then_some(attachments),
            allowed_agent_ids: delivery.input.room_agent_ids.clone(),
            tabletop_tools: delivery.input.tabletop_tools,
            room_tool_ingress: Some(tools),
        });
        let mut input = delivery.input.provider_input.clone();
        if let Some(persona) = &self.persona {
            input.push_str("\n\n");
            input.push_str(&agentsassemble_domain::render_persona_context(
                persona,
                &delivery.input.room_view,
            ));
            if !agentsassemble_domain::is_provider_input(&input) {
                return Err(AttendeeClientError::local(
                    "attendee_persona_input_exceeds_bound",
                ));
            }
        }
        let request = ProviderTurnRequest {
            request_ingress: requests,
            turn_id: start.turn_id.clone(),
            turn_generation: start.turn_generation,
            execution_id: start.execution_id.clone(),
            input,
            room_observation,
        };
        let prepared = self
            .adapter
            .prepare_turn(&self.session, &request)
            .await
            .map_err(|error| provider_error(&error))?;
        let authority = prepared.exact_authority();
        let adapter = self.adapter.clone();
        let session = self.session.clone();
        let task = tokio::spawn(async move {
            adapter
                .send_prepared_turn(prepared, &session, &request)
                .await
        });
        Ok(AttendeeExecution {
            delivery,
            authority,
            task: Some(task),
            started_id: Uuid::new_v4(),
            report: None,
        })
    }
}

impl AttendeeExecution {
    /// Validates a recovered delivery against the exact input already owned by this client.
    #[must_use]
    pub fn matches_delivery(&self, delivery: &AttendeeTurnDelivery) -> bool {
        self.delivery.authority == delivery.authority
            && self.delivery.input == delivery.input
            && self.delivery.input_up_to_seq == delivery.input_up_to_seq
            && (delivery.provider_turn_id.is_empty()
                || self
                    .report
                    .as_ref()
                    .is_some_and(|report| report.provider_turn_id == delivery.provider_turn_id))
    }

    pub(super) fn matches_authority(
        &self,
        authority: &agentsassemble_persistence::ProviderTurnStartAuthority,
    ) -> bool {
        &self.delivery.authority == authority
    }

    pub(super) async fn drain_after_quiescence(&mut self) -> Result<(), AttendeeClientError> {
        if let Some(task) = &mut self.task {
            // Quiescence has already been positively observed; join the outer waiter as well.
            let _outcome = tokio::time::timeout(
                crate::provider_turn_interrupt_runtime::QUIESCENCE_TIMEOUT,
                task,
            )
            .await
            .map_err(|_| AttendeeClientError::local("attendee_turn_join_timeout"))?
            .map_err(|_| AttendeeClientError::local("attendee_turn_owner_unresolved"))?;
            self.task = None;
        }
        Ok(())
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    /// Awaits the owned task without consuming its result on socket-event cancellation.
    ///
    /// # Errors
    /// Lost task ownership or uncertain provider effects require cleanup, never a fabricated result.
    pub async fn complete(&mut self) -> Result<(), AttendeeClientError> {
        let Some(task) = &mut self.task else {
            return Ok(());
        };
        let result = task
            .await
            .map_err(|_| AttendeeClientError::local("attendee_turn_owner_unresolved"))?;
        self.task = None;
        if let Err(error) = &result
            && (error.effect_uncertain || error.runtime_stopped)
        {
            return Err(provider_error(error));
        }
        let (provider_turn_id, provider_session_id, outcome) = match result {
            Ok(result) => {
                if result.turn_id != self.authority.turn_id {
                    return Err(AttendeeClientError::local("attendee_turn_result_mismatch"));
                }
                let outcome = match result.outcome {
                    ProviderTurnOutcome::Message {
                        content,
                        target_agent_id,
                    } => AttendeeTurnOutcome::Message {
                        content,
                        target_agent_id,
                    },
                    ProviderTurnOutcome::Declined { reason_code } => {
                        AttendeeTurnOutcome::Declined { reason_code }
                    }
                    ProviderTurnOutcome::Vote { command } => AttendeeTurnOutcome::Vote {
                        payload: command.to_payload(),
                    },
                };
                (result.provider_turn_id, result.provider_session_id, outcome)
            }
            Err(error) => (
                String::new(),
                None,
                AttendeeTurnOutcome::Failed {
                    error_code: error.code.into_owned(),
                },
            ),
        };
        let start = &self.delivery.authority;
        self.report = Some(AttendeeTurnReport {
            request_id: Uuid::new_v4(),
            turn_id: start.turn_id.clone(),
            turn_generation: start.turn_generation,
            execution_id: start.execution_id.clone(),
            start_dispatch_nonce: start.start_dispatch_nonce.clone(),
            runtime_handle_id: start.runtime_handle_id.clone(),
            runtime_owner_id: start.runtime_owner_id.clone(),
            runtime_lease_token: start.runtime_lease_token.clone(),
            provider_turn_id,
            provider_session_id,
            outcome,
        });
        Ok(())
    }

    #[must_use]
    pub fn started_request(&self) -> Option<AttendeeSocketRequest> {
        self.report
            .as_ref()
            .filter(|report| !report.provider_turn_id.is_empty())
            .map(|report| AttendeeSocketRequest::Started {
                request_id: self.started_id,
                authority: Box::new(self.delivery.authority.clone()),
                provider_turn_id: report.provider_turn_id.clone(),
            })
    }

    #[must_use]
    pub fn report_request(&self) -> Option<AttendeeSocketRequest> {
        self.report
            .as_ref()
            .map(|report| AttendeeSocketRequest::Report {
                report: Box::new(report.clone()),
            })
    }

    /// Releases the retained provider result only after its exact report acknowledgment.
    ///
    /// # Errors
    /// Rejects a different or not-yet-produced report identity.
    pub async fn acknowledge(
        &self,
        runtime: &mut AttendeeRuntime,
        request_id: Uuid,
    ) -> Result<(), AttendeeClientError> {
        if !runtime.owns_runtime(&self.delivery.authority)
            || self.authority.turn_generation != runtime.session.turn_generation
        {
            return Err(AttendeeClientError::local("attendee_report_ack_mismatch"));
        }
        let report = self
            .report
            .as_ref()
            .filter(|report| report.request_id == request_id)
            .ok_or_else(|| AttendeeClientError::local("attendee_report_ack_mismatch"))?;
        runtime.adapter.release_terminal_turn(&self.authority).await;
        if let Some(provider_session_id) = &report.provider_session_id {
            runtime
                .session
                .provider_session_id
                .clone_from(provider_session_id);
            if let Some(ready) = &mut runtime.ready {
                ready.provider_session_id.clone_from(provider_session_id);
            }
        }
        runtime.session.public.active_turn_id.clear();
        runtime.session.public.turn_phase = AgentTurnPhase::None;
        runtime.session.public.runtime_status =
            if matches!(report.outcome, AttendeeTurnOutcome::Failed { .. }) {
                AgentRuntimeStatus::Error
            } else {
                AgentRuntimeStatus::Idle
            };
        Ok(())
    }
}
