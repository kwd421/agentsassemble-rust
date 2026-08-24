use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{PersistenceError, SqliteStore, room_write_command_size};
use serde_json::Value;
use tokio::time::Instant;

const TRANSPORT_WINDOW: Duration = Duration::from_secs(10);
const MAX_TRANSPORT_COMMANDS_PER_WINDOW: usize = 256;
const MAX_TRANSPORT_BYTES_PER_WINDOW: usize = 2 * 1024 * 1024;
const MUTATION_WINDOW: Duration = Duration::from_mins(1);
const MAX_MUTATIONS_PER_WINDOW: usize = 3_600;
const MAX_MUTATION_BYTES_PER_WINDOW: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
struct Policy {
    window: Duration,
    max_commands: usize,
    max_payload_bytes: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            window: MUTATION_WINDOW,
            max_commands: MAX_MUTATIONS_PER_WINDOW,
            max_payload_bytes: MAX_MUTATION_BYTES_PER_WINDOW,
        }
    }
}

struct RollingBudget {
    policy: Policy,
    recent: HashMap<String, VecDeque<(Instant, usize)>>,
}

impl RollingBudget {
    fn new(policy: Policy) -> Self {
        Self {
            policy,
            recent: HashMap::new(),
        }
    }

    fn admit(&mut self, principal_id: &str, payload_bytes: usize) -> Result<(), PersistenceError> {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.policy.window).unwrap_or(now);
        let recent = self.recent.entry(principal_id.to_owned()).or_default();
        while recent.front().is_some_and(|(at, _)| *at <= cutoff) {
            recent.pop_front();
        }
        let byte_count = recent
            .iter()
            .fold(0_usize, |total, (_, bytes)| total.saturating_add(*bytes));
        if recent.len().saturating_add(1) > self.policy.max_commands
            || byte_count.saturating_add(payload_bytes) > self.policy.max_payload_bytes
        {
            return Err(PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                message: "Authenticated room write budget exceeded.".to_owned(),
            });
        }
        recent.push_back((now, payload_bytes));
        Ok(())
    }
}

pub(crate) struct PrincipalWriteBudget {
    transport: RollingBudget,
    mutations: RollingBudget,
}

impl PrincipalWriteBudget {
    pub(crate) fn new() -> Self {
        Self {
            transport: RollingBudget::new(Policy {
                window: TRANSPORT_WINDOW,
                max_commands: MAX_TRANSPORT_COMMANDS_PER_WINDOW,
                max_payload_bytes: MAX_TRANSPORT_BYTES_PER_WINDOW,
            }),
            mutations: RollingBudget::new(Policy::default()),
        }
    }

    pub(crate) fn admit_transport(
        &mut self,
        principal_id: &str,
        raw_frame_bytes: usize,
    ) -> Result<(), PersistenceError> {
        self.transport.admit(principal_id, raw_frame_bytes)
    }

    pub(crate) fn admit_mutation(
        &mut self,
        principal_id: &str,
        payload_bytes: usize,
    ) -> Result<(), PersistenceError> {
        self.mutations.admit(principal_id, payload_bytes)
    }

    pub(crate) async fn admit_command(
        &mut self,
        store: &SqliteStore,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<(), PersistenceError> {
        if !command_is_budgeted(principal, action)
            || !store
                .command_requires_principal_budget(principal, request_id, action, payload)
                .await?
        {
            return Ok(());
        }
        self.admit_mutation(
            &principal.principal_id,
            room_write_command_size(request_id, action, payload)?,
        )
    }
}

fn command_is_budgeted(principal: &AuthenticatedPrincipal, action: &str) -> bool {
    match action {
        "message.send" => principal.capabilities.message_send,
        "room.settings.update" => principal.capabilities.room_manage,
        "room.random.roll" | "room.random.choose" => principal.capabilities.room_random,
        "agent.create" | "agent.configure" | "agent.start" | "agent.resume" | "agent.stop" => {
            principal.capabilities.agent_control
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Policy, PrincipalWriteBudget, RollingBudget};

    #[test]
    fn one_principal_window_is_shared_independently_of_connections() {
        let mut budget = RollingBudget::new(Policy {
            window: Duration::from_mins(1),
            max_commands: 2,
            max_payload_bytes: 10,
        });
        budget
            .admit("operator", 4)
            .unwrap_or_else(|error| panic!("first socket write: {error}"));
        budget
            .admit("operator", 4)
            .unwrap_or_else(|error| panic!("second socket write: {error}"));
        let Err(error) = budget.admit("operator", 1) else {
            panic!("a third connection cannot shard the principal window");
        };
        assert!(matches!(
            error,
            agentsassemble_persistence::PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                ..
            }
        ));
        budget
            .admit("another-principal", 10)
            .unwrap_or_else(|error| panic!("independent principal: {error}"));
    }

    #[test]
    fn transport_window_charges_every_authenticated_command_frame() {
        let transport = RollingBudget::new(Policy {
            window: Duration::from_mins(1),
            max_commands: 2,
            max_payload_bytes: 10,
        });
        let mutations = RollingBudget::new(Policy {
            window: Duration::from_mins(1),
            max_commands: 10,
            max_payload_bytes: 100,
        });
        let mut budget = PrincipalWriteBudget {
            transport,
            mutations,
        };
        budget
            .admit_transport("operator", 5)
            .unwrap_or_else(|error| panic!("first frame: {error}"));
        budget
            .admit_transport("operator", 5)
            .unwrap_or_else(|error| panic!("second frame: {error}"));
        let Err(error) = budget.admit_transport("operator", 1) else {
            panic!("replayed frames must not bypass shared transport admission");
        };
        assert!(matches!(
            error,
            agentsassemble_persistence::PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                ..
            }
        ));
    }
}
