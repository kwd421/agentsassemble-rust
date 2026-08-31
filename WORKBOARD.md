# WORKBOARD

Status: Phase 5 — idle pause/resume and Codex cleanup custody are approved; explicit busy-turn interrupt is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/agent-session-slice.md`](docs/specs/agent-session-slice.md#active-idle-pauseresume-extension)
- Completed prerequisite: [`docs/specs/lobby-message-mutations-slice.md`](docs/specs/lobby-message-mutations-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: connect copied `agent.interrupt` through the existing exact-turn interrupt owner,
  then prove durable replay/recovery, retained-runtime semantics, and the packaged busy-turn flow.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Source structure: LOC only signals possible ownership drift. Review at 500 lines; treat 800 lines as a strong split candidate; reject over 1,000 by default, with concrete generated-code, fixture, or declarative-data exceptions considered only when they exist. Split at differing state/invariant, domain, authority, lifecycle, or change-reason owners regardless of size; reconsider a split if it increases state transfer, public interfaces, inter-module dependency count, or obscuring glue.
- Required order: preserve the approved pause/resume and custody flow; freeze the explicit
  interrupt contract in the active owner; reuse the existing durable provider-turn effect and
  reconciliation owner; connect protocol/server/frontend surface; then verify the real flow.
- Exit: a copied busy-session control durably requests one exact interrupt, retains the provider
  runtime and conversation when quiescence proves that result, restores inflight input without an
  immediate re-run, survives replay/restart boundaries, and passes repository plus packaged
  verification.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
