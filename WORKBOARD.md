# WORKBOARD

Status: Phase 5 — lobby message search and context are fully verified; canonical lobby history pagination is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-history-pagination-slice.md`](docs/specs/lobby-history-pagination-slice.md)
- Completed prerequisite: [`docs/specs/lobby-message-search-slice.md`](docs/specs/lobby-message-search-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: complete the copied lobby's canonical pre-cursor history page through the authenticated room WebSocket without treating the read as a mutation or exceeding the existing frame boundary.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Required order: follow the active owner from protocol and current-human persistence read through strict frontend acceptance, then packaged local/read-only verification, without coupling message mutation, read cursors, or custom channels.
- Exit: local and admitted read-only clients page the complete public lobby history with strict cursor, projection, frame, retry, and anchor behavior; the read creates no mutation state or background work.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
