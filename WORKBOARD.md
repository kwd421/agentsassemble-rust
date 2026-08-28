# WORKBOARD

Status: Phase 5 — room appearance is fully verified and manually approved; the lobby message-pin cutover is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/message-pins-slice.md`](docs/specs/message-pins-slice.md)
- Completed prerequisite: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust lobby-pin backend baseline `d940313`. The copied frontend cutover remains active.
- Active gate: implement the lobby pin contract from its durable pointer through exact local/remote HTTP authority and the copied frontend; adjacent message surfaces remain unavailable.
- Review cadence: commit each independent change below 1,000 changed lines; push and cross-review at three completed product features or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: follow the active owner from persistence to real frontend flow without adding search/history/channel authority in advance.
- Exit: local and remote humans use durable lobby pins through their actual permissions, read-only denial and restart behavior pass, and incomplete adjacent surfaces remain visibly unavailable.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
