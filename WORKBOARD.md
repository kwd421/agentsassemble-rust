# WORKBOARD

Status: Phase 5 — persona-card import, Agent Session selection, and prompt application are fully verified and manually approved; lobby message search and context are active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-message-search-slice.md`](docs/specs/lobby-message-search-slice.md)
- Completed prerequisite: [`docs/specs/persona-card-library-slice.md`](docs/specs/persona-card-library-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: implement complete lobby-message search and bounded context for current humans and Agent Sessions through one canonical persistence owner, purpose-bound HTTP authority, the copied frontend, and RoomPortal; custom-channel search remains unavailable.
- Review cadence: commit each independent change below 1,000 changed lines; push and cross-review at three completed product features or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: follow the active owner from measured derived-index custody through current-human HTTP reads, copied-frontend cutover, and exact-turn RoomPortal reads without adding a generic search/provider framework.
- Exit: complete lobby history, strict pagination/context, local/read-only human authority, copied packaged navigation/restart, and the configured real Agent matrix are verified; private event fields and incomplete custom-channel data never cross the boundary.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
