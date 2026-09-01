# WORKBOARD

Status: Phase 0A source, duplication, and defensive-complexity audits are complete;
product implementation remains paused while their plan/document corrections and
required manual reviews are closed.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: 0A audit freeze.
- Task: reconcile the complete retained-product sequence and repository findings
  with current contracts, then obtain both required manual reviews.
- Sequence/exit owner: [`docs/PRODUCT_REIMPLEMENTATION_PLAN.md`](docs/PRODUCT_REIMPLEMENTATION_PLAN.md)
- Finding/evidence owner: [`docs/architecture/REPOSITORY_AUDIT_2026-09-01.md`](docs/architecture/REPOSITORY_AUDIT_2026-09-01.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`;
  audited Rust baseline `8a5f75a`.
- Exit 0A: the complete unreviewed pushed range, individual commits, master plan,
  finding register, and aligned current contracts pass both required manual reviews.
  No product-code completion is claimed by this phase.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → this board → active phase owner.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the exact file under
  [`docs/specs/`](docs/specs/).
- Frontend or real-client verification: also read `docs/FRONTEND_BACKEND_GAPS.md`
  and `docs/VERIFICATION.md`.
- Workboard restructuring: also read `WORKBOARD_GUIDE.md`.
