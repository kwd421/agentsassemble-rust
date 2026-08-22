# WORKBOARD

Status: Phase 2 — Agent Session and provider runtime owner is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/agent-session-slice.md`](docs/specs/agent-session-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Exit: the same durable visible Agent Session completes a real room conversation through each provider in the exact matrix owned by [`docs/VERIFICATION.md`](docs/VERIFICATION.md), then all verification-owned processes and UI resources are shut down.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, or real-provider verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
