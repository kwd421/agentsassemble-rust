# WORKBOARD

Status: Phase 5 — lobby message attachments are fully verified and manually approved; the persona-card library and Agent Session selection cutover is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/persona-card-library-slice.md`](docs/specs/persona-card-library-slice.md)
- Completed prerequisite: [`docs/specs/lobby-message-attachments-slice.md`](docs/specs/lobby-message-attachments-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust lobby-attachment baseline `27b2c07`.
- Active gate: implement local-operator Risu/CCv3/CHARX import, safe library projection and thumbnail reads, exact Agent Session selection/configuration, and bounded provider-neutral ordinary-turn prompt application; executable card features and the v0 scripted-meeting pipeline remain unavailable.
- Review cadence: commit each independent change below 1,000 changed lines; push and cross-review at three completed product features or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: follow the active owner from normalized private import storage and safe local-operator reads through Agent Session persistence and durable turn-context application without adding a generic import/provider framework.
- Exit: representative supported assets import through the real copied picker, selection/configuration survives restart, ordered/ambient agents receive bounded literal persona context, executable features remain inert, and incomplete adjacent surfaces remain visibly unavailable.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
