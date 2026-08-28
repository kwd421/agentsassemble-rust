# WORKBOARD

Status: Phase 5 — lobby message pins are fully verified and manually approved; the lobby message-attachment cutover is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-message-attachments-slice.md`](docs/specs/lobby-message-attachments-slice.md)
- Completed prerequisite: [`docs/specs/message-pins-slice.md`](docs/specs/message-pins-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust lobby-pin baseline `6d0394d`.
- Active gate: implement ordinary lobby attachments from exact pending upload through atomic message binding, authorized copied-frontend rendering, and canonical-message-bound Agent Session reads; adjacent channel, vote, mutation, search, and paging surfaces remain unavailable.
- Review cadence: commit each independent change below 1,000 changed lines; push and cross-review at three completed product features or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: follow the active owner from shared absolute accounting and message-owned persistence through real local/remote frontend and provider flows without adding adjacent message authority in advance.
- Exit: local and remote humans use durable lobby attachments through their actual permissions, ordered/ambient agents read only canonical referenced media, read-only denial and restart behavior pass, and incomplete adjacent surfaces remain visibly unavailable.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
