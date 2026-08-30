# WORKBOARD

Status: Phase 5 — canonical lobby history is packaged-verified; canonical lobby votes are active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-votes-slice.md`](docs/specs/lobby-votes-slice.md)
- Completed prerequisite: [`docs/specs/lobby-history-pagination-slice.md`](docs/specs/lobby-history-pagination-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: complete the copied lobby's canonical poll creation, ballot transitions, current summary, and provider-neutral vote tools without polling or a second vote authority.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Required order: follow the active owner from strict domain variants and the single transactional vote projection through human WebSocket/UI behavior, then the common RoomPortal and packaged provider matrix, without coupling message edit/delete or custom channels.
- Exit: local, admitted read/write, and read-only clients preserve exact poll state across reload/restart, required providers use the same vote owner, and summary reads create no mutation or background work.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
