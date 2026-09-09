# WORKBOARD

## Active work

- Test/security duplication follow-up is locally complete: same-transaction reads,
  redundant codec checks, and fake test crypto removed; [evidence and retained boundaries](docs/VERIFICATION.md#test-necessity-and-security-duplication-audit-2026-09-07).
- User-requested repository-wide optimization is locally complete: measured duplicate
  work removed and creation cancellation fixed; scope, verification, and remaining finding:
  [optimization audit](docs/VERIFICATION.md#repository-wide-optimization-audit-2026-09-07).
- Custom API response-model correction is locally complete:
  [adapter-path verification](docs/VERIFICATION.md#custom-api-resolved-model-contract-2026-09-07).
- User scope revision: Freebuff is excluded; Antigravity is external CLI plus
  Room Connector invite only (Phase 7). Both managed registrations and Antigravity's
  resident path are retired; the fourteen-provider Phase 1 passes local verification
  and Daybreak's whole-phase review through `cae1f90`.
- Codex native terminal-status correction passes affected local verification;
  [failure/publication and interrupt evidence](docs/VERIFICATION.md#codex-native-terminal-status-correction-2026-09-07).
- Phase 1 is closed through `2a49599`: Pro's three supported findings are resolved;
  Daybreak approved all 136 commits, cumulative phase, final HEAD and complete local
  contract. [Disposition and verification](docs/VERIFICATION.md#completed-pro-review-corrections-2026-09-07).
- Phase 2 is closed through `5c8d17b`: Daybreak approved every individual commit,
  cumulative phase, final HEAD and complete local contract at C0/H0/M0/L0.
  [Evidence](docs/VERIFICATION.md#phase-2-exact-agent-session-controls-2026-09-07).
- Phase 3 is closed through `b8fd15b`: Agent identity, avatar custody and editor
  pass local acceptance and whole-phase Daybreak review at C0/H0/M0/L0.
  [Contract and acceptance](docs/specs/agent-profile-slice.md).
- Phase 4 is closed through `53a82f1`: Daybreak approved every final phase correction,
  cumulative phase, final HEAD and whole local contract at C0/H0/M0/L0. The one
  late-moderation terminal-export finding is closed; direct packaged desktop/mobile
  settings, profile, controls, moderation and room lifecycle are verified.
  [Contract](docs/specs/room-lifecycle-slice.md) and [review correction](docs/VERIFICATION.md#phase-4-whole-phase-review-correction-terminal-export-2026-09-08).
- Phase 5 is closed through `1e24adf`: Daybreak approved both Google corrections,
  the cumulative 44-commit phase, exact HEAD and whole local contract at C0/H0/M0/L0.
  [Review disposition](docs/VERIFICATION.md#phase-5-whole-phase-review-corrections-2026-09-08)
  and [account/social/human contract](docs/specs/identity-accounts-friends-slice.md).
- Phase 6 is closed through `25c7961`: Daybreak approved all 17 commits,
  cumulative phase, exact HEAD and complete local contract at C0/H0/M0/L0.
  [Correction and approval](docs/VERIFICATION.md#phase-6-whole-phase-review-correction-http-incarnation-2026-09-08).
- Phase 7 is closed through `d70224b`: Daybreak approved the correction, cumulative
  95-commit phase, exact HEAD and whole local contract at C0/H0/M0/L0. All three
  custody paths, packaged desktop/mobile and Windows process/IPC verification pass.
  [Correction](docs/VERIFICATION.md#phase-7-whole-phase-review-correction-muted-owner-2026-09-09).
  [Contract and evidence](docs/specs/external-ai-admission-slice.md).
  New Pro review and authorized real-provider proof remain at full closeout.
- Phase 8 is closed through `f776e2e`: Daybreak approved its usage correction,
  cumulative phase, exact HEAD and whole local contract at C0/H0/M0/L0.
  [Contract](docs/specs/operational-surfaces-slice.md) and [correction](docs/VERIFICATION.md#phase-8-whole-phase-review-correction-usage-freshness-2026-09-09).
- Active: Phase 9 final exposure, integration, resource measurement and authorized
  real-provider verification. [Acceptance](docs/specs/final-parity-slice.md).
- Scope, acceptance, dependency order, and finding placement:
  [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#phase-1--provider-contract-and-process-correctness).
- Execution cadence and reviewer settings:
  [per-slice execution gate](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).
- Current local exit evidence:
  [packaged provider catalog and real-turn matrix](docs/VERIFICATION.md#packaged-provider-catalog-and-real-turn-matrix-2026-09-03).
- Final real verification, after reimplementation: configured DeepSeek, Codex,
  OpenCode, external Antigravity, Grok, and Cursor `auto` only. Other providers use
  original-contract implementation and local verification, without real execution.
  Packaged frontend flows are verified by direct app manipulation in every phase;
  final real-provider flows add integrated coverage. Local tests, mandatory gates,
  and phase code reviews continue during implementation.

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

Earlier board records remain in Git at `be63b1e4e6031853a3a666bb7deddd82781ce43d`:
`git show be63b1e4e6031853a3a666bb7deddd82781ce43d:WORKBOARD.md`.
Consult them only for a specific historical finding or review; current contracts,
execution evidence, and active state remain with the owners linked above.
