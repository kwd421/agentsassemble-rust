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
- Task: D-02 — remove the unapproved local/remote receipt and per-frame proof layers
  while preserving the finite snapshot/catch-up and ordinary bounded JSON contract.
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
