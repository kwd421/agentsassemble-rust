# WORKBOARD

## Active work

- Test/security duplication follow-up is locally complete: same-transaction reads,
  redundant codec checks, and fake test crypto removed; [evidence and retained boundaries](docs/VERIFICATION.md#test-necessity-and-security-duplication-audit-2026-09-07).
- User-requested repository-wide optimization is locally complete: measured duplicate
  work removed and creation cancellation fixed; scope, verification, and remaining finding:
  [optimization audit](docs/VERIFICATION.md#repository-wide-optimization-audit-2026-09-07).
- Phase 1 remains open: resolve the Custom API response-model contract and reconcile
  Freebuff/Antigravity's missing native receipts with actual completion evidence.
  Earlier passing checks remain evidence; they do not close these findings.
- Next: follow the [current execution checkpoint](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#ordered-implementation-phases),
  then complete whole-phase cross-review before Phase 2. Phase 0A and finite Phase 0B
  remain complete; real-provider permissions and reviewer settings are unchanged.
- Scope, acceptance, dependency order, and finding placement:
  [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#phase-1--provider-contract-and-process-correctness).
- Execution cadence and reviewer settings:
  [per-slice execution gate](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).
- Current local exit evidence:
  [packaged provider catalog and real-turn matrix](docs/VERIFICATION.md#packaged-provider-catalog-and-real-turn-matrix-2026-09-03).
- Existing authorization for this phase's real verification covers Grok `grok-4.6`,
  Codex `gpt-5.6-luna`, and OpenCode `opencode/muse-spark-1.2-contributor-free` only.
  Antigravity is excluded; unavailable evidence remains incomplete, without substitutes.

## Read routes

Read the relevant sections and owning code, not entire documents or historical logs.

- Implementation constraints: [Rule.md](Rule.md). Substantial design: [SDD.md](SDD.md).
- Scope and sequencing: [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md).
  Historical finding labels do not override its approved phase order.
- Architecture, protocol, persistence, auth, lifecycle, or cutover:
  [architecture](docs/ARCHITECTURE.md) and the corresponding contract in `docs/specs/`.
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
