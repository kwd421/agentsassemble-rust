# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

`make verify` regenerates TypeScript protocol bindings from the Rust owner, builds the React production bundle, runs the socket-client tests, verifies the isolated Tauri shell and bundled sidecar input, and then runs the Rust architecture, source-growth, formatting, check, Clippy, and test gates.

The sidecar boundary tests close the parent control pipe and prove the process exits and releases its SQLite writer lease for a restart. A watchdog regression test suspends an owned process before closing the independent parent control pipe and proves the stopped process group is force-killed. Desktop real-flow verification separately kills the owning Tauri process while its live sidecar is suspended to prove the packaged watchdog cleanup, then suspends a live sidecar with its parent active to prove unhealthy-child replacement; only the exact processes created by that verification may be signalled.

Server boundary tests also prove that pre-authentication HTTP admission is bounded, incomplete headers expire, standalone static assets carry CSP and browser hardening headers, binary WebSocket ingress is rejected, and command-line help exposes no host-secret argument or environment path.

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

## API verification scope

When a reachable flow specifically needs an API-backed provider, the allowed paid/provider-specific candidates are the official DeepSeek API and the designated Flash provider path. Every other API-backed verification uses only an explicitly free API or free model. Missing credentials, exhausted free quota, or unavailable models fail visibly; they do not trigger a paid substitution or a fallback provider.
