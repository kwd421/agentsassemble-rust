# WORKBOARD

## Active work

Completed work lives in Git and `docs/VERIFICATION.md`. Add an entry here only while
work is active or waiting, and remove it when it closes.

- Active phase: Phase 9 final exposure, integration, resource measurement and
  authorized real-provider verification. [Acceptance](docs/specs/final-parity-slice.md).
  Phases 1-8 are closed (Phase 8 through `f776e2e`); closing reviews and evidence are
  linked from the product plan and `docs/VERIFICATION.md`.
- Host-owned owner sessions: deployed (Worker `80a4eabf`, rollback `a41c6fd2`) and
  accepted on signed 0.1.7 (web past six minutes with messages both ways, single
  and all device revocation, revoked reload denied); Daybreak Blue `xhigh` approved
  the corrections through `f2dda814` at C0/H0/M0/L0; signed 0.1.8 at `03756b43`
  re-verified the schema 75 -> 77 upgrade and revocation. Central logout acceptance
  was skipped by user decision.
- Planned (user direction 2026-10-05), in order after the ordered-room stall and
  desktop `/pair` fixes: (1) device pairing without the one-hour session limit,
  revocable per device from the host, shown as a QR (implemented with schema 80;
  automated evidence in `docs/VERIFICATION.md`, manual verification excluded);
  (2) Tailscale address support
  so the owner's own devices reach the host with no central request; (3) central
  request hardening: reuse an unexpired connect grant on reload and move abuse rate
  limiting off D1 writes (WAF rules later with a custom domain); (4) Discord-style
  multi-server rail: app opens and serves the local server when central is down,
  invited members bound to central identity by the host, and the account's server
  list synced across devices. End-to-end encryption over the tunnel is a later
  review item.
- Planned: 초대 멤버를 중앙 계정에 묶기 — C1 계약 및 C4a host floor 구현 ([소유 계약](docs/specs/identity-accounts-friends-slice.md#초대-멤버를-중앙-계정에-묶기--c1-승인-계약-2026-10-05)). C4a는 80 생성/81 인식만 제공하며 미배포; C2/C3 장벽·C4b migration·C5 노출·C6 검증은 남아 있다.
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
