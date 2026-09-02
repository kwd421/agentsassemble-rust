# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity inventory at
`9711232` remains reviewed historical evidence. Its finding-number order is not
the production roadmap. The corrected provider-first plan is active pending its
manual documentation cross-review. Before Phase 1, the two confirmed live Phase 0B
prerequisites F-14 and F-16 must close without expanding into unrelated polishing.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: plan correction, finite Phase 0B prerequisite closure (F-14 and F-16),
  then Phase 1 provider-first implementation. No product implementation starts
  until this documentation correction is reviewed.
- Historical Phase 0B labels are not a serial global gate. D-06 executes with Phase 1
  runtime measurement/hardening; D-07 executes with Phase 5 human admission; split
  F-18/F-20 work remains with its already named external-admission/custom-channel
  owners. Reordering does not waive a finding; its owning phase cannot exit before
  the finding is closed or evidence-deferred.
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
- Next production work: Phase 1 provider-first completion.
  - Prerequisite only: replace F-14's browser request-ID compatibility fallback
    with fail-closed secure UUID creation and make F-16's closed catalog watch
    terminate instead of immediately re-entering the select loop. Verify each exact
    failure boundary, then start the provider breadth pass; do not turn this into a
    general frontend/socket refinement detour.
  - First establish the full sixteen-provider acceptance matrix and the smallest
    common registration, selection, start, ordinary-turn, visible-failure, and stop
    contracts. On Codex, Antigravity, OpenCode, and DeepSeek, remove only false or
    unsafe behavior that blocks that shared foundation; do not polish one provider
    while the rest of the retained structure is absent.
  - Then connect every retained provider to that real basic contract: Claude through
    the official Agent SDK; the remote API family
    (Cerebras, OpenRouter, Vercel AI Gateway, LLM Gateway, TokenRouter, and Custom API);
    Ollama and LM Studio; and the remaining original native providers Cursor, Freebuff,
    and Grok when its official ACP client is available.
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
