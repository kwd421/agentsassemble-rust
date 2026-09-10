# WORKBOARD

## Active work

- Active: correct the user-rejected agent-creation login/update flow and catalog
  refresh presentation (2026-09-10). Login must follow confirmed authentication need
  during creation and return to the preserved draft; version inspection must offer
  Update/Later only when a newer version exists, with an actual supported update
  action. Remove permanent login/version/help controls from the creation form and
  Automatically inspect only the selected provider, using its own 24-hour cache;
  one manual refresh requests that provider regardless of cache age. Remove the
  whole-catalog refresh endpoint and its replaced callers and tests. Do not query
  authenticated model APIs before required authentication is available; successful
  login refreshes only that provider and preserves the creation draft.
  Automatic and manual refresh target only the requesting user's own computer's
  CLI installations and connected API accounts, never another user's or the remote
  room host's catalog merely because the user is connected to that room.
  The previous setup
  implementation and passing checks do not meet this user-flow acceptance.
  [Corrected acceptance](docs/specs/operational-surfaces-slice.md#agent-creation-setup-flow-correction-2026-09-10).
  Selected local discovery is implemented at `e980eef7`; cache/authentication boundary
  tests and signed packaged creation, refresh, existing settings and restart pass.
  Confirmed authentication-need handling now passes scoped tests and packaged
  OpenCode/Cursor creation checks; Codex's installed CLI rejects the current personal
  configuration, which remains unchanged. Optional update execution and automatic
  offers pass controlled checks and the signed OpenCode/Later keyboard flow; real
  installer/OAuth outcomes, pointer activation and independent review remain open.
  [Update evidence](docs/VERIFICATION.md#optional-update-execution-during-creation-2026-09-10).
  [Scoped evidence](docs/VERIFICATION.md#selected-local-provider-discovery-2026-09-10).
  [Authentication evidence](docs/VERIFICATION.md#authentication-need-during-creation-2026-09-10).

- Active: complete the final split Pro reviews without pausing between results,
  correct supported findings, and verify/re-review the final changes, as requested
  by the user. The immutable `a41b12a` review snapshot remains available while
  corrections continue. Signed app/helper/server startup and stored-key access after
  a different signed server build pass. The 2/64px visible-read mismatch and modal
  dismissal re-evaluation are corrected; background automation was not proof of a
  foreground WebView failure. Final affected-head reviews remain pending.
  [Signing contract](docs/specs/operational-surfaces-slice.md#signed-desktop-supervisor-and-keychain-continuity).
  Pro Group 2 completed at `7e10252`: REVISE C0/H0/M2/L0. Correct the
  external-session action mismatch and room-menu read persistence, then obtain
  affected verification and Pro re-review before continuing the remaining groups.
  Both Pro corrections pass packaged verification through `5c6199a`; Daybreak then
  identified unsupported external resident pause/resume advertising, now corrected
  with affected tests passing. User decision (2026-09-10): reuse prior Daybreak
  reviews for already-reviewed areas and continue with Pro only; its model-access
  failure does not block this scope. Pro's completed `ea10659` review closes those
  findings and identifies two further corrections: expose actual external interrupt
  capability and guard channel read actions until preferences are loaded. Both
  corrections now pass affected tests, gates and the scoped packaged checks;
  Pro `80483c2` closes M3/M4 but finds one new profile ACK/state-event projection
  mismatch (G2-M5). The persistence owner now reuses the committed event projection;
  external pre-ready/false/true outcomes, exact replay, strict ACK/socket consumption
  and signed packaged rename/restart/restoration pass. Pro `ac94b92` closes M5
  and approves that correction, but identifies a pre-existing deadline interleaving
  in profile ACK event sequencing (G2-M6). The event owner now reconciles once
  before its contiguous profile pair. Controlled open/resolving expiry, atomic
  failure/retry, public ACK/socket consumption, server integration and signed
  packaged save/restart/restoration pass. Pro's completed `e1dfd235` review closes
  M6 and approves the correction, cumulative range, whole Phase 4–6 integration
  and affected Phase 1–3 supplement at C0/H0/M0/L0; M1–M5 remain closed.
  Independent Phase 7–9 Pro review at that revision returns REVISE C0/H1/M2/L0:
  remote Connector retry custody, terminal leave receipt recovery and unconfirmed
  login cleanup retention. Remote private custody and exact terminal leave recovery
  now pass affected persistence, actual HTTP MCP and stdio verification. Login
  cleanup retention also passes controlled task-loss, retry/cancel/shutdown and
  affected transport checks. Final packaged verification and Pro re-review continue.
  The reviewer also reports incomplete historical patch reading; all individual
  Phase 7–9 commits and whole-group completion still require finished coverage.
  [Correction contract](docs/specs/final-parity-slice.md#external-session-action-ownership-correction-2026-09-10).

- Active: close out user-requested Harness/API freshness, inline OS credential
  approval, and the observed unread-state correction, then obtain Daybreak review.
  Broader mobile UI/UX is explicitly deferred by the user and remains incomplete;
  agent-selected desktop stress dimensions do not constitute mobile design acceptance.
  [Contract](docs/specs/operational-surfaces-slice.md#provider-model-freshness-2026-09-09).
  Pro sequence: supplement changed Phase 1–3 at the frozen HEAD, then 4–6, then
  7–9; brief each completed result and continue through corrections and re-review.
  [Owner](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).

- Earlier browser-to-local provider setup mechanics were locally verified:
  login/installation guidance, local setup links and optional version updates
  preserve provider/device authority. Daybreak approved all three commits,
  cumulative changes and exact `9bfc3a9` integration at C0/H0/M0/L0.
  [Verification](docs/VERIFICATION.md#browser-to-local-provider-setup-and-optional-updates-2026-09-09). [Contract](docs/specs/operational-surfaces-slice.md#browser-to-local-provider-setup-user-request-2026-09-09).
  Final split Pro review uses immutable `a41b12a` snapshots; supported corrections
  continue under the latest user instruction.

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
