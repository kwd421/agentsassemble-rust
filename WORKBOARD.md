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
- Active gate: finish complete lobby-message search and bounded context through exact-turn RoomPortal reads and real packaged clients. Canonical persistence, purpose-bound current-human HTTP reads, and the copied-frontend source cutover are implemented; custom-channel search remains explicitly unavailable.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Required order: follow the active owner from the implemented current-human/frontend boundary through exact-turn RoomPortal reads, then packaged local/read-only and configured real-Agent verification, without adding a generic search/provider framework.
- Exit: complete lobby history, strict pagination/context, local/read-only human authority, copied packaged navigation/restart, and the configured real Agent matrix are verified; private event fields and incomplete custom-channel data never cross the boundary.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
