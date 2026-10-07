//! Native callback custody is connection-local; secret answers are never replayed on reconnect.
use agentsassemble_domain::{
    ProviderRequestKind, ProviderRequestPrompt, ProviderRequestResolution,
};
use agentsassemble_persistence::OpenProviderRequest;
use agentsassemble_provider::{
    ProviderRequestCommand, ProviderRequestCompletion, ProviderRequestExchange,
    ProviderRequestExchangeError, ProviderRequestIngress, ProviderRequestResponder,
};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    AttendeeClientError, AttendeeSocket, AttendeeSocketFrame as Frame,
    AttendeeSocketRequest as Request,
};

#[derive(PartialEq, Eq)]
enum Phase {
    Opening,
    Waiting,
    Answered,
    LocalRejected,
    Reporting,
}

struct Pending {
    id: Uuid,
    responder: ProviderRequestResponder,
    completion: ProviderRequestCompletion,
    phase: Phase,
}

struct Outbound {
    request: Request,
    sent: bool,
    deadline: tokio::time::Instant,
}

pub(super) struct Requests {
    pub ingress: ProviderRequestIngress,
    receiver: mpsc::Receiver<ProviderRequestCommand>,
    pending: Option<Pending>,
    outbound: Option<Outbound>,
}

impl Requests {
    pub fn new() -> Self {
        let (ingress, receiver) = ProviderRequestIngress::channel(4);
        Self {
            ingress,
            receiver,
            pending: None,
            outbound: None,
        }
    }

    pub fn clear(&mut self) {
        self.pending = None;
        self.outbound = None;
        while let Ok(command) = self.receiver.try_recv() {
            command.complete(Err(ProviderRequestExchangeError::Closed));
        }
    }

    pub async fn send(&mut self, socket: &mut AttendeeSocket) -> Result<(), AttendeeClientError> {
        if let Some(outbound) = &mut self.outbound
            && !outbound.sent
        {
            socket.send(&outbound.request).await?;
            outbound.sent = true;
            outbound.deadline = tokio::time::Instant::now() + super::ACK_TIMEOUT;
        }
        Ok(())
    }

    pub fn deadline(&self) -> Option<tokio::time::Instant> {
        self.outbound
            .as_ref()
            .filter(|outbound| outbound.sent)
            .map(|outbound| outbound.deadline)
    }

    pub async fn step(&mut self) -> Result<(), AttendeeClientError> {
        tokio::select! {
            Some(command) = self.receiver.recv() => self.open(command),
            delivered = async {
                match &mut self.pending {
                    Some(pending) if pending.phase != Phase::Reporting => pending.completion.completion().await,
                    _ => std::future::pending().await,
                }
            } => {
                let pending = self.pending.as_mut().ok_or_else(|| error("attendee_provider_request_missing"))?;
                if !matches!(pending.phase, Phase::Answered | Phase::LocalRejected) {
                    return Err(error("attendee_provider_request_cancelled"));
                }
                if pending.phase == Phase::LocalRejected {
                    let pending = self.pending.take().ok_or_else(|| error("attendee_provider_request_missing"))?;
                    pending.completion.finish(Ok(()));
                    return Ok(());
                }
                pending.phase = Phase::Reporting;
                self.outbound = Some(outbound(Request::ProviderRequestDelivered {
                    request_id: Uuid::new_v4(), provider_request_id: pending.id, delivered,
                }));
            }
        }
        Ok(())
    }

    fn open(&mut self, command: ProviderRequestCommand) {
        if self.pending.is_some() {
            command.complete(Err(ProviderRequestExchangeError::Busy));
            return;
        }
        let id = command.request.provider_request_id;
        if command.request.request_kind == ProviderRequestKind::Permission {
            // No local approval UI exists. Never give the invitation issuer this authority.
            let denial = match &command.request.prompt {
                ProviderRequestPrompt::Option { options } if command.request.is_valid() => {
                    options.iter().find(|option| {
                        matches!(
                            option.kind.as_str(),
                            "reject_once" | "deny" | "decline" | "cancel"
                        )
                    })
                }
                _ => None,
            };
            let Some(denial) = denial else {
                command.complete(Err(ProviderRequestExchangeError::Closed));
                return;
            };
            let resolution = ProviderRequestResolution::Option {
                option_id: denial.id.clone(),
            };
            let (exchange, mut responder, completion) = ProviderRequestExchange::channel();
            if responder.respond(resolution).is_err() {
                command.complete(Err(ProviderRequestExchangeError::Closed));
                return;
            }
            self.pending = Some(Pending {
                id,
                responder,
                completion,
                phase: Phase::LocalRejected,
            });
            command.complete(Ok(exchange));
            return;
        }
        self.outbound = Some(outbound(Request::ProviderRequestOpen {
            request_id: Uuid::new_v4(),
            request: Box::new(OpenProviderRequest {
                turn_generation: command.turn_generation,
                execution_id: command.execution_id.clone(),
                request: command.request.clone(),
            }),
        }));
        let (exchange, responder, completion) = ProviderRequestExchange::channel();
        self.pending = Some(Pending {
            id,
            responder,
            completion,
            phase: Phase::Opening,
        });
        command.complete(Ok(exchange));
    }

    pub fn owns_reply(&self, id: Uuid) -> bool {
        self.outbound
            .as_ref()
            .is_some_and(|outbound| outbound.request.request_id() == id)
    }

    pub fn frame(&mut self, frame: Frame) -> Result<(), AttendeeClientError> {
        match frame {
            Frame::ProviderResponse {
                provider_request_id,
                resolution,
            } => {
                let pending = self
                    .pending
                    .as_mut()
                    .filter(|pending| {
                        pending.id == provider_request_id && pending.phase == Phase::Waiting
                    })
                    .ok_or_else(|| error("attendee_provider_response_mismatch"))?;
                pending
                    .responder
                    .respond(resolution)
                    .map_err(|_| error("attendee_provider_request_cancelled"))?;
                pending.phase = Phase::Answered;
            }
            Frame::ProviderRequestClosed {
                provider_request_id,
            } => {
                if self
                    .pending
                    .as_ref()
                    .is_none_or(|pending| pending.id != provider_request_id)
                {
                    return Err(error("attendee_provider_response_mismatch"));
                }
                self.clear();
            }
            Frame::Ack {
                request_id,
                resolution,
                ..
            } => {
                if !self.owns_reply(request_id)
                    || resolution != agentsassemble_protocol::CommandResolution::Committed
                {
                    return Err(error("attendee_provider_ack_mismatch"));
                }
                let pending = self
                    .pending
                    .as_mut()
                    .ok_or_else(|| error("attendee_provider_request_missing"))?;
                match pending.phase {
                    Phase::Opening => pending.phase = Phase::Waiting,
                    Phase::Reporting => {
                        let pending = self
                            .pending
                            .take()
                            .ok_or_else(|| error("attendee_provider_request_missing"))?;
                        pending.completion.finish(Ok(()));
                    }
                    Phase::Waiting | Phase::Answered | Phase::LocalRejected => {
                        return Err(error("attendee_provider_ack_mismatch"));
                    }
                }
                self.outbound = None;
            }
            Frame::Nack { request_id, .. } => {
                if !self.owns_reply(request_id) {
                    return Err(error("attendee_provider_ack_mismatch"));
                }
                self.clear();
                // A rejected/unconfirmed response must not enter generic reconnect replay.
                return Err(error("attendee_provider_request_failed"));
            }
            _ => return Err(error("attendee_provider_response_mismatch")),
        }
        Ok(())
    }
}

fn outbound(request: Request) -> Outbound {
    Outbound {
        request,
        sent: false,
        deadline: tokio::time::Instant::now(),
    }
}

fn error(code: &str) -> AttendeeClientError {
    AttendeeClientError::local(code)
}

pub(super) async fn wait_timeout(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentsassemble_domain::{
        ProviderRequest, ProviderRequestKind, ProviderRequestPrompt, ProviderRequestResolution,
    };

    #[tokio::test]
    async fn native_completion_waits_for_remote_receipt_and_connection_loss_fails_it()
    -> Result<(), Box<dyn std::error::Error>> {
        for acknowledged in [true, false] {
            let mut relay = Requests::new();
            let ingress = relay.ingress.clone();
            let id = Uuid::new_v4();
            let request = ProviderRequest {
                provider_request_id: id,
                request_kind: ProviderRequestKind::ExternalAction,
                title: "Finish external action".to_owned(),
                description: String::new(),
                timeout_seconds: 15,
                prompt: ProviderRequestPrompt::Acknowledge { action_url: None },
            };
            let (accepted, native) = tokio::join!(
                relay.step(),
                ingress.open("session", 1, "execution", request)
            );
            accepted?;
            let mut native = native?;
            relay.frame(ack(relay
                .outbound
                .as_ref()
                .ok_or("outbound missing")?
                .request
                .request_id()))?;
            relay.frame(Frame::ProviderResponse {
                provider_request_id: id,
                resolution: ProviderRequestResolution::Acknowledge,
            })?;
            assert!(matches!(
                native.receive().await?,
                ProviderRequestResolution::Acknowledge
            ));
            let completed = native.complete(true);
            tokio::pin!(completed);
            tokio::select! {
                biased;
                result = &mut completed => panic!("native completed without the remote receipt: {result:?}"),
                result = relay.step() => { result?; },
            }
            if acknowledged {
                relay.frame(ack(relay
                    .outbound
                    .as_ref()
                    .ok_or("outbound missing")?
                    .request
                    .request_id()))?;
            } else {
                relay.clear();
            }
            assert_eq!(completed.await.is_ok(), acknowledged);
        }
        Ok(())
    }

    #[tokio::test]
    async fn companion_permissions_are_denied_locally_without_remote_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        // OpenCode/Claude/ACP, Codex scope grants and Codex command/edit denials.
        for kind in ["reject_once", "deny", "decline", "cancel", "allow_once"] {
            let mut relay = Requests::new();
            let ingress = relay.ingress.clone();
            let request = ProviderRequest {
                provider_request_id: Uuid::new_v4(),
                request_kind: ProviderRequestKind::Permission,
                title: "Execute on this computer".into(),
                description: String::new(),
                timeout_seconds: 15,
                prompt: ProviderRequestPrompt::Option {
                    options: vec![agentsassemble_domain::ProviderRequestOption {
                        id: "native-choice".into(),
                        label: "Native choice".into(),
                        kind: kind.into(),
                        description: String::new(),
                    }],
                },
            };
            let id = request.provider_request_id;
            let (accepted, native) = tokio::join!(
                relay.step(),
                ingress.open("session", 1, "execution", request)
            );
            accepted?;
            assert!(
                relay.outbound.is_none(),
                "computer approval reached remote room"
            );
            if kind == "allow_once" {
                assert!(native.is_err(), "missing denial must fail closed");
                continue;
            }
            let mut native = native?;
            assert!(
                matches!(native.receive().await?, ProviderRequestResolution::Option { option_id } if option_id == "native-choice")
            );
            assert!(
                relay
                    .frame(Frame::ProviderResponse {
                        provider_request_id: id,
                        resolution: ProviderRequestResolution::Option {
                            option_id: "allow".into()
                        },
                    })
                    .is_err(),
                "remote response must not replace local rejection"
            );
            let (processed, completed) = tokio::join!(relay.step(), native.complete(true));
            processed?;
            completed?;
            assert!(relay.pending.is_none() && relay.outbound.is_none());
        }
        Ok(())
    }

    fn ack(request_id: Uuid) -> Frame {
        Frame::Ack {
            request_id,
            resolution: agentsassemble_protocol::CommandResolution::Committed,
            event_id: None,
            sequence: None,
            deduplicated: None,
        }
    }
}
