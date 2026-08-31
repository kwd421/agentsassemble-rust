# WORKBOARD

Status: Phase 5 — canonical lobby votes are packaged/provider-verified; lobby message mutations are active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-message-mutations-slice.md`](docs/specs/lobby-message-mutations-slice.md)
- Completed prerequisite: [`docs/specs/lobby-votes-slice.md`](docs/specs/lobby-votes-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: complete canonical lobby message edit and message/poll deletion with one atomic history, search, pin, vote, and attachment-lifecycle owner.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Source structure: LOC only signals possible ownership drift. Review at 500 lines; treat 800 lines as a strong split candidate; reject over 1,000 by default, with concrete generated-code, fixture, or declarative-data exceptions considered only when they exist. Split at differing state/invariant, domain, authority, lifecycle, or change-reason owners regardless of size; keep a cohesive large flow when splitting would add state transfer and obscuring glue.
- Required order: follow the active owner from strict mutation contracts through one persistence transaction, WebSocket/product-surface exposure, copied controls, and packaged local/remote verification without coupling custom channels or provider orchestration.
- Exit: local and admitted clients preserve exact edited/tombstoned state across live delivery, reload, and restart; search, pins, votes, and attachments agree without leaked deleted data or background cleanup.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
