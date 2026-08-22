# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

## Frontend real-flow cleanup

When Computer Use is used for frontend verification, every resource created solely for that verification is shut down after its evidence is collected:

- controlled browser tabs and windows;
- test-only desktop application instances;
- local runtime/server processes started by the verification;
- test-only Agent Sessions and provider processes.

Cleanup resolves exact owned process and session identities before stopping them. It never closes user-owned tabs, applications, providers, or unrelated processes. Cleanup failure is reported and is not treated as a clean run.

## Real Agent matrix

Frontend flows that require real Agent Sessions use exactly this matrix:

- Codex: Terra;
- Antigravity: Flash;
- OpenCode: the free Hy3 model.

The verification records the exact provider/model identifiers exposed by the installed runtime at execution time. Missing login, unavailable capability, unsupported model, or provider failure remains visible as failed or `unknown`; it never triggers model substitution, a mock pass, or a fallback provider.

Provider credentials, private conversation state, hidden reasoning, and provider-private identifiers are excluded from screenshots, logs, fixtures, public events, and committed artifacts.

