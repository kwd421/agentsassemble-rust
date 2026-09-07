# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity inventory at
`9711232` remains reviewed historical evidence. Its finding-number order is not
the production roadmap. The corrected provider-first plan is approved and active.
The finite Phase 0B prerequisites F-14 and F-16 and their whole-phase cross-review
are complete. Phase 1 provider-first implementation is complete at the local
candidate and awaits its whole-phase external cross-review.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: Phase 1 provider-first review candidate. The finite Phase 0B prerequisites
  and their complete local gate and whole-phase external cross-review are finished.
- Historical Phase 0B labels are not a serial global gate. D-06 executes with Phase 1
  runtime measurement/hardening; D-07 executes with Phase 5 human admission; split
  F-18/F-20 work remains with its already named external-admission/custom-channel
  owners; C-01 executes before Phase 4's first participant mutation, and F-19/C-09
  execute with Phase 4 room settings. C-13 splits between Phase 1 provider custody
  and Phase 5 human identity/admission; C-10 splits between Phase 8 operational
  surfaces and Phase 9's final inactive-surface proof. Reordering does not waive a
  finding; its owning phase cannot exit before the finding is closed or
  evidence-deferred.
- Current phase execution: establish the whole phase's dependency skeleton and
  acceptance matrix first, implement the smallest shared owners, connect one complete
  vertical flow for every target in dependency order, then harden and verify the phase
  as a whole. Do not keep polishing or defensively expanding one provider/feature while
  sibling targets remain structurally absent. Once a slice meets its phase contract,
  move to the next dependency unless concrete evidence reopens it.
- Current review cadence: complete and verify one whole implementation phase, then
  request one thorough cross-review of every individual commit in that phase, its
  cumulative range, final HEAD, and resulting product flow. Critical-web stays on
  Pro; the source/security reviewer uses Daybreak Blue at `xhigh`. Do not switch the
  web reviewer to very-high. Review-required corrections are re-reviewed before the
  phase closes. This latest user direction replaces the earlier per-batch external
  review cadence; independent sub-1,000-line commits remain mandatory.
- Completed pending Phase 1 whole-phase review: Codex exact terminal completion at
  `16ebb1f`. The audited driver could convert a final message followed by thread-idle
  into success after a one-second grace timer, before the provider supplied its
  terminal success or failure. The Codex turn owner now accepts only the exact
  `turn/completed` receipt; final-message, item-completed, and thread-status events
  remain nonterminal. This removes the grace timer and inferred-completion state
  without changing the three-minute read-inactivity bound, cancellation, output
  bounds, or RoomPortal publication authority. The change removes 42 net lines and
  adds no polling, retry, fallback, heartbeat, background task, or silent failure.
  The exact receipt predicate rejects the three former inference inputs, the real
  turn fixture still completes through one `turn/completed`, and all 197 provider
  tests, warning-denied provider Clippy, formatting, diff, architecture, and source-
  structure gates pass. No real provider turn was run for this correction.
- Completed the OpenCode portion of F-02 at `9c39938`. Installed OpenCode 1.17.18's
  official schema and tagged handler make `POST /session/:id/message` the blocking
  assistant-message result and the GET endpoint a history listing. The Rust driver
  now uses only the POST response for completion identity/content; SSE remains an
  exact request/model/terminal-state cross-check. Empty direct content fails visibly
  instead of issuing a history fallback. The change adds no test, state, abstraction,
  polling, retry, timer, heartbeat, or compatibility path. Three existing focused
  decoder tests, provider all-target check, warning-denied provider Clippy,
  formatting, architecture/policy/source-structure, and diff gates pass. F-02 is
  complete pending the Phase 1 whole-phase reviews; no real provider turn was run.
- Completed the observation-abort portion of F-03 at `fa8fac8`; portal teardown,
  hook cleanup, and residual-process reporting remain open. Codex, OpenCode, Claude
  Agent SDK, the shared remote OpenAI runtime, and shared Cursor/Grok ACP runtime now
  return their RoomPortal abort result instead of discarding it. ACP always attempts
  both permission-gate deactivation and portal close, even if either fails. The common
  turn owner also closes a partially begun observation and treats any abort failure as
  restart-required, then uses the existing exact stop/lease/tombstone path before
  returning; a normal definitive provider failure with successful cleanup still keeps
  its runtime. This addresses the concrete stale-observation/permission threat without
  a new state machine, timer, retry, polling, fallback, background task, or alternate
  cleanup path. The focused failed-abort test proves one exact stop and no runtime
  reuse; all 198 provider tests, warning-denied provider Clippy, formatting, diff,
  architecture, policy, and source-structure gates pass. No real provider ran.
- Completed the catalog-probe process portion of F-03 at `c429173`; explicit portal
  teardown and hook cleanup remain open. The prior shared probe always issued kill and
  a second wait even after its process-group or Job Object wait had already completed,
  while cancellation, timeout, stream failure, and missing-pipe cleanup discarded both
  kill and wait errors. Normal completion now uses its existing whole-tree wait as the
  single receipt. Every failure path sends termination and requires a group/job wait
  within the existing ten-second probe bound; an unconfirmed wait becomes the distinct
  non-startable `model_discovery_cleanup_failed` catalog state rather than being hidden
  by the original failure. This removes one redundant kill/wait pair from each successful
  probe without claiming measured latency improvement. The existing real cancellation
  test confirms the spawned descendant is gone, a synthetic failed kill/wait confirms
  the typed cleanup result, and catalog projection confirms it cannot start. All 199
  provider tests, warning-denied provider Clippy, formatting, diff, architecture,
  policy, and source-structure gates pass. No retry, fallback, polling, heartbeat,
  background cleanup, credential exposure, or real provider run was added.
- Completed the Antigravity hook portion of F-03 at `1328811`; explicit portal
  teardown remains open. The prior last-registration path removed registry and
  helper-executable ownership before it opened, locked, read, verified, and rewrote
  the hook document, then discarded every cleanup failure. The last explicit release
  now performs those exact operations before dropping either owner, and runtime stop
  or a launch failure after hook installation returns a cleanup failure instead of
  claiming release. `Drop` is only a final fail-closed attempt. The focused corruption
  test proves a failed release retains the helper owner and a later exact release can
  remove the restored managed definition; all 200 provider tests, warning-denied
  provider Clippy, formatting, diff, architecture, policy, and source-structure gates
  pass. Antigravity remains non-startable without a native receipt. No timer, polling,
  heartbeat, automatic retry, fallback, background cleanup, real provider run, or
  measured performance claim was added.
- Completed the resident-runtime RoomPortal stop portion of F-03 at `a5e4ff7`;
  provider-construction failure teardown remains open. Previously `Drop` cancelled
  and aborted only the listener task without awaiting it, while accepted connection
  tasks were detached, so a driver could report process stop before private bearer
  authority had an exact task-termination receipt. The common Portal owner now tracks
  accepted connections in a `JoinSet`, reaps completed entries in the event-driven
  accept flow, and explicitly cancels and joins the accept owner plus every connection.
  Codex, OpenCode, Claude Agent SDK, shared Cursor/Grok ACP, Antigravity, and the
  remote API family include this result in their existing stop outcome. The remote
  rmcp client's existing two-second close timeout becomes persistent
  cleanup-unconfirmed state rather than false success on a later stop. The accepted
  cost is at most one join entry per admitted connection under the existing hard
  eight-connection bound. A real TCP regression proves shutdown drains an accepted
  idle connection and closes the listener before it returns; all 201 provider tests,
  warning-denied provider Clippy, formatting, diff, architecture, policy, and source-
  structure gates pass. No production polling, retry, fallback, heartbeat, cleanup
  timer, background sweeper, credential exposure, real provider run, or measured
  performance claim was added. Files over 500 lines retain one existing provider or
  Portal state/invariant owner; splitting this stop call would add state-transfer and
  error-forwarding interfaces without separating a change reason.
- Completed the currently reachable provider-construction teardown portion of F-03
  at `15585cb`. Codex, OpenCode, Claude Agent SDK, and the shared Cursor/Grok ACP
  runtime now close a created RoomPortal when process spawn, pipe acquisition,
  protocol connection, or startup confirmation fails, and they stop an already
  started owned process before returning. The remote API family closes its rmcp
  client and Portal when connection, tool discovery, or exact catalog validation
  fails. One small cleanup-result owner combines an already provider-owned resource
  result with Portal shutdown; provider-specific process/client owners still perform
  the actual cleanup. Any unconfirmed resource or Portal cleanup remains the typed
  uncertain `provider_launch_cleanup_unconfirmed` result rather than being narrowed
  to a safe launch failure. The common non-Unix child-stop primitive owns the sole
  five-second bound; Codex and OpenCode platform process setup moved into private
  modules so their main provider files remain below the strong 800-line warning
  without exposing new state or forwarding interfaces. Antigravity still fails at
  its native-receipt gate before preparation and therefore creates no Portal or
  process; its dormant later construction path is not claimed as future completion.
  All 203 provider tests, warning-denied provider Clippy, formatting, diff,
  architecture, policy, and source-structure gates pass. A Windows cross-check
  reached `aws-lc-sys` but could not compile project code because the host has no
  `x86_64-w64-mingw32-gcc`; Windows compile/runtime behavior remains unknown. No
  retry, fallback, polling, heartbeat, background cleanup, credential exposure, or
  real provider run was added.
- Completed F-03's last reachable OpenCode boundary at `90bd91f`. Transport loss now
  poisons the driver and enters the existing exact process-stop path instead of
  discarding an abort result and retaining ambiguous session state. The distinct
  runtime-preserving interrupt requires OpenCode 1.17.18's exact `true` abort response
  and the existing idle receipt. Full stop removes the redundant best-effort native
  abort and MCP disconnect calls: owned process-tree termination and the separately
  awaited RoomPortal shutdown are the two complete authorities for that lifecycle.
  This removes 17 net lines without a new result type, state, abstraction, test,
  fallback, retry, polling, timer, heartbeat, or background work. All 203 existing
  provider tests, provider all-target check, warning-denied Clippy, formatting,
  architecture/policy/source-structure, and diff gates pass. No real provider ran;
  F-03 is complete pending the Phase 1 whole-phase reviews.
- Completed F-17 at `e01f071`. A malformed complete OpenCode SSE `data:` line now
  fails immediately as protocol corruption instead of being silently discarded and
  later surfacing as timeout or unrelated state. The existing pending-line buffer is
  exposed through one private function solely at its real complete-line boundary;
  one regression proves split valid input is retained and decoded before malformed
  input fails. All five focused SSE tests, provider all-target check, warning-denied
  Clippy, formatting, architecture/policy/source-structure, and diff gates pass. No
  alternate parser, retry, fallback, history read, polling, timer, background work,
  or performance claim was added. Phase 1 whole-phase review remains pending.
- Completed F-13 at `5ad4fdf` and `de637de`. Guardian re-execution, guardian binding,
  and Windows companion binding failures now remain bounded redacted construction
  results until the existing provider-start owner reports their exact cause. The
  intentional unstaged macOS absence remains distinct from a failed packaged binding;
  neither case selects weaker custody or another launch path. One focused regression
  proves a missing guardian executable reaches start as
  `provider_custody_binding_failed`. The separate structure commit moves factory and
  custody preparation out of the provider-registration policy owner without changing
  behavior, reducing `registration.rs` from 849 to 721 lines while adding one 134-line
  owner. The focused regression, provider all-target check, warning-denied Clippy,
  formatting, architecture/policy/source-structure, and diff gates pass. No test-only
  framework, retry, fallback, polling, timer, heartbeat, background work, weaker
  custody, or performance claim was added. Phase 1 whole-phase review remains pending.
- Completed C-06 at `2b9e895`. RoomPortal now exposes only its existing running
  state, endpoint, and process-scoped bearer environment name; the existing Codex
  config module alone translates those values into Codex `-c` syntax, approval mode,
  feature isolation, and tool timeouts. The existing command regression confirms the
  exact arguments and bearer secrecy. Provider all-target check, warning-denied
  Clippy, formatting, architecture/policy/source-structure, and diff gates pass, and
  repository search leaves no Codex config syntax in `room_portal.rs`. No new test,
  state, transport abstraction, retry, fallback, polling, timer, heartbeat, background
  work, or performance claim was added. Phase 1 whole-phase review remains pending.
- Completed C-03 at `3ab792b` and `bb25e9e`. One small domain owner now defines and
  validates the provider input, observation view, observation Agent-ID set, provider
  turn ID, and ordinary room-message limits. Producer, persistence recovery, adapter,
  provider-native, RoomPortal, and frontend boundaries still revalidate or normalize
  at their own trust edges; only the repeated policy moved. Semantically different
  128-byte identifiers remain separate. The previously unreachable persistence-only
  256-byte provider-turn allowance now matches the 128-byte adapter contract, and
  RoomPortal independently rejects an invalid observation instead of relying on the
  earlier adapter check. The copied message-edit UI reads its unchanged 12,000-character
  limit from a Rust-generated wire constant. No new test was added. Domain 58,
  persistence 243, and provider 205 tests, the three existing message-mutation UI
  tests, production frontend build, workspace all-target check, warning-denied Clippy,
  formatting, architecture/policy/source-structure, and diff gates pass. No new state,
  generic envelope type, retry, fallback, polling, timer, heartbeat, background work,
  or performance claim was added. Phase 1 whole-phase review remains pending.
- Completed D-06 measurement. A temporary, subsequently removed harness ran 10,000
  clean-store cycles of both indexed reconciliation candidate scans in 1.778 seconds
  in the unoptimized test binary (0.178 ms per one-second cycle; inclusive CPU below
  0.024% of one core). Ten real watcher samples recovered a pre-existing owner-loss
  candidate in 1.008-1.038 seconds, median 1.011 seconds. Staging cleanup measured
  15.792 ms p95/21.387 ms maximum for 159 sparse 64 MiB images and 62.570 ms p95 with
  a 96.090 ms cross-attempt maximum at the 1,024-entry cap. The exact 64 MiB copy,
  sync, and identity check measured 160.355 ms p95/maximum. The existing one-second
  recovery interval and 1,024 fail-closed staging cap therefore remain unchanged:
  weakening recovery would buy no material idle saving, while the bounded metadata
  cleanup is cheaper than the required byte-copy owner. No production code, test,
  state, timer, fallback, polling path, or instrumentation was added.
- Completed D-05 at `c76bb46` and `d88edf9`. The runtime-handle codec no longer
  generates, stores, parses, and discards a second random UUID. Runtime-v6 uses the
  already authoritative launch token as its sole generation identity and retains the
  Unix boot hash, platform distinction, separately adoptable supervisor owner, durable
  lease token, marker cross-checks, and `(handle, owner, token)` stale-CAS snapshots.
  The strict decoder rejects runtime-v5, cross-platform encodings, and trailing data;
  current schema 57 rejects schema 56 without migration, compatibility, or fallback
  code. The directly observed reduction is one UUID generation and 37 ASCII bytes per
  encoded live handle, not a claimed workload-level speedup. Twelve focused
  handle/absence/lease/recovery tests, all 203 provider tests, all 243 persistence
  tests, workspace check, warning-denied Clippy, formatting, diff, architecture,
  policy, and source-structure gates pass. Windows codec behavior is covered by the
  platform-neutral unit branch; no Windows binary or real provider ran. The change
  adds no state, abstraction, polling, retry, heartbeat, timer, silent failure, or
  background task and does not expand any file's responsibility.
- Completed the Phase 1 provider-custody portion of C-13 at `3779085`. The
  provider guardian's ready-line prefix is now produced, parsed, and exercised by
  tests from the one existing guardian protocol constant. `unix_custody` retains
  its independent bounded line parser, PID validation, failure mapping, and process
  custody checks; this does not create a general wire-constant module. Eight focused
  guardian tests plus the exact runtime initialization/reuse/stop regression pass,
  as do provider Clippy, formatting, diff, architecture, policy, and source-structure
  gates. The change removes only two duplicate literals and adds no state, allocation,
  task, timer, polling, retry, fallback, or performance claim. C-13's distinct human
  browser-credential and session-bearer items remain routed to Phase 5.
- Completed C-14 at `133749a`. One domain-owned pure Codex function now defines the
  two-member bundle identity, and one pure platform function owns the required
  `codex-code-mode-host` filename. Persistence commit-time revalidation, provider
  discovery and launch-time binding, open-file byte identity, staging, and server
  integration fixtures call that meaning without moving their distinct TOCTOU or
  lifecycle boundaries. Repository-wide current-source search leaves the bundle kind
  and companion filenames at that single owner. Three domain identity tests, one
  persistence revalidation test, four provider bundle tests, and all twelve server
  Agent Session boundary tests pass, with workspace check, warning-denied Clippy,
  formatting, diff, architecture, policy, and source-structure gates. The change is
  allocation- and I/O-neutral, introduces no generic executable framework, state,
  timer, polling, retry, fallback, or performance claim, and leaves every modified
  source file below the 500-line structure warning.
- Completed the lifecycle-intent portion of C-11 at `dcfc03d`. Domain enums now own
  the exact empty, start/stop, prepared, effect-inflight, unconfirmed, and
  effect-applied vocabulary while preserving the existing serialized strings.
  Lifecycle preparation, effect authorization, reservation matching, reconciliation,
  provider observation, cleanup, and failure mapping remain at their previous owners;
  no generic state machine or second transition authority was introduced. The one
  serialization-contract test, all 243 persistence tests, focused provider recovery
  and server reconciliation tests, warning-denied Clippy, formatting, architecture,
  policy, source-structure, and diff gates pass. There is no storage migration,
  compatibility path, new task/state/timer/poll/retry/fallback, or performance claim.
  Commit `307e359` completes C-11 by giving public Agent Session status, runtime
  status, and turn phase the same domain ownership and generated TypeScript unions.
  Existing lifecycle, turn, provider-observation, reconciliation, transaction, and
  error owners still perform every transition; no state-machine or repository layer
  was added. Repository-wide producer search fixed the copied frontend/test-only
  invalid session status `stopped` and removed the unproducible runtime-status
  `available` branch instead of preserving compatibility vocabulary. Current valid
  JSON strings are unchanged, while invalid stored or boundary values fail strict
  decoding. The one existing serialization-contract test, all workspace Rust tests,
  100 frontend files/664 tests, 25 desktop tests, warning-denied Clippy, formatting,
  architecture, policy, source-structure, generated-binding, CSS, and diff gates
  pass. This is a vocabulary-owner correction, not a performance claim, and adds no
  state, task, timer, polling, retry, heartbeat, fallback, or silent failure.
- Completed the Agent Session row portion of C-12 at `a82c981`. One private,
  entity-specific persistence module now owns exact optional row decode and row update;
  lifecycle, creation/reuse, and reconciliation callers retain their distinct missing,
  conflict, stale-CAS, transaction, and transition meanings. The insert path remains
  with creation because it atomically establishes participant and session authority.
  No generic repository, trait, codec, state, cache, retry, fallback, or new test was
  added. Repository-wide production search leaves the point-load and update SQL at one
  owner. All 243 existing persistence tests, workspace all-target check, persistence
  warning-denied Clippy, formatting, architecture, policy, source-structure, and diff
  gates pass. This is responsibility and duplication cleanup, not a performance claim.
  Commit `7c8eabc` closes the separate profile half by keeping row encoding/update
  with `profile_store` and reusing it from human admission; revision checks, avatar
  transfer/replacement, room projection, and admission transaction order remain with
  their existing owners. Three focused existing profile/admission tests pass with
  workspace all-target check, persistence warning-denied Clippy, formatting,
  architecture, policy, source-structure, and diff gates. It adds no new test,
  abstraction, state, fallback, polling, or performance claim. C-12 is complete.
- Completed the packaged provider-catalog correction and authorized three-provider
  real-turn matrix through `435615e`, `122061e`, `02bb066`, and `58d82f7`. The copied Agent Add
  flow exposed Grok as `model_discovery_failed` even though the same installed
  `grok models` command succeeded. A temporary removed diagnostic reproduced that
  the sixteen concurrent discoveries exhausted the four-slot filesystem owner and
  every later `try_acquire` failed immediately. The existing owner now queues for a
  permit inside its unchanged ten-second total deadline; the four-worker ceiling,
  cancellation, detached blocking-work custody, and visible timeout/failure mapping
  remain. Current Grok output marks its default as `* grok-4.6 (default)`, so the
  provider parser now accepts that exact official shape without broad compatibility
  parsing. Once all providers could discover, the real catalog measured 211,175
  bytes and correctly failed the unchanged 192 KiB publication bound. Removing only
  duplicate server-rendered descriptions produced a 173,833-byte lower bound. The
  first generic frontend derivation also changed unrelated DeepSeek accessibility
  names, which the complete frontend gate caught. The correction marks only gateway
  options whose descriptions were removed; the final provider array measures 192,033
  bytes and its complete ready catalog 192,221 bytes. The frontend derives the
  display/search/accessibility summary from retained structured context and
  pricing metadata only for that current projection. No model was
  removed, truncated, substituted, or loaded on demand, and no new state, worker,
  timer, polling, retry, fallback, provider special case, or catalog-size exception
  was added. All 205 provider tests and the corrected focused provider test, thirty
  existing Agent Add/model-selector tests,
  provider warning-denied Clippy, production frontend build, copied CSS gate, and
  artifact gate pass. The isolated packaged UI then created, started, completed one
  ordinary exact-response turn, and stopped `grok-4.6`, `gpt-5.6-luna`, and
  `opencode/muse-spark-1.2-contributor-free` in sequence. The app, its owned provider
  children, package, database/cache/WebKit data, and temporary workspace were removed
  after verification; unrelated Codex/ChatGPT processes were not touched. The final
  whole-workspace run passed every functional, frontend, desktop, Rust, TCP/WebSocket,
  generated-binding, warning-denied Clippy, policy, structure, CSS, formatting, and
  diff check in 366.19 seconds with 2,117,484,544-byte maximum RSS. Its final
  maintenance check found the now-complete Cargo cache at 21,158,686,720 bytes, above
  the existing 18 GiB retention ceiling; explicit repository-owned maintenance
  removed that cache and the artifact check then passed. Phase 1's sixteen exact
  registrations, generated frontend contract, provider-owned selection/configuration,
  single completion/session authorities, typed cleanup, explicit unavailable states,
  and three authorized real turns satisfy the local exit contract. No provider-history
  transcript/scraping/print path, silent runtime fallback, or ownerless reachable
  cleanup remains. Whole-phase external reviews remain pending; Phase 2 must not start
  until both reviewers approve this candidate.
- Next production work: Phase 1 whole-phase external cross-review.
  - First establish the full sixteen-provider acceptance matrix and the smallest
    common registration, selection, start, ordinary-turn, visible-failure, and stop
    contracts. On Codex, Antigravity, OpenCode, and DeepSeek, remove only false or
    unsafe behavior that blocks that shared foundation; do not polish one provider
    while the rest of the retained structure is absent.
  - Then connect every retained provider to that real basic contract: Claude through
    the official Agent SDK; the remote API family
    (Cerebras, OpenRouter, Vercel AI Gateway, LLM Gateway, TokenRouter, and Custom API);
    Ollama and LM Studio; and the remaining original native providers Cursor, Freebuff,
    and Grok through its official ACP stdio contract. Grok registration and
    static/contract verification are mandatory even without a local executable;
    executable/login absence affects runtime availability, while absent provider-run
    authorization affects only whether real-run evidence may be collected. Every
    retained provider, including Cursor, requires its static/contract implementation.
  - For that API family, first establish the smallest common HTTPS/SSE execution
    mechanism from DeepSeek and matching verified-original behavior: transport,
    streaming decode, cancellation, normalized failure/usage, bounds, and redaction.
    Provider endpoints, credentials, headers, catalogs/defaults/model controls,
    completion/session identity, and Custom API SSRF policy stay provider-owned.
  - After breadth exists, harden cancellation/interruption, restart/reconnect,
    long-running turns, authorized tool use, ambiguous completion/effects, explicit
    failure, and exact cleanup across the entire available-provider matrix. Only then
    perform evidence-backed provider-specific performance or UX refinement.
  - Reject Gemini CLI, Qwen CLI, and Goose ACP as review-driven scope expansion. They
    are absent from the verified reachable sixteen-provider baseline; Antigravity is
    not Gemini CLI, and reviewer suggestions do not add product scope.
  - Keep provider execution separate from later external admission. `assemble room
    attend --provider` launches an available driver under its own AgentBridge owner;
    `assemble room connector-mcp` launches no model/provider and lets an already-running
    AI app/CLI session use current-session room tools. Room Connector MCP and the
    resident provider's private RoomPortal MCP may share libraries/schemas, never
    principals, credentials, permissions, state, or lifecycle.
  - RoomPortal owns room-tool meaning, authorization, mutation, and results once.
    MCP and native function/tool-call integrations are thin transport bindings; every
    call rechecks the bound session/capability. Unsupported tool transport stays
    explicitly unavailable—never output parsing, prompt convention, or client mutation.
    Phase 1 removes duplicated provider-local tool-name/schema/allow-list declarations,
    including the current DeepSeek list, only where they express that same contract.
  - Real provider and packaged-frontend verification for current work uses Grok,
    Codex Luna, and OpenCode Muse Spark contributor free. Antigravity is excluded from
    this active real-run matrix. No missing client/model may be replaced by another
    provider, model, mock, or fallback; unavailable evidence remains incomplete.
  - Share only proven-identical transport, decoding, bounds, secret handling, redaction,
    cancellation, and cleanup mechanisms. Endpoints, credentials, catalogs, model
    controls, session identity, completion receipts, permissions, and lifecycle semantics
    remain with their actual owner. Do not build a generic provider framework in advance.
  - Restore each copied frontend entry point with its owning backend slice. Keep the
    current retained Rust frontend's `Harness`/`API`/`Local` grouping during provider
    cutover; older `Subscription` naming is not the target for this surface. Further
    redesign waits until post-parity, and a provider or model family may appear in
    more than one route group.
  - Treat the current Rust frontend as the presentation baseline. Restoring an
    inactive original flow must preserve the reviewed single-search, result-avatar
    and provider-logo projection, unified header/right-panel geometry,
    profile/modal stacking, and current Agent Add composition; import the missing
    behavior instead of replacing the current UI with an older tree.
  - Critical-web Pro owns whole-plan, product-parity, coverage, phase, SSoT/DDD, and
    overimplementation review. Daybreak Blue `xhigh` owns manual source/diff security,
    async/process/TCP/WebSocket, polling/timer/fallback, swallowed-failure, and cleanup
    review. Each completed phase receives both reviews; a diff approval is not approval
    of plan completeness.
- Build-artifact lifecycle uses the `35a418c` nonincremental profile. macOS uses
  packed debug information, eliminating Cargo's
  unpacked per-unit object copies while retaining source DWARF in dSYM bundles; the
  desktop shell shares the repository Cargo target. Routine complete verification
  performs non-destructive checks before and after the build. It fails closed if an
  obsolete desktop target exists or the active cache exceeds the measured 18 GiB
  ceiling; only explicit `make artifact-prune` maintenance invokes Cargo clean, so
  verification cannot race-delete another Cargo/Tauri operation. Unix accounting
  deduplicates hard links and uses allocated blocks; platforms without that metadata
  use logical bytes and never collapse zero file identities. Cargo incremental output
  is disabled after its measured owner alone reached 8.10 GiB and repeatedly pushed
  the active target over the maintenance ceiling. The current retained cache occupies
  14,836,060 allocated KiB after complete verification and 14.63 GiB after subsequent
  focused rebuilding, contains neither incremental data nor `.rcgu.o` files, passes
  complete verification in 432.17 seconds, and serves an immediate all-target
  workspace check in 0.26 seconds. The 18 GiB ceiling retains about 3.4 GiB of
  measured source/profile variance rather than the obsolete 20.6 GiB cache basis.
  Critical ChatGPT Pro and Daybreaker Blue High independently found the portable-test
  and redundant-scan defects; `537c1b9` closes both. Each reviewer approved that
  correction, exact `42f0af5..537c1b9`, complete correction
  `9d02acf..537c1b9`, full batch `9a4b5f6..537c1b9`, cumulative
  `8903445..537c1b9`, and HEAD `537c1b9` at `C0/H0/M0/L0`.
  The later nonincremental-profile correction `dff4b65`, exact
  `a9a31ee..dff4b65`, corrected full batch `b5b0f6a..dff4b65`, and HEAD `dff4b65`
  are independently approved by critical-web Pro and Daybreaker Blue High at
  `C0/H0/M0/L0`.
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

## Historical evidence

Earlier implementation and review journals remain in Git:
`git show be63b1e4e6031853a3a666bb7deddd82781ce43d:WORKBOARD.md`.
