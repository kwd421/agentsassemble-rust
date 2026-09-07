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
- Next: implement Phase 2's missing Agent Session controls using the
  [current execution checkpoint](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#ordered-implementation-phases).
  Phase 1's local exit is complete; first read the running Pro result and resolve
  supported findings with Daybreak review. Subsequent phases use Daybreak only.
  New Pro review waits until full reimplementation, under the per-slice gate below.
- Scope, acceptance, dependency order, and finding placement:
  [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#phase-1--provider-contract-and-process-correctness).
- Execution cadence and reviewer settings:
  [per-slice execution gate](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).
- Current local exit evidence:
  [packaged provider catalog and real-turn matrix](docs/VERIFICATION.md#packaged-provider-catalog-and-real-turn-matrix-2026-09-03).
- Final real verification, after reimplementation: configured DeepSeek, Codex,
  OpenCode, external Antigravity, Grok, and Cursor `auto` only. Other providers use
  original-contract implementation and local verification, without real execution.
  Packaged frontend and real-provider flows are verified together at the final stage;
  local tests, mandatory gates, and phase code reviews continue during implementation.

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
