# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity audit and planning
review are closed at reviewed content checkpoint `9711232`. Phase 0B foundation
correction is active from public baseline `4ab5ee1`.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: 0B foundation correction.
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
- Active task: F-11 frontend wire-contract generation ownership.
  - Definition: the Rust protocol exporter is the one semantic owner for room event,
    room settings, provider catalog, snapshot, Participant, and Agent Session wire
    values. Endpoint-local runtime decoders continue to own trust-boundary rejection.
  - Current defect: the copied `generatedRoomEvent.ts`, handwritten provider/snapshot
    interfaces, legacy `RoomAgentSession`/`RoomMember` fields, and a second React
    provider-array state independently describe values already generated by Rust.
    Several copied fields drive UI branches even though the Rust server never sends or
    accepts them.
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
    close those roots. Fresh complete repository verification passes in 163.03 seconds
    with 788,037,632-byte maximum RSS; correction re-review remains pending, so F-11
    is not closed.
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
