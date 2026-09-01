# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity audit and planning
review are closed at reviewed content checkpoint `9711232`. Phase 0B foundation
correction is active from public baseline `4ab5ee1`.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: 0B foundation correction.
- Completed: D-01 at `a7949bd`; the uncalled HTTP challenge/ticket bootstrap and
  startup secret are absent, while private-control and admitted-human socket ticket
  issuance remain.
- Completed: D-02 at `3ffb9eb`, `77cae0e`, `0d24741`, and `57fd6ec`; the
  evidence-free receipt, digests, per-frame HMAC/base64/counter envelope, proof-key
  ticket state, and obsolete test vocabulary are absent. One-use ticket authority,
  strict bounded JSON, finite snapshot/catch-up, replay, and failure contracts remain.
- Task: D-03 — remove the redundant human-session HTTP purpose-ticket exchange while
  preserving one bounded-header authorization at each target route. Desktop purpose
  tickets and one-use WebSocket upgrade tickets remain because they cross distinct
  authority boundaries.
- Completed D-03 targets through `9bfee34`: profile, preferences, message pins,
  message search, message attachments, and bound room-appearance reads authorize
  reusable remote sessions at the target; the obsolete socket-to-profile authority
  interpretation and public HTTP-purpose exchange state are absent. The three-feature
  batch beginning after `ac905de`, including its current review corrections, is pending
  manual cross-review.
- Sequence/exit owner: [`docs/PRODUCT_REIMPLEMENTATION_PLAN.md`](docs/PRODUCT_REIMPLEMENTATION_PLAN.md)
- Finding/evidence owner: [`docs/architecture/REPOSITORY_AUDIT_2026-09-01.md`](docs/architecture/REPOSITORY_AUDIT_2026-09-01.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`;
  audited Rust baseline `8a5f75a`.
- Exit 0A: satisfied. The complete planning range, master plan, finding register,
  and aligned current contracts received critical-web Pro and Daybreaker Blue High
  manual approval at `C0/H0/M0/L0`. No product-code completion is claimed by this
  phase.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → this board → active phase owner.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the exact file under
  [`docs/specs/`](docs/specs/).
- Frontend or real-client verification: also read `docs/FRONTEND_BACKEND_GAPS.md`
  and `docs/VERIFICATION.md`.
- Workboard restructuring: also read `WORKBOARD_GUIDE.md`.
