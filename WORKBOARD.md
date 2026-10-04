# WORKBOARD

## Active work

Completed work lives in Git and `docs/VERIFICATION.md`. Add an entry here only while
work is active or waiting, and remove it when it closes.

- Active phase: Phase 9 final exposure, integration, resource measurement and
  authorized real-provider verification. [Acceptance](docs/specs/final-parity-slice.md).
  Phases 1-8 are closed (Phase 8 through `f776e2e`); closing reviews and evidence are
  linked from the product plan and `docs/VERIFICATION.md`.
- Waiting for user approval: host-owned owner sessions. Local work is complete and
  pushed through `77603a81`; central Worker change `25fad46a` is on
  `codex/owner-session-renewal` (local only; production already has migrations
  0001-0008, so no new migration). Still needed: production Worker deployment, signed
  build, actual app/web acceptance (use past five minutes with messages, logout,
  single/all device revocation) and the required external review. Contract:
  identity-accounts-friends slice -> Host-owned owner workspace and devices.
- Open verification: Windows physical-device checks (recent Discord-style UI,
  server icons, interactive install/update `desktop-v0.1.4`, attendee CLI); full
  app/web parity rows in the final-parity slice; persistent web MCP endpoint
  deployment. Details and limits are in the matching `docs/VERIFICATION.md` sections.
- Scope, acceptance, dependency order, and finding placement:
  [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#phase-1--provider-contract-and-process-correctness).
- Execution cadence and reviewer settings:
  [per-slice execution gate](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).
- Current local exit evidence:
  [packaged provider catalog and real-turn matrix](docs/VERIFICATION.md#packaged-provider-catalog-and-real-turn-matrix-2026-09-03).
- Final real verification, after reimplementation: configured DeepSeek, Codex,
  OpenCode, external Antigravity, Grok, and Cursor `auto` only. Other providers use
  original-contract implementation and local verification, without real execution.

## Read routes

Read the relevant sections and owning code, not entire documents or historical logs.

- Implementation constraints: [Rule.md](Rule.md). Substantial design: [SDD.md](SDD.md).
- Scope and sequencing: [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md).
  Historical finding labels do not override its approved phase order.
- Architecture, protocol, persistence, auth, lifecycle, or cutover:
  [architecture](docs/ARCHITECTURE.md) and the corresponding contract in `docs/specs/`.
- Frontend changes: read [frontend UX guide](docs/FRONTEND_UX_GUIDE.md) before editing.
- Frontend or real-client verification: the affected entry in
  [frontend gaps](docs/FRONTEND_BACKEND_GAPS.md) and [verification](docs/VERIFICATION.md).
- Finding history: [repository audit](docs/architecture/REPOSITORY_AUDIT_2026-09-01.md).
- Artifact maintenance: existing `make artifact-check` / `make artifact-prune` owner;
  preserve the current limits and do not clean while Cargo/Tauri work is active.
- Board creation or restructuring: [WORKBOARD_GUIDE.md](WORKBOARD_GUIDE.md).

## Historical evidence

Earlier board records remain in Git: `git show 77603a81:WORKBOARD.md` (2026-09-16 to
2026-10-04 task notes) and `git show be63b1e4e6031853a3a666bb7deddd82781ce43d:WORKBOARD.md`
(before that).
Consult them only for a specific historical finding or review; current contracts,
execution evidence, and active state remain with the owners linked above.
