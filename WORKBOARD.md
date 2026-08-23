# WORKBOARD

Status: Phase 2 — Agent Session foundation and provider runtime owner is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/agent-session-slice.md`](docs/specs/agent-session-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust `87a6aec05b54dcc0a840eedbe44741e218712122`. Uncommitted work is not public completion evidence.
- Required order:
  1. keep document authority and frontend provenance/parity gates accurate;
  2. implement the minimum authenticated-principal, viewer projection, public-result redaction, and shared application-command boundary;
  3. replace client create→start→resync orchestration with server-owned `agent.create(start=false|true)` and complete the lifecycle fault/replay contract;
  4. pass contract, restart, copied-UI geometry/interaction, and exact real-provider verification with owned-resource cleanup;
  5. commit and push the completed slice before web and separate manual-security review.
- Exit: the same durable visible Agent Session completes a real room conversation through each provider in the exact matrix owned by [`docs/VERIFICATION.md`](docs/VERIFICATION.md); replay, ambiguity/adoption, hidden cursor, restart, and cleanup evidence is reproducible from that public commit; and all verification-owned processes and UI resources are shut down.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, or real-provider verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
