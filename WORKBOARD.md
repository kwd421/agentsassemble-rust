# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity inventory at
`9711232` remains reviewed historical evidence. Its finding-number order is not
the production roadmap. The corrected provider-first plan is approved and active.
The finite Phase 0B prerequisites F-14 and F-16 and their whole-phase cross-review
are complete. Phase 1 provider-first implementation is active.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: Phase 1 provider-first implementation. The finite Phase 0B prerequisites
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
- Completed: F-14. Room commands no longer substitute a wall-clock/counter identifier
  when WebCrypto UUID generation is unavailable. One browser request-ID owner now
  serves bootstrap, room creation, room admission, and room commands; durable client
  identity remains separate because it has a different storage lifecycle. A room
  command fails before pending-state insertion, its deadline, or socket transmission
  without secure request identity, while the existing exact serialized-ID replay
  contract is unchanged. The focused unavailable-identity regression, all 653 frontend
  tests, the production TypeScript/Vite build, and the copied CSS gate pass. This
  removes a concrete replay-identity downgrade without adding state, retry, fallback,
  polling, or a broader identifier abstraction.
- Completed: F-16 preventive cleanup. The socket branch now returns rather than
  continuing if its provider-catalog watch closes. In the current production graph this
  condition is unreachable: every `ProviderCatalogService` clone retains the sender and
  the socket owns its containing `AppState` for the whole session. The synthetic naked-
  watch test and helper were therefore removed instead of manufacturing evidence for a
  live CPU defect. The one-line terminal handling prevents a future custody change from
  exposing an immediate closed-watch loop while adding no polling, retry, fallback,
  abstraction, or state. Normal catalog pushes and socket behavior are unchanged.
  Fresh complete `make verify` at public HEAD `15b4c5c` passed the frontend (99
  files/653 tests), desktop, Rust, real TCP/WebSocket, generated-binding, Clippy,
  policy, structure, diff, CSS, and artifact gates in 180.78 seconds with
  1,948,237,824-byte maximum RSS. Critical-web Pro and Daybreak Blue `xhigh` each
  manually approve individual `2f66567`, corrected F-16 through `15b4c5c`, exact
  `9c31832..15b4c5c`, cumulative `4dbfd79..15b4c5c`, HEAD `15b4c5c`, and Phase 0B
  behavior at `C0/H0/M0/L0`. The only review findings were the removed synthetic
  F-16 reachability claim/test and stale closeout wording; no source, documentation,
  structure, duplication, overimplementation, ownership, lifecycle, polling, timer,
  fallback, swallowed-failure, compatibility, or performance-evidence finding remains.
- Completed: D-01 at `a7949bd`; the uncalled HTTP challenge/ticket bootstrap and
  startup secret are absent, while private-control and admitted-human socket ticket
  issuance remain.
- Completed: D-02 at `3ffb9eb`, `77cae0e`, `0d24741`, and `57fd6ec`; the
  evidence-free receipt, digests, per-frame HMAC/base64/counter envelope, proof-key
  ticket state, and obsolete test vocabulary are absent. One-use ticket authority,
  strict bounded JSON, finite snapshot/catch-up, replay, and failure contracts remain.
- Completed: D-03 through `5693e13`. Profile, preferences, message pins,
  message search, message attachments, and bound room-appearance reads authorize
  reusable remote sessions at the target; the obsolete socket-to-profile authority
  interpretation and public HTTP-purpose exchange state are absent. Desktop purpose
  tickets and one-use WebSocket upgrade tickets remain because they cross distinct
  authority boundaries. Critical ChatGPT Pro and Daybreaker Blue High each manually
  approved cumulative `ac905de..5693e13` and HEAD at `C0/H0/M0/L0`.
- Completed: F-04 through `8903445`. Four non-executable capability fields,
  copied room-delete/participant-kick/provider-response/agent-readd controls, and the
  producerless provider-request snapshot, kicked-event projection, and room-delete
  callback are absent. `bridge.publish` remains because the current vote path consumes
  it; the distinct server `participant_kicked` start-denial code and OpenCode's
  interactive-request fail-closed test remain current contracts. Critical ChatGPT Pro
  and Daybreaker Blue High each approved exact `f4bc3d9..8903445`, cumulative
  `dd1e99d..8903445`, and HEAD `8903445` at `C0/H0/M0/L0` after the stale re-add
  guidance and audit-state corrections.
- Correction history: F-05 closure and F-06 profile projection correction — gate copied requests,
  polling, and heartbeats whose complete Rust owner does not yet exist. Do not add
  dummy routes, fallback data, timers, or a generic feature framework. The first
  independently committed batch through `87d3d0d` removes the active Friends,
  side-chat, custom-channel, and deferred voice entry paths; a fresh `make verify`
  passed before review. Initial cross-review found three Low dead-state remnants;
  `a2b2f41` removes them and a fresh complete `make verify` passes. Follow-up
  re-review found two Low producerless/dead projections plus one Low documentation
  overclaim; `87d3d0d` and the current documentation correction address them, with
  a fresh complete `make verify` passing. Critical ChatGPT Pro and Daybreaker Blue
  High each approved exact `778d761..f74af57`, cumulative `8903445..f74af57`, and
  HEAD `f74af57` at `C0/H0/M0/L0`.
  The next three independent commits, `762ba40`, `11e167b`, and `7159c2d`, remove
  the copied Room Connector invite, operator-pairing issuer, and guest companion
  admission controls without changing human invitation, incoming pairing redemption,
  or room membership. A fresh complete `make verify` passes. Both manual reviewers
  found the Low omission of a JavaScript chunk from the emitted total; Daybreaker then
  found that first correction `95951d9` described the aggregate rounding incorrectly.
  Corrections through `7f2e878` distinguish the raw-byte aggregate from displayed
  per-chunk gzip figures. Critical ChatGPT Pro and Daybreaker Blue High each approved
  the corrected original batch, correction `9759d73..7f2e878`, cumulative
  `8903445..7f2e878`, and
  HEAD `7f2e878` at `C0/H0/M0/L0`.
  Feature commit `96a7573` removes the public Google account-settings mount,
  absent `/api/account*` client contract, and browser script loader while preserving
  central startup identity, guest recovery, and ordinary profile editing. The production
  build and all 102 frontend files/640 tests pass. Feature commit `fd74b90` then
  removes the producerless AI-friend packet branch and its obsolete moderator client
  while preserving managed human invitation; all 102 frontend files/639 tests pass.
  Feature commit `d45afb5` removes the unmounted runtime-version component whose
  source retained an absent-route 15-second poll and silently ignored failures; the
  production build and exact CSS gate pass. A fresh complete `make verify` passes at
  `d45978a`. Critical ChatGPT Pro and Daybreaker Blue High each manually approved every
  individual commit, exact `a9dceae..d45978a`, cumulative F-05
  `8903445..d45978a`, and HEAD `d45978a` at `C0/H0/M0/L0`. Evidence-backed
  dormant-source cleanup remains in F-05.
  Feature commits `84dbc3a`, `daadd8d`, and `9ee4952` replace the
  behavior-module CSS side-effect chain with one explicit stylesheet-order owner,
  then remove the unreachable Friends presentation and absent `/api/room-friends`
  client/hook boundary. The production CSS artifact remains byte-identical and the
  current frontend passes 101 files/628 tests. Friends product completion and its
  remaining dormant CSS are not claimed by this batch. Critical ChatGPT Pro and
  Daybreaker Blue High each approved every individual commit, exact
  `168bb32..91a071f`, cumulative F-05 `8903445..91a071f`, and HEAD `91a071f`
  at `C0/H0/M0/L0` with no actionable finding.
  The next independent commits `b723715`, `1521067`, and `8cd8628` remove the
  now-unreachable side-chat presentation, browser state owner, and absent
  `/api/side-chat` client contract. The production CSS/JavaScript output remains
  unchanged and the reduced frontend passes 98 files/617 tests. Phase 6 side-chat
  product completion and dormant copied CSS remain open. A fresh complete
  `make verify` passes. Critical ChatGPT Pro and Daybreaker Blue High each approved
  every individual commit, exact `e0c6ad0..d338936`, cumulative F-05
  `8903445..d338936`, and HEAD `d338936` at `C0/H0/M0/L0` with no actionable
  finding.
  The next independent commits `84ceb13`, `adf17ee`, and `2f439f1` remove only
  the now-unreachable side-chat, Friends directory/activity, and Friends profile/DM
  selectors. The active agent-add button and human-invite friend rows remain styled.
  The exact CSS gate follows the measured artifact; the final frontend emits
  154.83/27.47 kB CSS and passes 98 files/617 tests. Future Phase 5 Friends and
  Phase 6 side-chat behavior remain open, as does separate dormant HomeSidebar CSS.
  A fresh complete `make verify` passes in 238.79 seconds with a 573,587,456-byte
  maximum resident set. Critical ChatGPT Pro and Daybreaker Blue High both found
  the same documentation-only L1 unit error; `bbfb710` corrects “147 declarations”
  to “147 source lines.” Each then approved exact `5d0ee87..bbfb710`, cumulative
  F-05 `8903445..bbfb710`, and HEAD `bbfb710` at `C0/H0/M0/L0`.
  Candidate `f16bc2c` removes the last HomeSidebar-only selectors while preserving
  the active room sidebar and agent-add button declarations. Candidate `fbb952f`
  removes the unimported Python-mirrored provider-permission helper; deferred
  Mafia/RimWorld/voice and named Phase 6/8 surfaces remain untouched. The resulting
  production CSS is 152.64/27.14 kB and the frontend passes 98 files/617 tests.
  Commit `fbebad6` begins F-06 by making Agent Session identity authoritative in
  the shared canonical timeline/history/search profile map while retaining room role
  from the participant. Correction `f1edead` makes the configure regression use the
  real runtime-only contract rather than an impossible identity mutation. Commits
  `fe568bb`, `e0a681d`, and `97816b5` apply the same ownership to desktop/mobile
  rosters, mentions, and typing/progress labels; participant data still owns room
  role, mute, membership, and permissions. Commit `11d3528` hides the incomplete
  Agent identity editor instead of routing profile fields through runtime configure
  or the generic attachment owner; human profile and Agent runtime settings remain.
  The full frontend passes 98 files/618 tests, and a fresh complete `make verify`
  passes in 244.54 seconds with a 578,846,720-byte maximum resident set. Daybreaker
  Blue High approved the three feature commits and typing correction individually at
  `C0/H0/M0/L0`; its only cumulative/HEAD finding is this stale current-state
  documentation. Isolated packaged verification now confirms canonical identity
  across restart, roster, mention, typing, timeline, and search for Codex Terra,
  Antigravity Flash, and OpenCode Muse Spark sessions. Actual Antigravity and
  OpenCode turns complete with that identity; Codex start instead exposes the exact
  `runtime_start_recovered_gone` lifecycle failure and remains open without a retry
  or fallback. Correction `e869f42` reuses the shared provider-logo owner for
  avatarless Agent Session search results while preserving custom Agent avatars and
  human initials. Daybreaker Blue High found that its descendant image selector also
  restyled `ProviderLogo`'s nested image; correction `0363622` limits the search-avatar
  rule to a direct custom-avatar child. A fresh complete `make verify` passes in 228.86
  seconds with a 582,074,368-byte maximum resident set. A post-correction packaged
  visual recheck remains explicit `unknown`: central guest creation stayed pending on
  two bounded attempts, local-mode Antigravity failed on an unapproved terminal command,
  and the required OpenCode Muse Spark model was absent from that run's catalog. No
  alternate model, retry loop, or fallback was used. Daybreaker and critical-web Pro
  each found the same documentation-only Low: `e912e75` carried the preceding CSS
  artifact's gzip byte count. Correction `7566d3f` records the SHA-matching current
  artifact's 26,579 `gzip -9` bytes. Both reviewers approved `0363622`, `7566d3f`,
  exact `12430b1..7566d3f`, original search batch `35bc375..7566d3f`, cumulative
  F-06 `8903445..7566d3f`, and HEAD `7566d3f` at `C0/H0/M0/L0`; `e912e75` alone
  retains its historical L1. The complete Agent profile mutation/asset owner remains
  open.
- Completed: F-07 provider operation exposure. Independent commits `582a02e`,
  `edfb7c5`, and `c890a9a` make the Rust registration descriptor the credential-
  operation exposure owner, remove the absent catalog-refresh request, and remove the
  absent provider-usage request/state. Codex, Antigravity, and OpenCode no longer expose
  login or credential controls; DeepSeek retains its implemented keyring operations;
  usage remains visibly unsupported. Correction `9794b0a` waits for the actual
  workspace-picker recovery state exposed by the fresh complete verification instead
  of racing its async `finally`. Focused tests, production builds, architecture gates,
  and a fresh complete `make verify` pass. An isolated local-mode package confirms the
  operation boundary without starting a provider. No dummy route, local authority,
  compatibility path, retry, polling, heartbeat, timer, fallback, or swallowed failure
  replaces the removed requests. Daybreaker found two valid Lows in the pushed batch:
  dead dynamic quota/visibility code still influenced roster ownership, and the
  verification record overstated provider-usage failure handling as silent. Correction
  `1313aba` removes that dead contract, uses explicit `owner_id` for grouping, and passes
  all 97 frontend files/617 tests plus production-build, CSS, diff, and architecture
  gates. This record corrects the failure description. Daybreaker's re-review then
  found one excluded obsolete quota-visibility suite; `534a953` removes it and a
  repository-wide reference search is empty. Critical-web Pro independently confirmed
  the dead quota finding and found that this board called registration the credential-
  operation owner rather than the narrower exposure owner; the wording is now corrected
  without moving DeepSeek execution authority from its route and credential store.
  Critical-web Pro's correction review then found one remaining Low: the secondary
  roster projection inferred human ownership from Agent Session runtime custody.
  Correction `9256976` removes that duplicate `ownedByViewer` state and viewer
  fallback. Replacement critical-web Pro then found that the primary `LiveAgent`
  projection still substituted `agent.owner_id` when an existing room participant had
  no owner. Correction `4c1bd57` now uses only that participant's room-owned
  `owner_id`; a `LiveAgent` with no participant remains an explicitly separate
  presentation case. Critical-web Pro and Daybreaker then independently found the
  same stale-owner fallback in the active mobile roster and mention suggestions and
  revised HEAD `f934382` at `C0/H0/M0/L2`. Correction `703b5c6` applies the
  member-presence rule to both consumers, and `8beb103` keys ownerless desktop/mobile
  presentation groups by Agent
  ID rather than a non-authoritative display label. Daybreaker's next pass found one
  adjacent Low: the secondary mobile projection used mutable room role to identify a
  human. Correction `020d89f` uses immutable participant kind instead, so an Agent
  assigned the Human role still follows its room-owned owner. All 20 focused roster/
  mobile/mention tests, the production build, CSS gate, diff gate, and architecture
  gate pass. Daybreaker previously approved individual `9256976` and
  `d0c8ce8`, exact `ae171dc..d0c8ce8`, full correction
  `879db4b..d0c8ce8`, cumulative `5ec012f..d0c8ce8`, and HEAD `d0c8ce8` at
  `C0/H0/M0/L0`; critical-web Pro correctly revised that state at `C0/H0/M0/L2`.
  Manual source re-review of public HEAD `b6d844b` is now complete. Critical-web Pro
  and Daybreaker each approve individual `703b5c6`, `8beb103`, `020d89f`, and
  `b6d844b`, exact `f934382..b6d844b`, full correction `879db4b..b6d844b`,
  cumulative `5ec012f..b6d844b`, and HEAD `b6d844b` at `C0/H0/M0/L0`.
  Independent commits `5b94ac6`, `1959a08`, and `d081761` close the remaining
  F-07 model-selection scope. The Rust catalog now prefers exact
  `opencode/muse-spark-1.2-contributor-free` only when advertised, otherwise leaves
  the model unselected while retaining a ready nonempty catalog; the frontend also
  leaves an absent scoped default unselected, and the producerless `stale_cache`
  client state is absent. Backend exact selection remains authoritative. A fresh
  complete `make verify` and an isolated package pass; the package confirms the exact
  Muse Spark default and explicit selection of a different advertised model, while the
  preferred-missing packaged flow remains `unknown` because every current catalog
  contained its preference. Critical-web Pro and Daybreaker approve each source
  commit at `C0/H0/M0/L0` and revise exact `67303e0..d081761` and HEAD `d081761`
  only at `C0/H0/M0/L1` for stale current-state documentation corrected in
  `e13210f`. Each then approves individual `e13210f`, corrected exact
  `67303e0..e13210f`, and HEAD `e13210f` at `C0/H0/M0/L0`. No broader
  provider-completion claim is made.
- Completed: F-08 HTTP admission capacity. Source commit `c0cb3e2` retains the
  128-connection total ceiling and adds a 127-connection budget only after the
  existing ingress owner classifies a connection as trusted public. A real TCP
  request-body barrier proves one local health request progresses while 127 public
  connections are active, the next public request receives 503, and public admission
  resumes after every held request reaches its terminal TCP response. Pre-header
  sockets remain unclassified and can occupy all total permits for the existing
  three-second header deadline; this is an explicit residual limit, not a hidden
  fallback. Correction `c184739` replaces the initial client-drop teardown with those
  terminal-response barriers. Complete verification and focused correction gates
  pass. Critical-web Pro and Daybreaker Blue High each approve individual
  `c184739`, documentation correction `c47dbbb`, exact `952fa96..c47dbbb`, and
  HEAD `c47dbbb` at `C0/H0/M0/L0` with no actionable finding.
- Completed: F-09 human invite guide and accepted client kinds through `0fd931d`.
  Current-original
  commit `d504647` and the Rust frontend each send only exact `human`; source commits
  `0e38579`, `cae8d64`, and `e76cda7` therefore remove the unknown-token coercion,
  align the guide with terminal session expiry, and remove the frontend's 60-second
  early-expiry skew. Daybreaker's two Low findings are corrected in `9821433`: the
  guide derives its duration from the session-TTL owner and the startup E2E fixture
  no longer advertises same-invite renewal. Daybreaker approves the complete
  `820e427..9821433` range and HEAD at `C0/H0/M0/L0`. Critical-web Pro independently
  revises the requested `e76cda7` snapshot at `C0/H0/M0/L2` for those two projections
  and stale current-state documentation. Commits `9821433` and `0fd931d` correct
  those findings. Critical-web Pro and Daybreaker Blue High each approve the
  individual corrections, exact `9821433..0fd931d`, full correction
  `e76cda7..0fd931d`, cumulative F-09 `820e427..0fd931d`, and HEAD `0fd931d` at
  `C0/H0/M0/L0` with no actionable finding.
- Completed: F-10 DeepSeek credential-source authority in `2f0177b`. Current-original
  `d5046473` really prioritizes the keyring and then `DEEPSEEK_API_KEY`, but the
  reachable control exposes no environment-source selection or revoke authority.
  The Rust owner therefore has one keyring source, deletion becomes visibly missing,
  and the strict frontend rejects the retired response value. No source-selection
  framework, compatibility path, fallback, or second authority was added. Critical-
  web Pro and Daybreaker Blue High each manually approve corrected full batch
  `b5b0f6a..dff4b65` and HEAD `dff4b65` at `C0/H0/M0/L0`; the shared initial Low was
  only the obsolete build-cache ceiling basis corrected by `dff4b65`.
- Completed: F-11 frontend wire-contract generation ownership.
  - Definition: the Rust protocol exporter is the one semantic owner for room event,
    room settings, provider catalog, snapshot, Participant, and Agent Session wire
    values. Endpoint-local runtime decoders continue to own trust-boundary rejection.
  - Original defect: the copied `generatedRoomEvent.ts`, handwritten provider/snapshot
    interfaces, legacy `RoomAgentSession`/`RoomMember` fields, and a second React
    provider-array state independently describe values already generated by Rust.
    Several copied fields drive UI branches even though the Rust server never sends or
    accepts them.
  - Current batch: the Participant half is implemented in pushed commits `4fc06a0`,
    `2f8ebbb`, and `c40fe55`. Role mutation ACKs now
    expose their durable event sequence, snapshot/command boundaries accept only exact
    unique Participant projections bound one-to-one with Agent Sessions, and the browser
    aliases generated `Participant` instead of retaining legacy provider/runtime fields or
    a producerless member `thinking` signal. Room role, join state, mute state, and
    membership remain Participant-owned; runtime identity/lifecycle remain Agent
    Session-owned.
  - Non-goals: do not create a universal event decoder, restore the removed provider-
    request surface, add custom-provider or alternate-harness support, change voice or
    Mafia, or complete the separate Agent profile mutation owner.
  - Commit boundary 1: remove the orphan generated-looking event/settings file and use
    the actual Rust-generated semantic types in live, history, and search consumers.
  - Commit boundary 2: derive provider catalog types and React state from the generated
    catalog, retain Rust-owned `interactive`, and remove only the unowned executable,
    custom endpoint/model, and work-harness branches.
  - Commit boundary 3: derive Participant and Agent Session wire types from Rust, keep
    one deliberate socket/event validation schema, and remove legacy presentation
    fields and parallel activity/diagnostic states that no current producer owns.
  - Acceptance: snapshot, live, history, search, catalog update, and command-result
    boundaries retain their distinct errors and strict rejection; no missing value is
    replaced with fake/default wire state; no fallback, polling, heartbeat, retry,
    compatibility path, or provider-request surface is introduced; focused frontend
    tests, generated bindings, the production build, architecture/diff gates, and a
    complete verification pass.
  - Initial manual review of public `a07ecdf` is complete. Critical-web Pro found one
    cumulative Medium: socket event/settings bodies still accepted partial generated
    projections. Daybreaker found three Lows: the same boundary gap, catalog-absent
    controls and manually unbound key lists, and incomplete Agent Session participant/
    create-ACK binding. Independent corrections `77eea8a`, `fd473e8`, and `9320494`
    addressed those initially reported sites. Fresh verification of `1c9f397` passed in 163.03
    seconds with 788,037,632-byte maximum RSS. Daybreaker's first correction re-review
    then found one Medium: default `agent.create(start=true)` returns the creation event
    plus a final `agent_session_state`, while the browser required only the creation
    event. It also found one Low stale-status claim in this workboard. `71acb41` fixes
    the server's stale final `event_seq` and validates the real create/start and replay
    transition without a fallback. Fresh complete verification passed in 247.45 seconds
    with 1,860,009,984-byte maximum RSS. The next manual reviews found two remaining
    strict-boundary gaps: Daybreaker Low 1 showed that nested create/start event copies
    were compared by identity rather than complete payload, and Critical-web Pro Medium
    1 showed that standalone `room_settings_updated` events did not validate their exact
    generated settings projection. Pro also assigned Low 1 to the earlier overstatement
    that the settings root was already closed. Corrections `98a9af9` and `a8583f0`
    respectively bind complete duplicate event projections and require exact settings in
    the shared live/history/snapshot event validator. Complete verification passes in
    164.33 seconds with 791,937,024-byte maximum RSS. Daybreaker's correction review
    then found one real Medium: Rust settings persistence omitted `result.event_seq`
    even though the strict browser ACK contract requires it, so a reachable settings
    update would disconnect after commit. `6c799f5` emits the committed event sequence
    from the persistence owner and proves exact duplicate replay through the actual TCP
    boundary. `06c7139` keeps that replay invariant in its own focused test after the
    first full gate exposed test-function overgrowth; it changes no product behavior.
    Fresh complete verification passes in 197.35 seconds with 1,295,138,816-byte maximum
    RSS. Daybreaker approves individual `6c799f5`, `06c7139`, `07007c8`, exact
    `f7c1277..07007c8`, cumulative `dff4b65..07007c8`, and HEAD `07007c8` at
    `C0/H0/M0/L0`. Pro's completed `f7c1277` review found one additional Low: a
    state event could disagree with its nested Agent Session on session/runtime/display
    identity, and a create/start final session could avoid the producer's attached state
    while duplicating the same false projection everywhere. `a01502f` binds only those
    duplicated producer fields and the known fresh-start transition; it does not copy the
    lifecycle state machine into the browser. `fc229c0` further requires the Rust
    provider's actual fresh-start success fact, `provider_session_active=true`, rather
    than accepting an internally consistent producer-impossible ACK. Fresh complete
    verification of that correction passes in 165.14 seconds with 784,351,232-byte
    maximum RSS. Critical-web Pro and Daybreaker Blue High each approve individual
    `fc229c0`, its exact correction ranges, cumulative F-11 through `fc229c0`, and HEAD
    at `C0/H0/M0/L0`.
  - Participant-batch verification: the three commits are independently buildable and
    contain 68, 265, and 713 changed lines respectively. The final change removes 468
    lines of duplicate/fabricated projection state. Focused Participant/roster tests pass
    69 cases; the production build and CSS gate pass; fresh complete `make verify` passes
    frontend 99 files/652 tests, desktop, Rust unit, actual TCP/WebSocket boundaries,
    generated bindings, Clippy, policy, structure, diff, and artifact gates in 257.28
    seconds with 1,839,890,432-byte maximum RSS. No performance gain is claimed. The
    initial Daybreaker review found one Low legacy Participant-kind owner: the UI still
    exposed unused `subscription_ai/api/local/remote/unknown` metadata despite admitting
    only `human|agent`. Independent repository-wide correction inspection also found a
    partial mention projection. Correction `b4739ce` removes the reported legacy
    vocabulary and makes mention enumeration room-owned. Its focused 27 tests, production
    build/CSS, all 652 frontend tests, and fresh complete `make verify` pass in 164.73
    seconds with 769,294,336-byte maximum RSS. Pro's completed original-batch review found
    the same obsolete vocabulary Low plus a separate Low stale push-state sentence;
    `58f0f8b` had already corrected that sentence before the result was read. Daybreaker's
    correction re-review then found that mention presentation still accepted a handwritten
    Agent identity with producerless avatar/owner fields and that the first correction note
    incorrectly attributed the independently found mention issue to its initial review.
    Source correction `2a53349` now accepts generated Agent Sessions keyed by
    `participant_id`; the room Participant remains the only ownership source, while Agent
    Session supplies display/provider presentation. The focused 5 tests, production
    build/CSS, and all 652 frontend tests pass. Fresh complete `make verify` passes every
    frontend, desktop, Rust, real TCP/WebSocket, generated-binding, Clippy, policy,
    structure, diff, CSS, and artifact gate in 164.79 seconds with 778,829,824-byte
    maximum RSS. Daybreaker Blue High and critical-web Pro each approve individual
    `2a53349` and `5ae8b34`, exact correction `58f0f8b..5ae8b34`, corrected Participant
    batch `4fc06a0..5ae8b34`, cumulative F-11 `dff4b65..5ae8b34`, and HEAD `5ae8b34`
    at `C0/H0/M0/L0` with no actionable finding.
- Rejected task: F-12 DeepSeek complete-turn/cost budget. Reqwest's three-minute
  `read_timeout` applies to each read and resets after successful progress; it is not
  multiplied into a 51-minute whole-turn deadline. DeepSeek already has a finite initial
  response plus sixteen tool rounds, caller cancellation, and a selected per-request
  output bound. No observed cost, hang, or security threat justifies narrowing valid
  long-running work with another wall-clock or cumulative token/cost owner. Preserve the
  existing bounds and explicit failure semantics; measurements may inform a later product
  decision but do not pre-authorize a limit.
- Plan-lock review findings: critical-web Pro found that Grok static implementation
  was incorrectly gated by a local client, then found the same escape for Cursor and
  a coupling between provider-run authorization, runtime availability, and evidence.
  Daybreaker independently confirmed the cross-provider contract inconsistency. Commits
  `6f14d23`, `5badabe`, and `3229fbe` close those findings, including the residual Phase 8
  wording. Pro withdrew the macOS Keychain finding because the shipped process has one
  composition-owned credential store; reopen it only if a second production store or a
  Keychain interaction-state user outside that owner is added. The F-12 rejection above
  remains unchanged. Critical-web Pro and Daybreaker Blue `xhigh` each approve individual
  `3229fbe`, exact correction `5badabe..3229fbe`, full correction
  `62d58b9..3229fbe`, cumulative plan `1c5b37e..3229fbe`, HEAD, and master-plan
  completeness at `C0/H0/M0/L0` with no actionable finding.
- Implemented pending Phase 1 whole-phase review: F-01 Antigravity transcript removal.
  Commit `bddd08e` makes an installed client visibly available but non-startable with
  stable `provider_native_receipt_unavailable`, removes transcript-derived session and
  completion promotion, the launch nonce, active 100 ms transcript polling, and the
  now-pointless model probe. Commit `617efee` moves the retired transcript-correlated
  turn path unchanged under root `deprecated/`, outside every build/runtime edge.
  Commit `bd8074c` closes the separate existing-durable-session entry by rejecting
  launch safely before executable binding, RoomPortal creation, hook installation, or
  PTY/ConPTY spawn. PTY/ConPTY custody, managed workspace hooks, and the persistent
  non-print command remain; dormant I/O and hook-receipt methods have narrow lint
  expectations rather than a module-wide exception. The prior path enumerated and
  sorted up to 20 provider-history candidates every 100 ms and could allocate/read a
  2 MiB tail; those costs and provider-private-history reads are gone. Focused tests,
  all 151 provider tests, Clippy with warnings denied, formatting, architecture/policy
  gates, and diff checks pass. No Antigravity turn or Phase 1 completion is claimed.
- Implemented pending Phase 1 whole-phase review: fixed-endpoint HTTPS ownership at
  `c43e161`. The previous DeepSeek-only client installed a second DNS resolver, rejected
  every non-public resolution, and forced `no_proxy` even though the credential target
  is a code-owned constant and redirects are disabled. No observed request forgery path
  justified that duplicate resolver, while it performed an extra lookup task and blocked
  ordinary system proxy policy. The common fixed-endpoint client now retains HTTPS-only,
  TLS hostname validation through reqwest/rustls, redirect denial, the ten-second connect
  timeout, and the progress-reset three-minute read-inactivity timeout, while using the
  platform resolver and configured proxy path. The `ip_network` dependency and 109 lines
  of resolver/test state are removed. This owner is intentionally not reusable for
  caller-selected Custom API URLs; their hostname/address validation remains a separate
  Phase 1 SSRF boundary. All 150 provider tests, Clippy with warnings denied, formatting,
  architecture/policy gates, and diff checks pass. No remote-provider implementation or
  performance gain is claimed by this refactor.
- Implemented pending Phase 1 whole-phase review: canonical RoomPortal provider-tool
  projection at `59785ed`. The DeepSeek adapter previously repeated eleven tool names,
  terminal-action classification, and replay-unsafe classification outside the MCP
  contract owner, and it silently omitted the existing bounded `read_attachment` tool.
  The RoomPortal contract now owns the exact twelve-name inventory plus tabletop,
  terminal, and replay-safety predicates; DeepSeek filters the actual MCP descriptors
  through those predicates and can invoke `read_attachment` only through the existing
  observation-bound attachment authority. Provider-specific wire formatting remains in
  the adapter. No new tool, permission, state, retry, fallback, polling, or timer was
  added. All 151 provider tests, focused policy/DeepSeek tests, Clippy with warnings
  denied, formatting, architecture/policy gates, and diff checks pass. No throughput or
  allocation improvement is claimed; the change removes a concrete cross-provider drift
  source before the remote API family is connected.
- Implemented pending Phase 1 whole-phase review: bounded OpenAI-compatible SSE at
  `2dc39cd`. The previous DeepSeek path requested a non-streaming response, retained the
  complete raw body up to 4 MiB, and then allocated a second decoded response even though
  the verified original flow used SSE. One 490-line transport/decoder owner now uses the
  maintained `eventsource-stream` framing parser, consumes reqwest chunks under the same
  4 MiB raw-response and request-context bounds, joins at most sixteen uniquely identified
  bounded tool calls, normalizes credential/rate-limit/HTTP/transport failures without
  response-body disclosure, and retains normalized token usage. It performs no reconnect,
  retry, fallback, heartbeat, or total-turn timeout. DeepSeek now requests streaming with
  usage, preserves its exact model/Thinking/tool-round/output contracts, and accepts a turn
  only after an explicit SSE `[DONE]` plus a compatible finish reason; EOF never infers
  completion. The existing reqwest read timeout resets on every successful read, so valid
  long-running work is not narrowed to a three-minute whole-turn deadline. The raw-body
  double allocation is removed, but no measured memory or latency improvement is claimed.
  Focused fragmented/truncated stream tests, all 153 provider tests, Clippy with warnings
  denied, formatting, architecture/policy gates, and diff checks pass. Real-provider and
  phase-wide integration evidence remain open.
- Implemented pending Phase 1 whole-phase review: provider credential account ownership
  at `80d667d`. The composition-owned secure store previously embedded the DeepSeek
  account name and exposed DeepSeek-specific forwarding methods, which would require
  copying the same keyring backend for each retained remote API provider. One exact
  credential-account enum now selects isolated keyring accounts for DeepSeek, Cerebras,
  OpenRouter, Vercel AI Gateway, LLM Gateway, TokenRouter, and Custom API while one
  semaphore still serializes the blocking platform store. DeepSeek keeps its existing
  `deepseek` account, public route, validation bounds, no-prompt status query, and
  fail-closed missing/invalid/store-unavailable behavior; its provider-specific error
  wording moved to the DeepSeek adapter. No additional provider, credential route,
  frontend state, retry, fallback, polling, timer, or cache was exposed. The provider
  account-isolation test proves that deleting one credential cannot delete another;
  all 155 provider tests, the real TCP credential boundary, Clippy with warnings denied,
  formatting, architecture/policy gates, and diff checks pass. This is a duplication and
  secret-collision prevention boundary, not a measured CPU, memory, latency, or disk
  improvement.
- Implemented pending Phase 1 whole-phase review: shared remote OpenAI-compatible runtime
  at `01e07f5` and `e03cacc`. The first commit is a pure source-owner relocation so the
  functional refactor remains independently reviewable and below the commit-size gate.
  The second keeps one 568-line cohesive owner for remote SSE turn, RoomPortal tool,
  attachment, completion, interruption-uncertainty, and shutdown state; immutable provider
  endpoint/header/request/error policy is a separate 80-line owner because it changes when
  a provider is added while runtime state does not. DeepSeek now supplies its exact endpoint,
  request body dialect, reasoning replay rule, credential account, and existing user-visible
  errors from its 107-line provider module. This preserves the existing catalog, credential
  route, selected model/Thinking/output values, required first room read, terminal-effect
  handling, provider-session identity, and stop behavior. No second provider, route, state,
  retry, fallback, polling, heartbeat, timer, or framework was added. All 156 provider tests,
  the exact DeepSeek request-profile regression, Clippy with warnings denied, formatting,
  architecture/source-policy gates, and diff checks pass. The refactor removes the imminent
  copy boundary for the retained remote family but claims no measured resource improvement.
- Implemented pending Phase 1 whole-phase review: Cerebras API vertical at `e8793c3`.
  The registered `cerebras_api` path now owns its exact public catalog endpoint,
  completion endpoint, `X-Cerebras-Version-Patch: 2` header, `gpt-oss-120b`
  preference, reasoning/output controls, provider-specific errors, and isolated
  keyring account. One exact private credential route is authorized before reading
  its bounded body, and the copied Agent-add flow exposes it only because the Rust
  registration now advertises that implemented operation. The provider uses the
  existing bounded remote SSE/tool runtime rather than a second execution state
  machine.
  The current original's gateway catalogs share one schema and projection function;
  a new 408-line common owner therefore performs only that proven-identical public
  HTTPS fetch, size/model bounds, cancellation, and model-option projection while
  provider policy remains in the Cerebras module. The retained one-request limits
  are the original 1 MiB response and 256 raw models, with an eight-second catalog
  deadline. They bound an untrusted public response before allocation/projection;
  no retry, fallback, polling, heartbeat, cache, background task, or response-body
  disclosure was added. On 2026-09-03 the real endpoint returned two compatible
  models including the preferred model and its advertised tool/reasoning/context/
  pricing metadata. Unlike the original, catalog failure or an absent compatible
  preferred model never substitutes the static manifest: failure is visible and
  non-startable, while a valid catalog without the preference requires explicit
  model selection.
  The commit changes 779 lines, its largest new owner is 408 lines, and the existing
  618-line catalog file remains one public selection-authority flow after the 500-line
  structure review. Repository-wide searches found no second Cerebras endpoint,
  header, credential path, catalog-bound, or execution owner. Focused provider,
  frontend, credential-TCP, and Clippy checks pass; a fresh complete `make verify`
  passes all frontend, desktop, Rust unit/integration, real TCP/WebSocket,
  generated-binding, Clippy, policy, structure, diff, CSS, and artifact gates. No
  Cerebras key was available for an authorized real completion, so real-turn evidence
  remains explicitly open for the Phase 1 provider matrix rather than being simulated.
- Implemented pending Phase 1 whole-phase review: OpenRouter API vertical at `d06da15`.
  The registered `openrouter_api` path now owns its exact completion endpoint,
  `HTTP-Referer`/`X-Title` headers, preferred `openai/gpt-4.1-mini` identity,
  output control, provider-specific failures, and isolated keyring account. Its one
  exact private credential route reuses the authorization-before-body owner, and the
  copied Agent-add flow exposes the operation only from the registered Rust capability.
  Request execution, SSE/tool completion, cancellation, secret redaction, and lifecycle
  remain in the common remote runtime; no second state machine or provider branch was
  added there.
  A live 2026-09-03 request to the original unpaged public URL returned 341 tool-capable
  models and 599,654 bytes, so the original 256-entry policy rejects the entire current
  response before exposing a usable provider. OpenRouter's live endpoint accepts a
  server-side `limit`, preserves the requested most-popular ordering, and returns
  `total_count` plus an explicit next link. This slice requests one finite 32-model page:
  the smallest tested page that retains useful breadth while its complete projected
  provider fits the existing 16 KiB per-provider and 48 KiB aggregate public catalog
  owners. A temporary real-discovery test exercised the production Rust path and proved
  a nonempty, startable OpenRouter projection inside both bounds; the test file was then
  removed rather than adding network dependence to the suite. The unrequested tail is
  not fetched, no page loop, background task, retry, fallback, polling, heartbeat, cache,
  or silent failure exists, and an absent preferred model remains visibly unselected.
  The feature changes 350 lines and its 117-line provider module is the largest new
  file. The existing 639-line catalog remains one public selection-authority flow after
  the 500-line review; identical remote output-token controls and public gateway
  fetch/projection now each have one owner. Repository-wide searches found no duplicate
  OpenRouter endpoint, header, credential path, catalog, or runtime owner. A fresh
  complete `make verify` passes frontend 99 files/656 tests, desktop, Rust unit/
  integration, real TCP/WebSocket, generated-binding, Clippy, policy, structure, diff,
  CSS, and artifact gates in 235.05 seconds with 1,779,924,992-byte maximum RSS. No
  OpenRouter key was available for an authorized real completion, so real-turn evidence
  remains open for the Phase 1 matrix rather than being simulated.
- Implemented pending Phase 1 whole-phase review: canonical snapshot provider catalog at
  `6c844b6`. `RoomSnapshot` previously serialized the same provider inventory twice:
  `provider_catalog.providers` was authoritative and `available_providers` was a direct
  clone that the browser accepted only when byte-equivalent, then discarded. The protocol
  and socket now emit only `provider_catalog`; the generated TypeScript contract and every
  live fixture derive from that owner, while the browser's trust-boundary decoder rejects
  the retired alias as an unknown top-level field. This removes one full serialized copy
  of every provider entry from each snapshot before adding the larger verified Vercel
  catalog, without changing catalog selection, operation exposure, UI state, or any room
  authority. No compatibility decoder, pagination framework, cache, retry, fallback,
  polling, heartbeat, timer, or alternate state was added. The 54 focused socket/canonical-
  room tests and production frontend build pass; a fresh complete `make verify` passes all
  99 frontend files/656 tests, desktop, Rust unit/integration, real TCP/WebSocket,
  generated-binding, Clippy, policy, structure, diff, CSS, and artifact gates in 236.28
  seconds with 1,941,831,680-byte maximum RSS. The existing 542-line browser validation
  file retains one cohesive trust-boundary responsibility after its 500-line review;
  splitting the snapshot key check would add an interface and key-list forwarding without
  separating state or an invariant.
- Implemented pending Phase 1 whole-phase review: Vercel AI Gateway vertical through
  `c70c9fe` and `3af62ec`. The verified original applies its 256-model ceiling after
  projecting compatible entries, while the shared Rust parser had incorrectly rejected
  any raw catalog above 256 entries. `c70c9fe` restores that one policy owner: 257 valid
  text/tool models fail closed, but arbitrary incompatible entries do not consume the
  compatible-model budget. No Vercel exception or alternate parser was introduced.
  `3af62ec` registers the exact `vercel_ai_gateway` HTTPS path, public `/v1/models`
  catalog, `/v1/chat/completions` SSE runtime, `openai/gpt-5.4-mini` preference,
  output control, provider-specific failure text, isolated keyring account, private
  authorization-before-body credential route, and capability-derived Agent-add surface.
  It reuses the existing gateway projection and remote SSE/tool runtime; endpoint,
  credential, default selection, request shape, and errors remain provider-owned.
  On 2026-09-03 the public endpoint returned 365 raw entries/373,107 bytes, of which
  229 were text/tool compatible. The production Rust path projected those 229 models
  to a 99,723-byte provider containing the exact preference, and the complete seven-
  provider catalog was ready at 124,630 bytes. Those measurements replace the obsolete
  16 KiB/48 KiB publication limits with 128 KiB per provider and 192 KiB aggregate
  absolute ceilings, leaving 64 KiB inside the unchanged 256 KiB WebSocket frame for
  room metadata. Separate regressions reject each enlarged bound and prove a 229-model
  catalog crosses the real WebSocket snapshot once, without the retired duplicate alias.
  The live measurement test was removed immediately; there is no network-dependent suite,
  page loop, retry, fallback, polling, heartbeat, timer, cache, or swallowed catalog
  failure. A fresh complete `make verify` passes all 99 frontend files/658 tests,
  desktop, Rust unit/integration, real TCP/WebSocket, generated-binding, Clippy, policy,
  structure, diff, CSS, and artifact gates in 200.28 seconds with 1,785,643,008-byte
  maximum RSS. No Vercel credential was available for an authorized real completion,
  so real-turn evidence remains explicitly open for the Phase 1 provider matrix.
- Implemented pending Phase 1 whole-phase review: remote catalog projection cleanup at
  `57a82e5`. Repository-wide consumer searches showed that `selection_kind`,
  `compatibility_evidence`, and the projected `tools` flag had no consumer, while an
  absent `relation_scope` already means the same global selection relation as the
  repeated `"global"` value. The shared projection owner now omits those four redundant
  per-model entries and retains the consumed model identity, label, family, context,
  output, price, vision, reasoning, description, and per-model reasoning-effort data.
  This is not a changed catalog policy or a provider-specific compression path: exact
  selection, the 256 compatible-model ceiling, and `per_model` relation validation are
  unchanged. A temporary live measurement using the production fetch/projection path
  found Vercel's 229-model option array reduced from 98,768 to 74,036 bytes and LLM
  Gateway's 249-model array from 98,811 to 74,233 bytes. The network-dependent test was
  removed immediately. The three deterministic projection tests, all seven selection
  tests, Clippy with warnings denied, formatting, architecture/policy gates, and diff
  checks pass. The selection group was run serially because its existing executable-
  validation tests share a four-permit filesystem worker; no product capacity, retry,
  fallback, polling, heartbeat, timer, cache, or failure handling changed.
- Implemented pending Phase 1 whole-phase review: LLM Gateway vertical through
  `2e3e645`, `eb982ac`, and `9759646`. The first commit consolidates the identical
  public-gateway fetch, empty-compatible-catalog rejection, exact preferred-model
  selection, and common output/permission control transition already copied across
  Cerebras, OpenRouter, and Vercel. Provider endpoint, display name, preferred model,
  and optional reasoning control remain explicit inputs; no provider switch, framework,
  state, or lifecycle owner was introduced. The second commit gives per-model relation
  metadata one exact representation for an optional empty value. An explicit empty
  entry now permits omission, while a nonempty-only relation still rejects omission;
  the remote gateway projection adds that empty entry because its shared request
  contract omits `reasoning_effort` when the user selects provider default. Existing
  Codex and Cerebras requirements remain unchanged.
  `9759646` registers the exact `llm_gateway_api` HTTPS provider, public `/v1/models`
  catalog, `/v1/chat/completions` SSE endpoint, `gpt-oss-120b` preference, optional
  default plus none/minimal/low/medium/high/xhigh/max reasoning control, output limit,
  provider-specific failures, isolated keyring account, private authorization-before-
  body credential route, and capability-derived Agent-add surface. It reuses the common
  bounded catalog projection and remote SSE/tool runtime; endpoint, credential, model
  and reasoning policy, request shape, and errors remain provider-owned. A temporary
  production-path request on 2026-09-03 observed 249 compatible models, retained the
  exact preferred model as startable, and produced a complete eight-provider public
  catalog of 172,032 bytes under the 192 KiB aggregate and 256 KiB WebSocket bounds.
  The network-dependent test was removed immediately. Repository-wide searches found no
  second LLM Gateway endpoint, credential route, catalog-transition owner, timer, retry,
  fallback, polling, heartbeat, cache, or swallowed failure. The 680-line catalog and
  551-line registration files remain respectively one public catalog state-transition
  owner and one declarative registration/launch table after the 500-line review;
  splitting either now would add forwarding interfaces without separating an invariant.
  A fresh complete `make verify` passes all 99 frontend files/660 tests, desktop,
  Rust unit/integration, real TCP/WebSocket, generated-binding, Clippy, policy,
  structure, diff, CSS, and artifact gates in 258.66 seconds with 1,806,270,464-byte
  maximum RSS. No LLM Gateway credential was available for an authorized real
  completion, so real-turn evidence remains explicitly open for the Phase 1 matrix.
- Implemented pending Phase 1 whole-phase review: TokenRouter API vertical at
  `9009cd9`. The registered `tokenrouter_api` path owns the verified original
  completion endpoint, public pricing-catalog endpoint and schema, isolated keyring
  account, `moonshotai/kimi-k3-free` preference, 4,096-token output ceiling, request
  shape, and provider-specific visible failures. The public projection accepts only
  bounded `Text` entries whose endpoint types include `openai` and enabled groups
  include `default`; the common catalog owner supplies only the identical HTTPS fetch,
  compatible-model ceiling, and ready-state transition. The private credential route
  authorizes before reading its bounded body, and Agent Add exposes that operation only
  from the registered Rust capability.
  The original kept a bundled same-provider model when public discovery failed. That
  fallback is not product semantics in the Rust target: discovery failure remains
  visible and non-startable, and a valid catalog missing the historical preference is
  ready with no selected model until the user explicitly chooses one. The production
  endpoint was dynamic during measurement: an initial 62,895-byte response held 134
  raw entries and the compatible count changed from 74 to 75 between observations, so
  no exact count is encoded as policy. The production Rust path projected 75 compatible
  models at the recorded observation and produced a complete nine-provider catalog of
  184,420 bytes under the 192 KiB aggregate and 256 KiB WebSocket frame ceilings.
  Network-dependent measurement code and its temporary response were removed.
  TokenRouter's pricing payload does not expose a sufficiently verified, stable dollar
  schema for the current UI contract, so the adapter does not infer `free` from a model
  name or publish uncertain prices. The original's bounded provider-error-body
  classification, including its TokenRouter exhausted-quota distinction, remains an
  explicit phase-wide normalized-failure hardening item; this breadth-first slice
  exposes failures but does not claim that final error-code parity yet.
  Repository-wide searches found no second TokenRouter endpoint, credential route,
  catalog policy, runtime owner, retry, fallback, polling, heartbeat, timer, cache, or
  swallowed catalog failure. The 208-line provider module is the largest new owner.
  The existing 711-line catalog and 592-line registration files remain respectively one
  public catalog state-transition owner and one declarative registration/launch table
  after the 500-line structure review; splitting either now would add forwarding
  interfaces without separating state or an invariant. A fresh complete `make verify`
  passes all 99 frontend files/662 tests, desktop, Rust unit/integration, real
  TCP/WebSocket, generated-binding, Clippy, policy, structure, diff, CSS, and artifact
  gates in 190.51 seconds with 770,097,152-byte maximum RSS. No TokenRouter credential
  was available for an authorized real completion, so real-turn evidence remains open
  for the Phase 1 matrix rather than being simulated.
- Implemented pending Phase 1 whole-phase review: Custom API backend at `22d37aa`
  and frontend connection at `5cd595f`. The provider accepts one caller-selected
  direct HTTPS OpenAI-compatible base URL or complete `/chat/completions` URL and one
  required model ID. Selection normalizes that URL once, stores it only in the private
  durable Agent Session, includes it in runtime profile identity, and runtime launch
  accepts only the identical normalized authority. The public session projection
  exposes neither the URL nor its credential. Runtime profile version 5 and schema
  version 56 reject older state without migration or compatibility decoding.
  The concrete threat was credentialed SSRF: an arbitrary hostname could otherwise
  send the Custom API bearer credential to loopback, link-local, private, reserved, or
  DNS-rebound addresses. The Custom API owner therefore requires HTTPS, rejects
  embedded credentials/query/fragment, local names and non-public IP literals, disables
  proxies and redirects, and validates every DNS answer on each connection. Fixed-host
  providers retain their existing platform resolver/proxy behavior; the extra resolver
  and its DNS lookup cost exist only while a Custom API runtime is launched. A mixed
  public/private DNS answer fails closed, and local-network models remain the Local
  provider's responsibility. No polling, heartbeat, retry, cache, fallback, background
  task, or silent error path was added.
  The catalog registration derives both caller-input requirements from one
  `ProviderConfigurationAuthority` enum, preventing two boolean authorities inside the
  registry; the two public capability bits are only its frontend projection. Shared
  code owns HTTPS/SSE/tool mechanics, while endpoint normalization, credential account,
  model ID, request shape, and visible failures remain Custom API-owned. The 856-line
  selection file was reviewed at the 800-line strong warning: it still owns one
  fail-closed catalog-to-session transition, while the new Custom API tests are a
  separate 100-line module; extracting the transition would increase state passing and
  interfaces without separating an invariant. The 661-line registration table likewise
  remains one declarative discovery/launch owner.
  Direct tests cover URL normalization, IPv4/IPv6 and mapped-address denial, runtime
  revalidation, catalog shape, selection/profile identity, private projection, isolated
  credential HTTP authority, and the actual modal request. A fresh complete
  `make verify` passes all 100 frontend files/664 tests, desktop, Rust unit/integration,
  real TCP/WebSocket, generated-binding, Clippy, policy, structure, diff, CSS, and
  artifact gates. No Custom API credential or user endpoint was supplied for an
  authorized real completion, so real-turn evidence remains open for the Phase 1 matrix.
- Implemented pending Phase 1 whole-phase review: local OpenAI-compatible Ollama and
  LM Studio verticals at `be12e21`. Each provider owns its executable, fixed numeric
  loopback endpoint, discovery commands and deadlines, model-capability policy,
  preferred model, labels, visible failures, and registration. Ollama inspects at most
  the first 32 inventory entries and admits only models whose `Capabilities` section
  declares tools; its installed versus cloud location is projected on each model so the
  existing frontend presents one provider in the Local and Harness groups without a
  second provider identity. LM Studio requires the exact running-server status and
  admits only loaded `llm` entries explicitly marked `trainedForToolUse`.
  The common OpenAI-compatible runtime now represents bearer-authenticated and
  unauthenticated transports as distinct authentication authorities. The latter is
  accepted only with a fixed `http://127.0.0.1:<port>` endpoint, disables proxies and
  redirects, emits no Authorization header, and retains the existing three-minute
  read-inactivity, request/response, SSE, tool-round, RoomPortal, and visible-failure
  bounds. This is not the child-process `LoopbackHttp` owner: Ollama and LM Studio are
  independently launched local servers, so claiming AA child ownership or attaching
  its private Basic capability would be incorrect. No local credential state, endpoint
  input, provider conversation store, polling, heartbeat, retry, fallback, cache, or
  background task was introduced. Discovery is one cancellation-aware bounded startup
  operation and every spawned probe tree is terminated and reaped by the existing
  process owner.
  Invalid LM Studio JSON is reported as malformed discovery rather than being collapsed
  into an empty model inventory. Model identifiers reuse the catalog owner's 128-byte
  option bound before becoming command arguments; common labels and HTTP/SSE mechanics
  are shared, while the two inventory schemas and state meanings remain provider-owned.
  Repository-wide searches found no second production endpoint, model-policy, local
  credential route, registration, retry, fallback, polling, heartbeat, timer, cache, or
  swallowed failure. The new owners are 317-line Ollama, 208-line LM Studio, and
  146-line local transport modules. Existing 738-line catalog, 663-line registration,
  and 660-line common runtime files remain below the 800-line strong warning and each
  retains one state-transition, declarative table, or turn/runtime invariant owner;
  moving provider logic into them was deliberately avoided. A fresh complete
  `make verify` passes all 100 frontend files/664 tests, desktop, all 180 provider
  tests, Rust unit/integration, real TCP/WebSocket, generated-binding, Clippy, policy,
  structure, diff, CSS, and artifact gates. The user-authorized real-provider matrix
  excludes Ollama and LM Studio, so no installed server or model was invoked and
  real-turn evidence remains explicitly uncollected rather than simulated.
- Implemented pending Phase 1 whole-phase review: Cursor official-ACP vertical through
  `383423b`, `a99b125`, and `22ebefc`, with dependency-induced canonical JSON
  corrections at `e1e3e4a` and `e32b2ac`. Read-only comparison against original
  `d5046473` found a persistent Cursor PTY/RoomPortal path, terminal-output completion,
  transcript support, and a second corrective prompt when the first prompt omitted its
  room-read receipt. The Rust path instead uses the currently installed official
  `cursor-agent acp` protocol-V1 server and the maintained `agent-client-protocol`
  library. Static `cursor-agent acp --help` and the bounded production
  `cursor-agent models` probe confirm both installed entry points without starting a
  provider turn.
  The Cursor module owns live catalog parsing and exact model/effort/tier tuples; one
  567-line ACP session owner keeps initialization, native new/load, exact model
  confirmation, prompt, bounded output, cancel receipt, and poison/restart invariants
  together. It was reviewed at the 500-line structure warning and remains below the
  800-line strong split signal; splitting that state flow would add cross-module state
  transfer and interfaces. The 380-line process owner separately owns executable and
  process custody, stderr drainage, and the private bearer-authenticated HTTP
  RoomPortal. The existing common adapter remains the only room-observation finalizer.
  There is no Cursor-specific room-tool declaration, polling, heartbeat, retry,
  transcript, terminal parsing, corrective second prompt, or completion fallback.
  ACP lines are capped at 256 KiB and assistant output at 128 KiB; startup/model/cancel
  handshakes are bounded, while ordinary long turns retain no invented total timeout.
  Exact native `cancelled` completion permits retained-runtime interrupt. Every ACP
  permission request currently rejects or cancels, so only `meeting_read_only` is
  advertised; workspace-write and actual RoomPortal tool approval remain explicitly
  unverified rather than permissively inferred. No real Cursor provider turn was run,
  because Cursor is outside the user-authorized Grok/Codex Luna/OpenCode Muse Spark
  real-run matrix.
  The ACP dependency enables insertion-preserving JSON maps workspace-wide. Complete
  verification exposed two pre-existing hidden order dependencies: canonical command
  hashes and room-settings revisions relied on the former map backend, while a central
  registration test mistook object member order for its exact-field contract. One
  61-line domain encoder now explicitly sorts canonical JSON object keys and preserves
  both public values; the central test still checks the exact field set and signed
  transcript without requiring meaningless wire order. Repository-wide searches found
  no second Cursor production owner; the Rust key-iteration search found no other exact
  JSON-object-order assertion. The three
  typed in-memory ACP wire tests use synchronization channels rather than sleeps and
  prove new/session model selection, durable load with a category-less exact `model`
  option, and native cancellation receipt. A fresh `/usr/bin/time -l make verify`
  passes all architecture/source/policy/diff and artifact gates, frontend 100 files/
  664 tests, desktop 25, domain 57, persistence 243, protocol 6, provider 186, server
  88, and every real TCP/WebSocket suite in 204.22 seconds with 2,012,725,248-byte
  maximum RSS. These are verification costs, not a Cursor runtime performance claim.
- Implemented pending Phase 1 whole-phase review: Grok official-ACP vertical through
  `0365a2b`, `7593404`, `6dd2016`, and contract tests `daa8fa0`. Read-only comparison
  against original `d5046473` confirms that the current product catalog excludes
  `[model.<id>]` entries from the native `grok models` inventory and launches the
  selected model as `grok [--permission-mode acceptEdits] agent --model <id>
  --reasoning-effort <low|medium|high> stdio`. The Rust path omits the superseded
  one-shot JSON/resume, transcript, print, and compatibility paths.
  One shared typed ACP client now owns protocol initialization, new/load session,
  prompt output, cancellation, and permission responses for Cursor and Grok. One
  shared ACP runtime separately owns verified-process custody, pipes, bounded stderr
  drainage, and private RoomPortal lifetime. Provider differences remain local:
  Cursor confirms and selects a session model and rejects all permission requests;
  Grok requires the process-selected initialization model before opening a session
  and permits only an exact canonical RoomPortal tool while the matching session,
  turn, and room observation are active. Conflicting identities, prefix/suffix
  impostors, inactive observations, native tools, and every other permission request
  fail closed. Native Grok tool permission remains explicitly unavailable pending
  whole-matrix hardening rather than receiving a broad approval path.
  Grok catalog discovery performs one bounded eight-second owned process probe and
  one optional two-second, 1 MiB-bounded configuration read per discovery. It adds no
  steady timer, polling, heartbeat, retry, fallback, transcript, silently ignored
  catalog failure, or second catalog cache. Provider state is isolated in a stable
  private `0700` directory keyed by room, Agent Session, and runtime profile beneath
  the database state root; the user's login file is referenced without copying
  credential data.
  ACP lines remain capped at 256 KiB, assistant output at 128 KiB, and protocol
  handshakes at ten seconds, while ordinary long turns receive no invented whole-turn
  timeout. The common void observation-abort callback and its ignored cleanup result
  remain open Phase 1 matrix work; this slice does not claim that contract complete.
  The four focused ACP/Grok contract groups pass, all 191 provider tests pass, all-
  target provider Clippy is warning-free, and architecture/source-policy gates pass.
  Static inspection of the installed official client found `grok 1.0.5`, the
  `grok agent stdio` entry point, protocol V1, load-session and HTTP MCP capability,
  and an exact initialized model receipt. No prompt or real provider turn was sent;
  the authorized Grok matrix flow remains open rather than being simulated.
- Implemented pending Phase 1 whole-phase review: canonical RoomPortal turn projection.
  Repository-wide comparison found the same `ProviderTurnRequest` to
  `RoomObservationStart` field mapping independently copied by Codex, OpenCode, the
  remote OpenAI-compatible runtime, and the shared ACP runtime. `RoomPortal` now owns
  that pure conversion and finish cursor once; provider-specific error mapping and ACP
  activation state remain with their existing owners. This removes 51 net lines and a
  fifth prospective Claude copy without changing room authority, lifecycle, transport,
  timing, or failure semantics. All 191 provider tests and warning-denied provider
  Clippy pass; no performance improvement beyond removed duplicate allocation/copy code
  is claimed.
- Implemented pending Phase 1 whole-phase review: Claude official Agent SDK vertical
  through `60c8bff`, `6bee248`, `8c256dd`, `c83c9f2`, `6a35424`, `436fc04`, and
  `d557a0d`. Read-only comparison against original `d5046473` found a persistent
  Claude Code PTY/ConPTY path whose turn authority depended on hooks, transcript
  observation, terminal parsing, and print-like output capture. The Rust path instead
  bundles the exact maintained `@anthropic-ai/claude-agent-sdk` `0.3.258` ESM runtime
  and a 328-line newline-JSON bridge; none of the old transcript, print, terminal
  scraping, compatibility, or second-prompt paths is connected to the build.
  The bridge accepts one canonical-v4 Agent Session and one turn at a time, verifies
  the SDK init receipt against the exact session, working directory, selected model,
  effort, fast state, permission mode, and sole private RoomPortal MCP server, and
  accepts success only when the final result repeats the exact session/turn/model and
  reports no queued turn. RoomPortal read-only tools are the only advertised tools and
  every permission request outside their exact names fails closed. Protocol lines are
  capped at 256 KiB and assistant output at 128 KiB; initialization and shutdown
  handshakes have ten-second bounds, while ordinary long turns have no invented total
  timeout. Native cancellation/interrupt is not advertised because an exact SDK receipt
  has not yet been proven; typed observation-abort and cleanup outcomes remain explicit
  whole-matrix Phase 1 work rather than a simulated success.
  A static, no-prompt probe of installed Claude Code `2.1.231` returned the current
  exact catalog `claude-fable-5` and `claude-sonnet-5`. The preferred Haiku model was
  absent, so creation requires explicit selection rather than substituting an arbitrary
  default. That probe completed in 1.24 seconds with 0.48 seconds user CPU, 0.29 seconds
  system CPU, and 398,524,416-byte reported maximum resident size. The 11,728-byte
  bridge and 1,513,260-byte SDK module are bounded bundle inputs. The installed Claude
  executable is 294,720,528 bytes; measurements exposed that macOS and Windows were
  copying it a second time after it was already byte-verified and privately bound.
  `d557a0d` reuses that protected bound path on those platforms, while Linux/Android
  retain one private companion because a Node child cannot use the sealed parent
  memfd path. A no-prompt catalog probe through a renamed `provider` hardlink proves
  the macOS child path remains executable. This removes one per-session 281 MiB copy
  without changing executable identity, process custody, room authority, or failure
  semantics.
  Unix process-group/guardian custody and Windows Job Object custody both own Node,
  Claude, RoomPortal, pipes, stderr drainage, and bounded shutdown. Windows source was
  added and native macOS compilation remains clean, but the corrected Rust Windows
  cross-compile stopped in dependency `aws-lc-sys` before project code because the
  host lacks `x86_64-w64-mingw32-gcc`; Windows compile and runtime evidence therefore
  remain unknown, not approved. All 195 provider tests passed after the receipt slice;
  after the platform and copy changes the focused three Rust Claude tests, three Node
  bridge tests, warning-denied provider Clippy, formatting, diff, architecture, policy,
  and source-structure gates pass. No real Claude prompt or provider turn was run.
  The 832-line `registration.rs` is a strong split candidate and was reviewed: it still
  owns one declarative provider table and its matching launch dispatch, while splitting
  now would add state transfer, interfaces, and glue without separating an independent
  invariant. It will be revisited if another owner or change reason enters. Repository-
  wide checks found no new polling, heartbeat, retry, fallback, transcript, print, or
  silently swallowed failure.
- Implemented pending Phase 1 whole-phase review: Freebuff static/fail-closed vertical
  at `477a69a`. Read-only comparison against original `d5046473` found no structured
  provider protocol: it repeatedly captured a PTY screen to infer model labels, slept
  between synthetic arrow-key navigation, treated the last 40 terminal lines as the
  public answer, cached inferred menu positions, and silently ignored room-publication
  failure. None of those polling, timing, screen-scraping, completion-inference, cache,
  or swallowed-failure paths was ported. The locally installed official Freebuff
  `0.0.154` help surface exposes only its interactive TUI, `--continue`, `--cwd`, and
  `login`; it has no prompt argument, headless/JSON output, ACP server, or other exact
  session/completion receipt. The official headless/SDK request
  `CodebuffAI/freebuff#947` remains open, and its own evidence distinguishes the
  Freebuff device token from the separately published
  paid Codebuff SDK API-key contract, so substituting that SDK would change both product
  and credential authority.
  Freebuff is now the sixteenth retained registration and uses the current `Harness`
  grouping. Discovery owns only bounded executable presence/identity. A present CLI is
  visibly `available` but not `startable`, with no model/default/control claims and the
  shared `provider_native_receipt_unavailable` failure; a missing CLI remains a distinct
  `command_missing` failure. The launch boundary independently fails before any process
  or provider effect, so a forged/stale stored profile cannot bypass catalog selection.
  The exact sixteen-provider registration-set test and Freebuff unavailable-state test
  pass with all 197 provider tests, warning-denied provider Clippy, formatting, diff,
  architecture, policy, and source-structure gates. No real Freebuff process, login,
  model, prompt, or turn was run. No polling, heartbeat, timer, retry, fallback,
  transcript, print, cache, background task, or credential state was added. The central
  registration owner is now 833 lines; the prior strong-warning review still applies
  because this one declarative entry added no second state flow or independent owner.
- Next production work: Phase 1 provider-first completion.
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
