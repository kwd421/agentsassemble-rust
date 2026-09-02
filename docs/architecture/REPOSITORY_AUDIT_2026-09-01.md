# Repository-wide reimplementation audit — 2026-09-01

Status: manual source, history, duplication, and defensive-complexity audit
complete; the Phase 0A planning route is approved at reviewed content checkpoint
`9711232`. Product implementation remains paused. This record owns findings and
dispositions, not product contracts or the implementation sequence.

Audited baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`, Rust
`8a5f75a`. User-owned working-tree changes in `AGENTS.md` and `.agents/` were not
modified.

## Method and limits

The audit compared reachable original registrations and frontend entry points with
the current Rust protocol, domain, persistence, server, provider, desktop, and
frontend graphs. It also searched the complete Rust repository for repeated
semantic constants, SQL mechanisms, validation, state transitions, capability
lists, timers, retries, fallbacks, swallowed failures, dead/copied surfaces, large
files, and duplicated documentation authority. Defensive code additionally had to
identify a reachable use case, observed failure, or concrete in-scope threat and
was compared with the smallest fail-closed design preserving the same contract.

The following passed before this documentation change:

- `python3 scripts/check_architecture.py`
- `python3 scripts/check_source_growth.py` (125 source files produced the expected
  500-line responsibility warnings; no hard failure)
- `cargo test --workspace --all-features`
- `npm --prefix frontend test`
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Those commands prove only their tested/static boundaries. They do not approve the
product inventory, active UI reachability, provider-native protocols, ownership,
resource cost, or real user flows. No automated security scan, Deep Scan, real
provider, browser, or Computer Use session was run for this audit.

## Planning-history conclusion

The repository was not developed with no design. The initial commit `ea012c7`
created `Rule.md`, `SDD.md`, a Phase 1 workboard, an architecture document, one
core-room slice, and verification rules. By `87a6aec`, the workboard had moved to a
single Phase 2 Agent Session slice. Later work added sixteen detailed slice specs
and extensive review/verification evidence.

The missing item was a product-wide retained-feature inventory and dependency
plan. Neither workboard version mapped all current original behavior, provider
families, frontend entry points, or final sequence. The consequence is visible in
`87a6aec` (`343 files`, `+60,072/-2,390`): the original frontend and provider work
were copied in one large change, including surfaces that had no Rust owner and an
Antigravity transcript path later forbidden by the user. `99165dd`
(`+3,823/-814`) and `6de2671` (`+2,398/-1,053`) were also too broad for the current
independent-commit rule.

Therefore the accurate conclusion is: there was substantial slice-level design,
but no complete master reimplementation plan. Existing verified foundations are
not discarded, but no completion percentage is meaningful until the inventory and
authority map in `docs/PRODUCT_REIMPLEMENTATION_PLAN.md` are closed.

Manual review also found one published commit-order defect. `b558da5` introduced
the plan while referring to a complete finding register and `D-07`, but the register
did not exist in that tree and arrived in `aaeb74c`. The current tree resolves the
reference; the earlier commit is still not independently self-contained. Rewriting
public history is outside this correction, so this remains a recorded historical
rollback/review limitation rather than a defect claimed as fixed. Future commits may
refer only to authority present in the same commit.

## Initial external review findings

- Critical ChatGPT Pro: `REVISE — C0/H0/M3/L0`. Accepted findings are the
  lobby-only `all` search overclaim, the historical `b558da5`/`aaeb74c` ordering
  defect above, and D-02's evidence-free local receipt approval. The participant
  filter removal itself remains the user's approved UI difference, not a parity
  restoration task.
- Daybreaker Blue High manual review: `REVISE — C0/H0/M4/L0`. Accepted findings are
  the duplicated batch-threshold authority, the same D-02 evidence gap, one D-03
  current-versus-target wording error, and two competing frontend allowlist owners.

These are review findings, not approvals. Final reviewer dispositions are recorded
only after the correction commit is pushed and re-reviewed.

## Final external review closure

- Critical ChatGPT Pro and Daybreaker Blue High each closed their respective
  documentation findings and returned `APPROVE — C0/H0/M0/L0` for the Phase 0A
  planning state at reviewed content checkpoint `9711232`.
- Both reviews left F-20, D-02/D-03, the frontend allowlist, and the historical
  `b558da5` rollback/review limitation in their documented states. Approval closes
  the Phase 0A plan and finding route only; it does not close pending product work.

### Phase 0B D-01/D-02 manual cross-review closure

- Critical ChatGPT Pro initially returned `REVISE — C0/H0/M2/L2` for
  `4ab5ee1..640429e`: the master plan still routed completed D-02 work, the remote
  socket-ticket parser accepted the retired proof field, the 256 KiB frame policy
  had independent Rust/TypeScript owners, and the retired-envelope TCP test did not
  reproduce the canonical old base64 wire shape.
- Daybreaker Blue High initially returned `REVISE — C0/H0/M2/L2` for the same
  batch: it confirmed the parser, frame-limit, TCP-fixture, and stale proof-era
  vocabulary findings. Its correction reviews additionally found protocol-limit
  literals in integration assertions and unused proof-era frame/counter/async test
  harness ceremony.
- Corrections `e434496` through `aea2a06` close those findings without restoring a proof,
  compatibility path, or second policy owner. Critical ChatGPT Pro manually approved
  every correction, cumulative `640429e..aea2a06`, full `4ab5ee1..aea2a06`, and
  final public HEAD `aea2a06` at `APPROVE — C0/H0/M0/L0`. Daybreaker Blue High
  manually approved final public HEAD `aea2a06` at `APPROVE — C0/H0/M0/L0`.

### Phase 0B D-03 manual cross-review closure

- For `5d0b069..6ab175d`, Critical ChatGPT Pro returned `REVISE — C0/H0/M0/L3`
  and Daybreaker Blue High returned `REVISE — C0/H0/M0/L2`: stale grant lifecycle
  names, an over-narrow profile error, and inaccurate removed-cost wording remained.
- `787bbe5` closed those initial findings. Subsequent reviews found and closed an
  additional Pro Low 2 in `f5737cf`, then a Daybreaker Low 1 in `9e2c9f3`. A later
  repository-wide review found one Medium: the WebSocket `Room` ticket was also
  accepted by profile HTTP despite separate production issuers and callers.
  `f5ddb91` removed that compatibility path and its stale profile branches.
- Critical ChatGPT Pro and Daybreaker Blue High each manually approved exact
  `9e2c9f3..f5ddb91`, cumulative `5d0b069..f5ddb91`, and final public HEAD `f5ddb91`
  at `APPROVE — C0/H0/M0/L0` before the remaining targets.
- For final-target batch `ac905de..e68261e`, Critical ChatGPT Pro found two Low
  stale-contract strings in search and attachment code, while Daybreaker Blue High
  found two Low current-status/range errors in planning documents. `3e8dd1b` corrected
  the status owners and `5693e13` corrected the two source strings without changing
  behavior, state, or authority.
- Critical ChatGPT Pro and Daybreaker Blue High each manually approved exact
  `3e8dd1b..5693e13`, cumulative `ac905de..5693e13`, and final public HEAD `5693e13`
  at `APPROVE — C0/H0/M0/L0`. All D-03 target operations are closed.

## Fix findings

### F-01 — Antigravity transcript remains live

Disposition: `Fix first`; product/security/cost impact high.

`crates/agentsassemble-provider/src/lib.rs:1-16` includes the transcript module.
`antigravity.rs:75-124,335-356,375-449` creates it, polls every 100 ms, and accepts
its completion/session result. `antigravity_transcript.rs:135-177` enumerates and
sorts the provider history directory before truncating candidates, and later
parses bounded transcript tails. This duplicates completion and provider-session
authority, reads provider-private history, and creates repeated filesystem work.

Remove the module, launch nonce used only for transcript correlation, all history
discovery/parsing, and transcript-derived completion/session promotion from the
production graph. Preserve PTY/ConPTY and hook mechanisms. If the old code is kept
for the explicitly requested historical reference, it lives only under a clearly
non-build `deprecated/` boundary with no crate module, import, feature, test,
runtime, or fallback connection. An exact native signal may replace it; otherwise
Antigravity turn completion remains explicitly incomplete. Terminal silence, print
mode, scraping, or another provider is not an acceptable replacement.

The corrected architecture and Agent Session draft now mark this implementation as
historical/forbidden rather than current authority. Code removal and real-flow
evidence remain required before the finding closes.

### F-02 — Codex and OpenCode have multiple completion authorities

Disposition: `Fix or explicitly approve after native-protocol proof`; medium.

`codex_turn.rs:217-269,328-379,475-517` accepts multiple event/identity dialects,
final/delta combinations, and a one-second final-plus-idle inference.
`opencode.rs:415-540` uses prompt/SSE response and then an HTTP history read when direct content
is empty. Original provenance alone does not approve either path under the current
no-fallback rule.

Verify the installed current protocols. Choose one explicit completion/session
owner per provider. If history is the documented OpenCode primary response, model
it as primary rather than a fallback; otherwise remove it. Keep only the exact
current Codex event dialect or record a narrowly approved invariant with real
evidence.

### F-03 — cleanup and abort failures lose authority

Disposition: `Fix`; medium lifecycle/availability impact.

`driver.rs:40-58` makes observation abort return no result.
`runtime_turn.rs:293-300,366,380` and provider implementations in
`codex.rs:571-579`, `opencode.rs:592-605,786-792`, and
`deepseek.rs:420-428` discard abort/disconnect/portal cleanup errors.
`antigravity_hook.rs:69-100` clears registry ownership before best-effort disk
cleanup. `process.rs:147-150` discards probe kill/wait results. A failed portal
cleanup can leave an active observation that blocks the next turn.

Use one common fallible abort/poison outcome, provider-specific explicit teardown
where necessary, and typed residual-process/hook failure. `Drop` may remain only a
last best effort after the explicit owner has reported the result.

### F-04 — signed capabilities advertised actions that did not exist

Disposition: `Closed through 7b2168f; both manual reviewers approved`; high
product/medium structure impact.

Before `1a87de8`, the signed capability projection included `room.delete`,
`participant.kick`, `provider.request.resolve`, and `bridge.report`, although the
registered product surface had no matching executable action. The strict client
rejected those commands before send, so no permission bypass was found; the concrete
defect was a signed false product promise. `1a87de8` removes those four fields from
the single Rust capability owner and regenerated TypeScript contract. It retains
`bridge.publish` because the current vote authorization path consumes that exact
permission.

`5842f8a` removes the copied room-delete and participant-kick controls, command
construction, ACK validation, and obsolete tests. `e711def` removes the copied
provider-response controls and the otherwise unowned per-room provider-request React
state, normalization, model, card, tests, and CSS.

The first pushed cross-review found four remaining false owners. `cccf513` removes the
still-reachable `agent.readd` selector/command/ACK path, the always-empty
`RoomSnapshot.provider_requests` field and frontend provider-request wire vocabulary,
the producerless `participant_kicked` event projection, and the never-invoked
room-delete callback chain and synthetic tests. The exact server
`participant_kicked` start-denial error remains because it protects the current
lifecycle invariant; OpenCode's interactive provider-request rejection remains
because the current driver must fail closed instead of waiting for nonexistent UI.

The correction adds no route, authority, fallback, compatibility branch, retry,
timer, polling, heartbeat, or feature framework. Repository-wide searches found no
remaining frontend caller for the three absent actions. The later owning slices may
reintroduce a control only with its complete server action, authorization, transition,
failure, and real-user flow.

Critical ChatGPT Pro approved exact `5b1b331..fe8ffbf`, cumulative
`dd1e99d..fe8ffbf`, and HEAD `fe8ffbf` at `C0/H0/M0/L0`. Daybreaker found one Low:
two active verification passages and two server rejections still presented removed
re-add as available remediation. `d874d92` makes the rejection text authority-neutral;
the active verification owner now defers re-add to its complete participant/session
transition. Critical ChatGPT Pro and Daybreaker Blue High each approved exact
`fe8ffbf..7b2168f`, cumulative `dd1e99d..7b2168f`, and HEAD `7b2168f` at
`C0/H0/M0/L0`.

### F-05 — copied frontend actively calls missing Rust services

Disposition: `Fix exposure now, implement in later owning slice`; high product.

- `useFriendsDirectory` is enabled at startup for a resolved non-guest identity
  (`useAppController.tsx:244-261`) and calls absent `/api/room-friends` routes.
- `useRoomSideChat` is enabled for every active room
  (`useAppController.tsx:273-286`) and calls absent `/api/side-chat`; the right and
  mobile panels mount its controls (`AppView.tsx:521-532,624-642`).
- `CustomChannelView` is reachable (`AppView.tsx:481-500`) and polls absent custom
  text APIs every 2.5 seconds. Its deferred voice body polls every five seconds and
  sends a 20-second join heartbeat (`CustomChannelView.tsx:347-372`).
- external-AI invite, AI-friend, operator-pairing, and companion controls in
  `useRoomInviteController.ts` and `RoomInviteModal.tsx` target payloads/routes not
  owned by the strict human-invite Rust boundary.
- public Google account settings call absent `/api/account*` routes when opened.

Permanent 404s are not transient state. Disable these entry points and startup
effects until their complete server owner exists. Do not make the human route
permissive or add empty/dummy endpoints. Voice is user-deferred and must not run.

`FrontendUpdateNotice.tsx` also contains a 15-second missing-route poll, but no
production importer currently mounts it. It is dead/latent source, not an active
runtime cost; remove or leave unmounted until a runtime-version owner exists.

Candidate correction through `d058af8`: three independent frontend commits remove
the normal-startup Friends route and request owner, the active-room side-chat request
owner and panels, and the custom-channel selection/create/render path. The last change
also makes the copied 2.5-second text poll, five-second voice poll, and 20-second voice
join heartbeat unreachable from the production import graph. Normal startup now enters
the implemented lobby, and the right panel retains its implemented room-connection
information. No Rust route, placeholder, fallback, compatibility branch, retry, timer,
or generic feature abstraction was added.

The copied hooks and some presentation modules remain dormant source and some are still
referenced by the CSS provenance import owner; this batch does not claim their deletion.
At that checkpoint, `FrontendUpdateNotice` remained unmounted. The next F-05 review had to decide their removal
from actual reachability and CSS ownership evidence, then assess the still-exposed
external-AI/AI-friend invite, operator-pairing, companion, and public Google controls.
The initial candidate passed the complete repository gate. Critical ChatGPT Pro found
no issue and approved `8903445..cf71db4` at `C0/H0/M0/L0`. Daybreaker found three Low
dead-state remnants: mobile side-chat mode state and branches, a producerless canonical
socket callback, and a permanently-false custom-channel search branch. `a2b2f41`
removes those owners without replacing them. A fresh complete repository gate passes;
correction re-review confirmed the original three findings closed, then found two Low
source remnants and one Low documentation overclaim. `87d3d0d` removes the producerless
`onLobby`/`onRoster` handler contract and consumerless controller projections; the
verification record is narrowed to the exact side-chat callback removed. Final
repository gates pass. Critical ChatGPT Pro and Daybreaker Blue High each approved
exact `778d761..f74af57`, cumulative F-05 `8903445..f74af57`, and HEAD `f74af57`
at `C0/H0/M0/L0` with no remaining actionable finding.

The second candidate batch keeps these future authority boundaries unavailable instead
of borrowing human admission. `762ba40` removes the copied Room Connector card and its
controller projection; `11e167b` removes only operator-pairing issuance while preserving
incoming pairing redemption; and `7159c2d` removes the admitted-human companion card,
packet state, obsolete client call, and its now-unreferenced clipboard helper. The
current human invite and managed public-ingress owners are unchanged. The production
bundle emits a 5.18/2.15 kB `AdminPanel` chunk plus an 839.15/251.87 kB main chunk.
Summing raw chunk bytes before rounding gives 844.34 kB minified JavaScript; the
displayed per-chunk gzip figures total 254.02 kB. CSS is 169.13/29.62 kB after the
three removals; a fresh complete `make verify` passes 103 frontend files/643 tests,
desktop Clippy/25 tests, all Rust and real TCP boundary tests, and workspace
warning-denied Clippy. Dormant AI-friend invitation, public Google controls, and
evidence-backed dormant-source cleanup remain. Both manual reviewers found the Low
omission of the separate `AdminPanel` chunk from the first documentation total;
Daybreaker then found that first correction `95951d9` misstated the aggregate-rounding
method. Corrections through `7f2e878` close both documentation findings; Critical
ChatGPT Pro and Daybreaker Blue High each approved correction `9759d73..7f2e878`,
the original batch as corrected,
cumulative F-05 `8903445..7f2e878`, and HEAD `7f2e878` at `C0/H0/M0/L0`.

Feature commit `96a7573` removes the still-reachable public Google settings mount
and its sole `/api/account*` client, Google browser-script loader, and component tests.
The central startup identity path, admitted-guest recovery, and ordinary profile owner
remain distinct and unchanged. Repository-wide frontend search finds no remaining
`/api/account*` caller or Google Identity script owner. The focused profile suite,
complete 102-file/640-test frontend suite, production build, exact CSS gate, architecture
gate, and diff check pass. Dormant AI-friend and evidence-backed dormant-source cleanup
remain in F-05; no backend completion is claimed.

Feature commit `fd74b90` removes the producerless AI-friend invite state/actions,
obsolete moderator/host-token `createRoomInvite` client, optional controller session
credential, and the unreferenced invite-copy helper. The current managed-human invite
and public-ingress owner is unchanged. Repository-wide frontend search finds no active
caller or exported AI-friend packet action; the focused 13-test controller suite,
complete 102-file/639-test frontend suite, production build, unchanged exact CSS gate,
architecture gate, and diff check pass. Evidence-backed dormant-source cleanup remains
in F-05; no AgentBridge backend completion is claimed.

Feature commit `d45afb5` deletes the unmounted `FrontendUpdateNotice` source,
including its absent-route 15-second interval, focus/visibility triggers, permissive
response defaults, and blanket failure catch. No runtime-version caller or polling
constant remains in frontend source. The production build, updated exact CSS artifact
hash, architecture gate, and diff check pass; no route, timer, retry, fallback, or
replacement abstraction is added. Other evidence-backed dormant-source cleanup remains
in F-05.

Critical ChatGPT Pro and Daybreaker Blue High then manually reviewed every individual
commit in the batch, exact `a9dceae..d45978a`, cumulative F-05
`8903445..d45978a`, and HEAD `d45978a`. Both returned `APPROVE C0/H0/M0/L0`
with no actionable finding and without an automated security scan.

Candidate `84dbc3a` removes the `originalImportOrder` behavior-module import chain
that existed only to reproduce global component-CSS traversal. One
`styles/componentOrder.ts` owner now imports the same 14 production component
styles in their measured emitted order; the component modules no longer own global
CSS side effects. The exact CSS filename and SHA-256 gate remains unchanged.
Removing the forced graph edges reduces transformed production modules from 2,196
to 2,189. Emitted JavaScript raw bytes remain 835.41 kB total, while the displayed
per-chunk gzip sum increases from 251.51 to 253.02 kB because module ordering changed;
the accepted 1.51 kB gzip cost avoids restoring non-product imports merely to tune
compression.

Candidates `daadd8d` and `9ee4952` then remove the separately rollbackable dormant
Friends presentation and client owners. Repository-wide frontend search finds no
remaining `RoomFriend`, Friends directory hook, or `/api/room-friends` request.
These commits do not implement or remove the retained future Friends product
contract; the original project remains its behavior reference for Phase 5. They do
not yet remove dormant Friends selectors from the copied original stylesheet.
Production CSS and JavaScript remain unchanged after the stylesheet-owner candidate,
and the reduced suite passes 101 frontend files/628 tests. No route, placeholder,
feature flag, polling, retry, fallback, or client authority replaces the removed
absent-service path.

Critical ChatGPT Pro and Daybreaker Blue High manually reviewed each individual
commit, exact `168bb32..91a071f`, cumulative F-05 `8903445..91a071f`, and HEAD
`91a071f`. Both returned `APPROVE C0/H0/M0/L0` with no actionable finding and
without an automated security scan.

Candidates `b723715`, `1521067`, and `8cd8628` next remove the independently
rollbackable dormant side-chat presentation, browser state owner, and absent HTTP
client/parser contract. The active entry points and canonical socket callback were
already removed, and repository-wide TypeScript search now finds no side-chat symbol
or `/api/side-chat` route. This removes 848 unreachable source/test lines without
replacing them with a flag, fallback, timer, retry, placeholder, or client authority.

The production CSS/JavaScript artifacts remain unchanged and the reduced suite passes
98 frontend files/617 tests. This is a source/state ownership reduction, not a runtime
performance claim. Phase 6 retains future server-owned side chat and the original
reachable behavior as its reference; dormant copied CSS remains an explicit open
cleanup boundary.

Critical ChatGPT Pro and Daybreaker Blue High manually reviewed every individual
commit, exact `e0c6ad0..d338936`, cumulative F-05 `8903445..d338936`, and HEAD
`d338936`. Both returned `APPROVE C0/H0/M0/L0` with no actionable finding and
without an automated security scan.

Candidates `84ceb13`, `adf17ee`, and `2f439f1` remove the separately owned dormant
side-chat, Friends directory/activity, and Friends profile/DM styles after their
presentation consumers are absent. Repository-wide source search confirms no
non-CSS consumer for those selectors. The active `.dc-agent-add-entry` rule and
human-invite `.dc-invite-friend-*` family are deliberately retained; dormant
HomeSidebar CSS remains a separate open boundary.

The exact CSS artifact shrinks from 168.34/29.49 kB to 154.83/27.47 kB by displayed
raw/gzip measures while JavaScript remains 835.41/253.02 kB. All 98 frontend
files/617 tests pass after every candidate. This is an unreachable-source and
delivered-byte reduction, not a runtime CPU or latency claim. Future Phase 5 Friends
and Phase 6 side-chat behavior remain server-owned work; no fallback, timer, retry,
placeholder, compatibility path, or alternate client authority replaces them.

A fresh complete `make verify` passes in 238.79 seconds with a 573,587,456-byte
maximum resident set. Critical ChatGPT Pro and Daybreaker Blue High independently
found one documentation-only L1 in `66d2b22`: 147 deleted CSS source lines were
called declarations. `bbfb710` corrects the unit. Both reviewers then approved the
corrected exact `5d0ee87..bbfb710`, cumulative F-05 `8903445..bbfb710`, and HEAD
`bbfb710` at `C0/H0/M0/L0` with no remaining actionable finding.

Candidates `f16bc2c` and `fbb952f` close the remaining evidence-backed F-05
dormant-source items: HomeSidebar-only CSS and the unimported Python-mirrored
provider-permission helper. The active room sidebar/button styles and every named
deferred or future product surface remain. CSS is 152.64/27.14 kB by displayed
raw/gzip measures; the frontend passes 98 files/617 tests. Full repository
verification passes in 239.05 seconds with a 579,043,328-byte maximum resident set;
cross-review is pending.

### F-06 — Agent Session identity SSoT is reversed in frontend projection

Disposition: `Fix`; high product/medium ownership impact.

The intended contract in `docs/specs/user-profile-slice.md:14-24` assigns Agent
display/avatar/provider/model/runtime to the Agent Session and assigns role, mute,
membership, and permissions to the room participant. In
`canonicalRoomProjection.ts:68-101`, Agent Sessions are projected first and then
participant display/avatar/provider fields overwrite them.
`useMemberEntries.ts:83-110` and other roster paths also prefer participant identity in places.

`AgentIdentitySettings.tsx` sends display/avatar fields through `agent.configure`
and the generic attachment route, but `configuration.rs:6-48,113-130` owns runtime,
model, and persona settings and preserves existing display identity. The current
editor therefore has no complete Agent profile mutation or asset lifecycle.

Make the Agent Session authoritative for Agent identity fields, merge only
room-owned participant state, and hide the editor until a distinct complete Agent
profile/avatar owner exists.

Candidate `fbebad6` corrects the first shared projection owner. Participant data is
projected first for room role, then an Agent Session with the same participant ID
authoritatively replaces display name, avatar, and provider. Current/paged lobby
history and lobby search use this one map. Focused tests pass 35/35 and the complete
frontend passes 98 files/618 tests. Subsequent commits `fe568bb`, `e0a681d`, and
`97816b5` apply the same owner to desktop/mobile rosters, mentions, and
typing/progress labels without moving room role, mute, or membership out of the
participant. Commit `11d3528` removes the incomplete editor and its invalid generic
attachment/runtime-configure path; it does not invent the missing Agent profile
mutation or avatar lifecycle owner. Daybreaker Blue High found and approved the
typing correction, then reported only stale current-state documentation. Packaged
restart verification now confirms canonical identity in roster, mention, typing,
timeline, and search for Codex Terra, Antigravity Flash, and OpenCode Muse Spark
sessions. Actual Antigravity and OpenCode turns complete. Codex start instead
exposes `runtime_start_recovered_gone` and remains an explicit lifecycle failure.
Correction `e869f42` reuses the existing provider-logo owner for avatarless Agent
search results while preserving human initials and custom Agent avatars. Daybreaker
Blue High found that its broad descendant-image selector also restyled the nested
provider-logo image; `0363622` restricts the custom-avatar rule to a direct child.
Post-correction packaged visual closure remains `unknown`: central guest creation
stayed pending on two bounded attempts, the real Antigravity turn failed on an
unapproved terminal command, and the required OpenCode Muse Spark model was absent
from the current catalog. No substitute, retry loop, fallback, or placeholder was
used. The Codex lifecycle failure and future complete Agent profile mutation/avatar
owner keep F-06 open. Daybreaker Blue High and critical-web Pro each found the same
documentation-only Low in `e912e75`: its current CSS SHA was paired with the prior
artifact's gzip byte count. `7566d3f` corrects that value to the independently
reproduced 26,579 `gzip -9` bytes. Both reviewers approved `0363622`, `7566d3f`,
exact `12430b1..7566d3f`, original search batch `35bc375..7566d3f`, cumulative
F-06 `8903445..7566d3f`, and HEAD `7566d3f` at `C0/H0/M0/L0`; the superseded
`e912e75` remains an individual `C0/H0/M0/L1` result.

### F-07 — provider catalog and controls overstate real operations

Disposition: `Fixed through d081761; reviewed through e13210f`; medium
product/structure impact.

- Before `5b94ac6`, `catalog.rs` substituted the first discovered model when the
  preferred model was missing, and the OpenCode preference still named
  `opencode/hy3-free`, contrary to the selected Muse Spark flow.
- Commits `582a02e`, `edfb7c5`, and `c890a9a` close the operation-exposure half of
  this finding. The Rust registration descriptor now advertises the credential exposure
  fact only where operations exist, so DeepSeek retains its separately owned keyring
  operations while Codex, Antigravity, and OpenCode expose no login or credential
  controls. The absent catalog-refresh and provider-usage requests, their response types,
  and their client
  state are removed. Daybreaker found that the dynamic quota presentation and
  visibility machinery remained without a producer and also influenced roster
  ownership. Correction `1313aba` removes those types, formatters, visibility rules,
  components, and styles; the static unsupported notice remains, and roster ownership
  now follows explicit `owner_id` only. Daybreaker's re-review found an obsolete
  quota-visibility suite outside Vitest's include set; `534a953` removes that final
  excluded test, and the deleted helper has no repository reference. Critical-web
  Pro then found that a secondary roster projection still treated Agent Session
  `external_owned=false` runtime custody as viewer ownership. Correction `9256976`
  removes that derived ownership state and viewer fallback. Replacement critical-web
  Pro then found that the primary `LiveAgent` path still substituted `agent.owner_id`
  for an existing room participant with no owner. Correction `4c1bd57` makes that
  present participant's room-owned `owner_id` the sole grouping authority and leaves
  an empty owner unassigned; the no-participant `LiveAgent` presentation case remains
  explicit and separate. Critical-web Pro and Daybreaker then independently found the
  same stale-owner precedence in the reachable mobile roster and mention suggestions.
  Correction `703b5c6` applies the member-presence rule to both consumers;
  `8beb103` also prevents an owner display
  label from merging unrelated ownerless desktop/mobile presentation groups.
  Daybreaker's next pass found that the secondary mobile projection used mutable room
  role to identify a human; `020d89f` now uses participant kind, preserving room
  ownership when an Agent has the Human role. Critical-web Pro and Daybreaker manually
  re-reviewed public HEAD `b6d844b` and each approve individual `703b5c6`, `8beb103`,
  `020d89f`, and `b6d844b`, exact `f934382..b6d844b`, full correction
  `879db4b..b6d844b`, cumulative `5ec012f..b6d844b`, and HEAD `b6d844b` at
  `C0/H0/M0/L0`.
- Commit `5b94ac6` makes the exact advertised
  `opencode/muse-spark-1.2-contributor-free` the sole OpenCode preference and leaves
  a ready nonempty catalog unselected when that preference is absent. Backend exact
  selection remains authoritative; empty catalogs and invalid non-model defaults
  still fail closed. Commit `1959a08` removes the same first-option substitution from
  the frontend scoped projection and requires an explicit exact selection, even for
  one candidate. Commit `d081761` removes the producerless `stale_cache` source,
  status branch, and test without replacing it. Both manual reviewers approve each
  source commit at `C0/H0/M0/L0`; each revises exact `67303e0..d081761` and HEAD
  `d081761` only at `C0/H0/M0/L1` for the stale current-state records corrected in
  `e13210f`. Each then approves individual `e13210f`, corrected exact
  `67303e0..e13210f`, and HEAD `e13210f` at `C0/H0/M0/L0` with no actionable
  finding.

Require an exact verified preferred model or explicit unavailable/unselected state.
Advertise login, credential, usage, refresh, and model operations only from their
real provider operation owner. Do not add dummy routes.

### F-08 — one global HTTP connection budget may let public traffic starve local control

Disposition: `Closed at c0cb3e2; evidence corrected at c184739; reviewed through c47dbbb`;
medium availability impact.

The prior owner admitted 128 connections before ingress classification with no
public partition. In the controlled counterfactual, 128 trusted-public requests could
all remain active; the 128th received 200, consuming the final total permit until a
connection released it. Each request used the real manual-public proxy headers and
the server's `100 Continue` response as a barrier while its body remained incomplete.
This establishes reachable contention without treating it as an unbounded remote
exploit.

Commit `c0cb3e2` moves both HTTP limits and rejection observation into
`http_admission.rs`. The existing total remains 128. After canonical ingress trust,
each classified-public connection retains one of 127 public permits for its lifetime.
At the real TCP boundary, 127 active public body requests make the next public request
return 503 while a local `/healthz` request returns 200; releasing those connections
and reading every terminal TCP response lets public admission return 200 again. The
trust decision remains solely in `ingress_trust.rs`. Correction `c184739` makes that
release observation deterministic instead of treating client-socket drop as proof
that the server had already released the permit.

Critical-web Pro and Daybreaker Blue High each manually approve individual
`c184739`, documentation correction `c47dbbb`, exact `952fa96..c47dbbb`, and
HEAD `c47dbbb` at `C0/H0/M0/L0`. Neither found a new authority, lifecycle,
resource, fallback, polling, retry, or structural issue in the corrected range.

This is deliberately a partition inside the existing resource ceiling: it adds one
semaphore, one optional public permit and connection-owned synchronization state, but
no sockets, tasks, buffers, listener, polling, heartbeat, retry, timer, or fallback.
Before full headers exist, traffic is still unclassified and can occupy all 128 total
permits for the existing three-second header deadline. That residual limit is
explicit; the correction guarantees progress under stable classified-public
saturation, not absolute pre-classification DoS prevention.

### F-09 — human invite guide and accepted aliases need current-client proof

Disposition: `Fixed and reviewed through 0fd931d`; low.

The reachable original frontend at `d504647` and the Rust frontend each send only
exact `human`. Commit `0e38579` replaces the human-admission denylist with that one
canonical accepted value; provider and external AgentBridge participant kinds remain
outside this browser admission owner. Commit `cae8d64` states the durable terminal
transition: an expired session is not renewed from the same admission key and the
host must issue a new invite. Commit `e76cda7` removes the browser's unsupported
60-second early-expiry skew. Daybreaker's two Low findings—duplicated guide TTL and
an obsolete same-invite E2E fixture—are corrected in `9821433`; its manual re-review
approves the complete range and HEAD at `C0/H0/M0/L0`. Critical-web Pro final review
of the requested `e76cda7` snapshot independently found the same projection Low plus
this stale current-state audit/workboard Low, revising the range and snapshot at
`C0/H0/M0/L2`. Commits `9821433` and `0fd931d` correct both. Critical-web Pro and
Daybreaker Blue High each approve the individual corrections, exact
`9821433..0fd931d`, full correction `e76cda7..0fd931d`, cumulative F-09
`820e427..0fd931d`, and HEAD `0fd931d` at `C0/H0/M0/L0` with no actionable finding.

### F-10 — DeepSeek keyring-to-environment priority is an unapproved fallback shape

Disposition: `Resolved by 2f0177b`; low security/product impact.

Current-original `d5046473` confirms that `ProviderSecretStore` prefers the platform
keyring and then reads `DEEPSEEK_API_KEY`. The first Rust cutover preserved that
priority, while the reachable copied control exposed only status, secure set, and
secure delete; it had no environment-source selection or revoke operation. Deleting
the keyring item could therefore reveal a second authority the same UI could not
remove.

Commit `2f0177b` removes the environment capture at the credential-store owner and
the retired response value at the strict frontend boundary. Keyring absence now
means `missing`, deletion becomes visibly missing, and a later provider turn fails
with the existing credential-missing result. The installed-store error continues to
fail closed rather than being confused with absence. This chooses the smaller of the
two audited dispositions: no source-selection state, launch-source abstraction,
compatibility decoder, schema, SQL, cache, task, timer, polling, retry, or fallback
was added. The 559-line credential file remains one cohesive backend/store/error and
test owner; splitting it would expose its private backend state without separating a
second invariant.

### F-11 — frontend wire-contract generation has two owners

Disposition: `Fix before further event-shape work`; high compatibility/structure
impact.

`frontend/src/types/generatedRoomEvent.ts` claims to come from
`scripts/generate_room_event_types.py`, but that generator is absent. Active socket,
history, search, and provider-request code imports this hand-maintained shape while
`frontend/src/types/generated/RoomEvent.ts` is the actual `ts-rs` output generated
from Rust by `crates/agentsassemble-protocol/src/bin/export_types.rs`. The orphan
shape also imports a separate legacy `RoomAgentSession`; `api/agentSessions.ts` and
`api/room.ts` define more legacy-shaped session/participant types; and
`participantEventContract.ts` manually repeats Participant, Agent Session, and
persona field lists. `roomSocketTypes.ts:77-142` then hand-defines provider catalog
and snapshot shapes again, including fields the current server does not own such as
`work_harness_available`, `custom_endpoint`, `custom_model`, `executable`, and
`interactive`.

Choose the Rust protocol/code generator as the single wire owner, then keep strict
runtime validation against generated metadata or one deliberately maintained
boundary schema. Do not preserve two generated-looking models or replace them with
another handwritten union. Share semantic generated types and constants, not a
universal decoder: snapshot, live event, history, search, and provider-request
boundaries keep separate finite envelopes, errors, and runtime rejection. Verify
them together because they currently consume the split owners.

Implementation plan and evidence:

1. Remove the orphan `generatedRoomEvent.ts` and make live, history, and search use
   the real `ts-rs` `RoomEvent`/`PublicRoomSettings` output. Dynamic event extras stay
   local to the boundary that understands them; no replacement global event union is
   introduced. Completed: the 101-line generated-looking duplicate and its separate
   Python-generator claim are gone; timeline-only extras now have a local projection
   while search, history, live sequencing, and participant-event validation retain
   their existing runtime rejection owners. This is a type-ownership correction, not
   a claimed runtime optimization: generated bindings, the production build, and 83
   focused event/settings/boundary tests pass without a fallback or default value.
2. Replace the handwritten provider catalog interfaces with aliases of the generated
   types and one socket-local finite validator. Keep `interactive`, which current Rust
   really serializes. Remove `work_harness_available`, `custom_endpoint`,
   `custom_model`, and public `executable`, which current Rust neither serializes nor
   accepts as catalog-owned controls. React currently retains the catalog and a second
   independently parsed provider array per room and performs two state updates per
   catalog transition; the Rust owner already bounds one public catalog at 48 KiB, so
   retain only the catalog state and derive its provider list without claiming a wire-
   size reduction. Completed: generated aliases now own provider/catalog/control
   semantics; a finite socket validator rejects extra private fields, malformed catalog
   updates, and snapshot provider aliases that disagree with the catalog owner. The
   second React array and its second transition update are gone, as are the producerless
   custom-endpoint/work-harness controls. `interactive` and all server-produced catalog
   controls remain. Snapshot equality adds one bounded serialization comparison over at
   most the existing 48 KiB catalog; that small admission cost is accepted to reject two
   conflicting authorities before projection. Production build and all 622 frontend
   tests pass; no wire-size or speculative throughput improvement is claimed.
3. Derive the browser Agent Session and room-member projection from generated
   `AgentSession` and `Participant`. These are two commits rather than one mechanical
   type cleanup: an Agent Session owns runtime identity and lifecycle, while a
   Participant owns room role, join state, mute state, and room membership. Reuse one
   exact schema per generated owner for snapshot and event rejection, while history
   pagination, search context, live sequencing, catalog update, and command ACKs keep
   separate error semantics.

   The Agent Session half is complete. `RoomAgentSession` now aliases the generated
   Rust type, one compile-time exact key list backs snapshot/live-event validation,
   and agent command acknowledgements reject partial or extra session projections.
   The producerless HTTP resume endpoint, `share_activity`, copied latency/profile/
   stderr/protocol counters, and the session-avatar fallback are removed. Activity
   visibility remains a local viewer preference; an Agent Session without a future
   Agent-profile owner displays its provider identity instead of borrowing the room
   participant avatar. This removes duplicate state and false controls rather than
   claiming a runtime speedup. Generated bindings were unchanged after export; the
   production build, 86 focused tests, and all 624 frontend tests pass. The current
   815.99 kB production JavaScript asset is recorded only as an observed build result,
   not attributed to this ownership correction. No retry, timer, polling path,
   fallback, compatibility decoder, or new abstraction was added.

   The Participant half remains the next independent change. It will remove legacy
   participant kinds and the independent member `thinking` signal only after room-
   owned role/join/mute semantics are derived from generated `Participant` and proven
   through the real snapshot/event paths.

Initial manual cross-review of public `a07ecdf` found four actionable issues. Critical-
web Pro assigned one cumulative Medium to partial `RoomEvent`/`PublicRoomSettings`
acceptance. Daybreaker assigned one Low to that boundary, one Low to catalog-absent
synthetic controls and manually unbound generated key lists, and one Low to missing
Agent Session participant/create-ACK cohesion. Pro approved every individual source
commit but revised cumulative `dff4b65..a07ecdf` and HEAD at `C0/H0/M1/L0`;
Daybreaker revised the three source commits and cumulative HEAD at `C0/H0/M0/L3`.

Correction `77eea8a` makes the endpoint-owned socket validator require current event
version, timestamp, exact nonempty actor, generated optional-field types, and exact
public room settings. Settings ACKs now require the event and result projections to
match field-by-field; the downstream actor projection no longer substitutes flat
legacy fields or an empty identity. The old `activity_plugin` default is absent.
Correction `fd473e8` displays only controls actually serialized by the catalog and
compile-ties catalog/provider/control/option key lists to generated types. Correction
`9320494` requires nonempty room/session/participant identity, binds state events to
their participant, and requires the created event, top-level Participant, and top-
level Agent Session projections to agree.

The concrete threat was an authenticated but malformed or conflicting socket body
crossing the canonical browser boundary and becoming a second authority. The added
work stays at that owner: one room-settings comparison over the server's existing
50-channel maximum and one create-ACK comparison over 38 Agent Session plus 11
Participant fields. It adds no steady task, timer, polling, heartbeat, retry,
fallback, compatibility path, cache, or framework. The accepted trade-off at reviewed
HEAD `1c9f397` was this bounded admission work and an 818.10 kB production JavaScript
artifact in exchange for terminal rejection before projection; no throughput or size
improvement is claimed. Focused correction tests pass, all 98 frontend files/633 tests
pass, and the production TypeScript/Vite build plus original CSS gate pass. Fresh complete
repository verification passes every gate in 163.03 seconds with 788,037,632-byte
maximum RSS.

Daybreaker's first correction re-review returned `REVISE — C0/H0/M1/L1`. The Medium
was a reachable create/start mismatch: Rust commits `agent_session_created`, join,
attach, and final `agent_session_state` events for the default startable-provider UI
path, but the browser required the ACK's final event to remain the creation event. The
server also replaced `result.event` without updating `result.event_seq`. The Low was
the workboard's stale use of “Current defect” and premature root-closure wording.
Correction `71acb41` updates the final sequence at the persistence owner and validates
the exact four-event creation/start transition, its nested start projection, and the
deduplicated replay; stopped creation remains the separate one-event contract. This is
bounded ACK admission only and adds no task, retry, timer, polling, fallback, or
compatibility decoder. Persistence create/start tests (4), the actual server TCP
create/start/replay boundary test, frontend contract tests (5), TypeScript/Vite and
original CSS build, and architecture/policy gates pass. The focused build reports an
820.14 kB production JavaScript artifact as an observation only. Fresh complete
verification passes all 98 frontend files/635 tests, 25 desktop tests, the Rust
workspace including TCP and WebSocket boundaries, Clippy, generated bindings, and all
repository gates in 247.45 seconds with 1,860,009,984-byte maximum RSS. Its production
JavaScript artifact is 820.35 kB, again an observation rather than an improvement claim.
The next manual re-reviews found two residual sites. Daybreaker returned
`REVISE — C0/H0/M0/L1` because nested create/start events and the final ACK event were
compared with their outer event only by ID, sequence, room, and type; different actor or
display payloads could therefore become a second projection. Critical-web Pro returned
`REVISE — C0/H0/M1/L0` for the cumulative correction because a standalone
`room_settings_updated` event reached live, history, and snapshot through the shared
event validator without validating its nested generated `PublicRoomSettings`. Pro also
assigned the earlier correction-document commit one Low for claiming that root closed
before this remaining path was checked.

Correction `98a9af9` compares each server-duplicated create/start event as one complete
JSON projection and rejects nested or final actor/content disagreement. Correction
`a8583f0` adds the missing event-type rule to the existing endpoint validator, so the
same exact settings owner now applies to command ACK, live, history, and snapshot events.
These are bounded trust-boundary comparisons over already admitted socket values; they
add no state, task, timer, polling, heartbeat, retry, fallback, compatibility decoder,
cache, or framework. The accepted cost is serialization of at most the four existing
create/start event copies and one exact settings check before projection. The generated
semantic types, endpoint-specific envelope/error/sequence owners, room settings product
behavior, and create/start persistence transition are unchanged.

The focused Agent Session/settings/history tests pass 25 cases. Fresh complete
verification passes all 98 frontend files/640 tests, 25 desktop tests, all Rust unit and
real TCP/WebSocket suites, generated bindings, warning-denied Clippy, diff, structure,
policy, CSS, and artifact gates in 164.33 seconds with 791,937,024-byte maximum RSS. The
production JavaScript artifact is 820.38 kB and is recorded only as an observation.
At reviewed HEAD `f7c1277`, `participantEventContract.ts` was exactly 500 lines and
owned one cohesive Participant/Agent Session/create-ACK boundary invariant. Splitting
would expose its exact parsing internals or add cross-file glue, so the warning alone did
not justify a mechanical split. Both corrections remained subject to final Pro and
Daybreaker re-review; no final approval was claimed.

Daybreaker's next source review found a reachable Medium omitted by the frontend-only
fixture: `room_settings.rs` returned the committed event but not its `event_seq`, while
the strict ACK admission above requires both projections. This was not a reason to relax
the browser contract. Correction `6c799f5` adds the sequence at the persistence owner and
an actual TCP/WebSocket settings-update test that proves the ACK sequence equals the
published event and that an exact duplicate request replays the same result without a
second event. The first full gate passed all product tests but rejected an enlarged
scheduler scenario under the existing Clippy function-size rule. Follow-up `06c7139`
moves the result/replay invariant into its own focused persistence test; it adds no
production state or abstraction.

The correction adds no task, timer, polling, heartbeat, retry, fallback, compatibility
path, or second policy owner. It preserves room authorization, settings validation,
durable ordering, idempotent replay, and endpoint-local rejection. Fresh complete
verification passes all frontend, desktop, Rust unit, real TCP/WebSocket, generated,
Clippy, diff, architecture, policy, CSS, and artifact gates in 197.35 seconds with
1,295,138,816-byte maximum RSS. Final correction re-review remains pending, so no final
approval is recorded.

Daybreaker manually approves individual `6c799f5`, `06c7139`, and `07007c8`, exact
`f7c1277..07007c8`, cumulative `dff4b65..07007c8`, and HEAD `07007c8` at
`C0/H0/M0/L0`. Pro's completed review of the preceding `f7c1277` HEAD found one further
Low: `agent_session_state` could disagree with its nested Agent Session on duplicated
session, runtime, display, or participant identity, and create/start could repeat one
internally consistent but producer-impossible final session across every event copy.

Correction `a01502f` compares those state-event aliases to the already exact nested
Agent Session and checks the narrow fresh-start transition from the created event's
prepared session to `status=attached`, `runtime_status=idle`, and `enabled=true`, while
requiring all non-transition fields to remain unchanged. It deliberately does not copy
the full Rust lifecycle state machine. The owner file is now 546 lines, above the
500-line inspection signal but below the 800-line strong split signal; its one boundary
invariant remains cohesive and extraction would expose private matchers plus glue.

The correction adds no task, state owner, timer, polling, heartbeat, retry, fallback,
compatibility path, cache, or framework. Focused Agent Session tests pass 12 cases; the
production build and approved original CSS cascade pass. Fresh complete verification
passes all repository gates in 164.23 seconds with 776,175,616-byte maximum RSS. Final
Pro and Daybreaker re-review of the latest pushed correction remains pending, so no
approval is claimed for `a01502f` or its containing range.

No frontend provider-request consumer remains after the reviewed F-04 removal, so
this correction verifies its absence instead of recreating it. Custom providers,
alternate harnesses, Agent profile mutation, voice, and Mafia remain separate future
product slices. Completion requires focused boundary tests, generated bindings,
production TypeScript/Vite output, repository structure/diff gates, and complete
verification, with each of the three source changes independently buildable and
rollbackable below 1,000 changed lines.

### F-12 — one DeepSeek turn has no complete wall-clock/cost budget

Disposition: `Measure and bound at the owning turn`; medium resource/cost risk.

`deepseek.rs:33,158-191` permits an initial completion plus sixteen tool rounds.
The shared remote client gives each response read up to three minutes
(`remote_https.rs:17-26`), so the API portion alone can occupy roughly 51 minutes in
the worst case, before room-tool execution. Per-request and round bounds are useful,
but they do not bound the complete user operation, process slot, credentialed
network cost, or cancellation latency.

Measure real standard/deep tool flows, then add the smallest complete-turn deadline
or budget that preserves required provider behavior. Record observed latency/cost,
accepted maximum, cancellation owner, and uncertain-effect result. Do not shorten it
from intuition or add a retry/fallback.

### F-13 — provider factory erases guardian/helper construction causes

Disposition: `Fix typed startup reporting`; medium lifecycle/diagnostic impact.

`registration.rs:383-413` converts guardian re-execution, production guardian, and
Windows helper binding failures into `Option` through `.ok()`. Later launch code can
report only that the guardian/helper is unavailable, losing the actual custody
failure and making packaged diagnosis depend on inference.

Retain the construction result or a bounded redacted typed cause until the launch
owner reports it. Absence may remain a valid explicit provider-unavailable state;
it must not silently select weaker custody or another launch mechanism.

### F-14 — room command IDs have an isolated weak fallback

Disposition: `Remove fallback and share one fail-closed primitive`; medium replay
identity risk.

`roomSocketClient.ts:123-129` falls back from `crypto.randomUUID()` to wall-clock
plus an in-memory counter. The same shipped client already treats missing secure
UUID support as fatal for device identity and admission intent. A command request
ID is durable replay authority, so changing its uniqueness contract only on this
path is both duplicated policy and an unapproved compatibility fallback.

Use one secure request-ID owner and surface failure before command admission. Keep
domain-separated server operation IDs and durable replay validation; do not weaken
those checks merely because the browser generated the identifier.

### F-15 — CSS order is owned by behavior-module side effects

Disposition: `Fix before further frontend decomposition`; medium coupling/dead-code
risk.

`App.tsx` imports `app/originalImportOrder.ts`, whose only stated purpose is CSS
cascade preservation but which side-effect imports 46 API, hook, behavior, deferred
plugin, component, and utility modules. This makes dead or unsupported behavior
appear reachable, lets unrelated module edits change visual order, and hides the
actual stylesheet dependency graph.

Replace it with one explicit stylesheet-order entry that imports styles only.
Behavior modules must be imported solely by their real owner. Verify the copied
desktop geometry and cascade before/after; do not reorder styles opportunistically
or rewrite the frontend.

### F-16 — closed provider-catalog watch can busy-spin every room socket

Disposition: `Fix first`; high CPU/availability impact.

`room_socket_session.rs:251-254` awaits `catalog_updates.changed()` but uses
`continue` when the watch sender is closed. A closed Tokio watch returns an error
immediately on every later call, so the socket's `select!` loop can spin without
yielding useful work for every connected room session. Repository-wide search found
no other swallowed closed-watch loop with this shape.

Treat sender closure as terminal owner failure: close the affected session or
permanently disable that branch with one explicit visible state, according to the
catalog owner's shutdown contract. Do not add sleep, retry, polling, or another
catalog source to mask it. Add one focused closed-channel CPU/progress regression
test rather than a broad timer test suite.

### F-17 — malformed OpenCode SSE data is silently discarded

Disposition: `Fix protocol failure visibility`; medium correctness/latency impact.

`opencode_sse.rs:165-179` correctly ignores non-`data:` SSE lines, but also catches
JSON decoding failure for a `data:` line and continues. Provider protocol corruption
can therefore be hidden as a later timeout or a different event, changing both the
reported cause and completion behavior.

Keep blank/comment/non-data SSE handling, but make malformed `data:` a stable
provider-protocol error immediately. No retry, alternate parser, history lookup, or
extra abstraction is needed. Verify split chunks plus one malformed data line.

### F-18 — external invite controls use an obsolete host-token client path

Disposition: `Remove legacy path`; keep external AI unavailable until its owner
exists; high product/authority impact.

The canonical human flow uses `api/humanInviteManager.ts` and purpose-bound desktop
tickets. External-agent and AI-friend branches in `useRoomInviteController.ts`
instead call the older `api/invites.ts` client, which sends `X-Host-Token` through
`api/http.ts`. Rust owns no such token authority and its human invite endpoint
requires a consumed purpose ticket, so these controls cannot succeed. No production
caller establishes the old saved host-token state either.

Preserve the canonical human path. Remove the legacy token state/helpers and old
create/operator-pairing calls. Keep Room Connector and AgentBridge controls
unavailable until their separate complete credentials, permissions, and lifecycles
exist; do not widen or commonize the human route.

### F-19 — active frontend and domain input limits drift

Disposition: `Fix policy ownership before further settings UI`; low correctness/UX
impact.

The room label owner accepts 128 characters in `room_settings.rs:12`, while the
active settings UI silently slices to 80 in `RoomSettingsModal.tsx:203`. Channel and
profile limits currently match but are repeated as hand-written UI numbers. Decide
the reachable room-name product policy, keep server validation authoritative, and
generate only matching named UI hints from the existing contract exporter. An
intentional narrower UI limit must be documented; no form-schema framework is
needed.

### F-20 — room-wide search silently returns lobby-only results

Disposition: `Fix exposure in Phase 0B; complete in the custom-channel slice`;
medium product correctness impact.

The user-approved UI correction in `8c13a5c` intentionally removed the separate
right-panel participant filter, retained one room-named message search, made `all`
the default scope, and added participant-ID avatar projection. The search label and
scope control now promise every readable channel (`ChannelHeader.tsx:382-412`), but
`useAppMessageSearch.ts:20-24` sends `all` to a server that always calls the lobby
store (`message_search_web.rs:99-117`) and always projects `channel_id = "lobby"`.
The frontend response contract likewise accepts only lobby results. The original
`message_search/routes.py:96-110` expanded `all` across lobby and custom text
channels, so this is a silent narrowing rather than a completed room-wide search.

Until the custom-text message/search owner exists, expose only the implemented
lobby/current scope and describe it truthfully. The later custom-channel slice may
restore a real room-wide union. Do not add a client-side merge, dummy endpoint,
fallback, polling path, or generic search framework.

## Consolidate findings

### C-01 — participant JSON load/save SQL mechanism is repeated

Disposition: `Consolidate narrowly`; medium drift risk.

The exact `SELECT participant_json` and `UPDATE participants SET participant_json`
mechanism is repeated across `authority.rs`, `agent_lifecycle.rs`,
`agent_reconciliation.rs`, `provider_turn_reconciliation.rs`,
`room_turn_support.rs`, `participant_mute.rs`, `participant_roles.rs`,
`participant_leave.rs`, `room_user_identity.rs`, `profile_store.rs`, and admission
code. Authorization and transitions are intentionally domain-local; the row
encoding/load/save mechanism is not.

Introduce only small transaction-scoped `load_participant_by_key` and
`save_participant_exact` helpers with explicit missing/rows-affected results. Do
not create a generic repository framework or move domain authorization into it.

### C-02 — capability keys and permissions digest have three owners

Disposition: `Consolidate or remove redundant digest`; medium outage drift risk.

`CapabilitySet` is serialized/generated from Rust, but `server_proof.rs:42-63`,
`frontend/src/lib/serverProof.ts:13-28`, and integration subscription-proof support
separately list the same fourteen keys.
Adding a capability can make the frontend reject an otherwise signed snapshot.
The snapshot is already hashed and signed as part of the subscription contract.

Remove the redundant permissions digest with the frame-proof layer in D-02. Keep
capabilities in the Rust-generated snapshot and strict client decoder; do not add
another ordered key list or signed projection.

### C-03 — provider-turn envelope limits are repeated

Disposition: `Consolidate semantic values; Keep independent revalidation`; low.

The 20,000-character provider input/view limits, 96-KiB encoded-view ceiling,
64-Agent-ID cap, and several semantically distinct 128-byte identifiers repeat in
`runtime_turn.rs`, `provider_turn_reconciliation.rs`, `room_turn_context.rs`,
`room_portal.rs`, `codex_turn.rs`, and `room_turn_support.rs`. The 12,000-character
message limit also repeats across live domain, persistence, provider, and RoomPortal
boundaries. The transcript code removed by F-01 is not a shared-contract consumer.

Put each product semantic value and calculation in one domain/contract owner.
Do not merge unrelated identifiers into a generic `MAX_ID` merely because their
current number matches.
Keep validation at producer, durable decoder, adapter, and child-tool trust
boundaries; repeated checking is not duplicated policy.

### C-04 — profile attachment identifier validation is duplicated

Disposition: `Consolidate`; low.

The same attachment-ID predicate exists in domain profile code and persistence
profile-attachment code. Export the domain predicate and use it in persistence;
storage existence and lifecycle checks remain persistence-owned.

### C-05 — RoomPortal tool identity is duplicated in DeepSeek

Disposition: `Consolidate`; low.

`deepseek.rs:504-553` repeats tool names/filtering while
`room_portal.rs:37-49` and `room_portal_mcp.rs:102-276` own the actual descriptors
and router. RoomPortal should expose one canonical descriptor set; DeepSeek owns
only its supported subset and tabletop/tool-choice policy.

### C-06 — provider-neutral RoomPortal owns Codex CLI syntax

Disposition: `Move to existing owner`; low coupling risk.

`room_portal.rs:244-277,612-615` builds Codex `-c` flags. RoomPortal should provide
endpoint, bearer, tools, and approval contract; `codex_config.rs` or the Codex
driver should own Codex command syntax. No new provider framework is needed.

### C-07 — documentation repeats status, review verdicts, and optimization history

Disposition: `Consolidate by ownership, not line count`; medium maintenance risk.

`docs/VERIFICATION.md` is 5,093 lines with 78 second-level sections and hundreds of
review/approval references. Before this correction its header still named public
Rust `fdb4e49`; the draft now distinguishes pushed origin `1c5b37e` from this audit's
local baseline `8a5f75a`.
`human-invite-admission-session-slice.md` is 1,582 lines and mixes durable contract
with a long implementation journal; dated verification records repeat additional
history and old batching rules.

Keep product sequence in the master plan, current implementation facts in
architecture, feature contract in its slice, exposure in the gap map, and dated
execution evidence in one verification record. Later extract or retire duplicated
history only after links and unique evidence are mapped; do not mechanically split
at a line count.

### C-08 — provider operation policy is split across registry and UI branches

Disposition: `Consolidate at the existing registration owner`; medium drift risk.

Provider discovery and creation already have a Rust registration. Commit `582a02e`
extends that existing owner with one bounded `credential_available` fact and removes
the frontend provider-name inference that produced the false login/credential
controls in F-07. Commits `edfb7c5` and `c890a9a` remove unsupported refresh and usage
operations instead of inventing descriptor fields without a server owner. Correction
`1313aba` removes the now-producerless dynamic quota contract and decouples roster
ownership from quota visibility instead of preserving a speculative future surface. Exact
model selection is closed at its existing Rust catalog and frontend projection owners
through `5b94ac6` and `1959a08`; `d081761` removes the dead cached-source branch.
Provider-native authentication and model rules stay in their modules. Do not build a
speculative plugin framework.

### C-09 — future-only activity-plugin state occupies the live room contract

Disposition: `Remove unless a retained current flow is proven`; low current cost,
medium schema/ownership debt.

`RoomSettings.activity_plugin` crosses domain parsing, public settings, persistence,
HTTP preferences, generated TypeScript, frontend parsing, and tests, but the general
plugin framework is not a current retained product and RimWorld is deferred. This is
state and validation without a reachable owner.

Prove a non-deferred current consumer or remove the field through one clean-schema
change before implementing more settings. Do not keep it merely for future
extensibility and do not replace it with a generic extension bag.

### C-10 — dead copied surfaces resemble live product contracts

Disposition: `Gate or remove by retained-surface ownership`; medium maintenance
risk.

The unmounted Admin component, deferred Mafia/RimWorld paths, old broad `api.ts`
calls, and unimported
`frontend/src/lib/liveAgentPermissionOptions.ts` remain beside active code. Some
contain routes, timers, permissions, or protocol vocabulary that no Rust surface
owns. Their presence caused both false parity claims and repeated polling reviews.

Keep source only when a named retained phase owns it and the entry point is safely
gated. Remove otherwise; deferred product work belongs in plans and history, not a
half-live compatibility surface.

### C-11 — Agent Session state vocabulary is repeated as strings

Disposition: `Consolidate vocabulary; Keep transition authorities separate`;
medium drift risk.

`model.rs:306,362-364`, `agent_lifecycle.rs:158-168,301-318`,
`agent_reconciliation.rs:307-340,525-669`, `agent_reconciliation_recovery.rs:47-68,116-132,284-288`,
`runtime_reconciliation.rs:54,217`, and `runtime_recovery.rs:92-93` compare the same
runtime and lifecycle-intent strings. Put only the finite serialized vocabulary in
the domain model. Lifecycle predicates remain with `agent_lifecycle_authority.rs`,
turn transitions with `room_turn_scheduler.rs`/`turn_authority.rs`, and
reconciliation classification with its recovery owner. Provider observation,
orchestration, transactions, side effects, and error mapping also remain at their
boundaries; no generic state-machine framework is approved.

### C-12 — Agent Session row encoding and entity row writes repeat

Disposition: `Consolidate only entity-specific primitives`; medium drift risk.

Agent Session JSON selection/decoding repeats in `agent_lifecycle.rs:357-395`,
`agent_creation_records.rs:279-332`, and `agent_reconciliation.rs:719-735`.
Profile updates also repeat at `profile_store.rs:210-214` and
`human_admission_identity.rs:155-159`. Add only exact optional-load/save primitives at its persistence
owner; callers keep missing/stale meaning, authorization, transitions, and transaction
scope. Reuse the existing profile update owner from admission. A room-event row insert
may be shared only below event construction, sequence, and indexing. A generic CRUD,
codec, or repository abstraction would be overimplementation.

### C-13 — identical internal wire constants have multiple producers

Disposition: `Consolidate per protocol`; low drift risk.

Browser-device credential prefix/length repeats in `deviceIdentity.ts:6-9` and
`human_browser_credential.rs:4-7`; human-session bearer format repeats in
`human_admission_store.rs:25-27` and `human_session_bearer.rs:4-7`; the guardian ready
marker repeats in `guardian.rs:23` and `unix_custody.rs:31`. Export each
from its existing protocol/provider owner. Generation, parsing, canonical decoding,
fingerprint/signature checks, authorization, and boundary errors remain independent;
do not add a common authentication or wire-constant crate.

### C-14 — Codex bundle identity meaning is duplicated

Disposition: `Consolidate pure identity only`; low TOCTOU drift risk.

`filesystem_authority.rs:142-153` and `codex_executable.rs:15,205-206` independently
spell the native bundle identity and companion name. Share that pure meaning at the current Codex
owner. Canonicalization, open-handle identity, commit-time revalidation, launch-time
binding, and staging protect different reachable races and must remain separate. No
generic executable-manifest framework is needed.

## Defensive-complexity findings

### D-01 — HTTP host challenge has no production caller

Disposition: `Removed` at `a7949bd`; Phase 0B.

At the audited baseline, `host_ticket.rs` and the private `/api/host-challenge` plus
`/api/ws-ticket` routes added a 30-second challenge map/mutex, HMAC exchange, and startup secret.
Desktop production obtains its socket ticket over the owned private control pipe;
admitted humans use the authenticated session exchange. Repository-wide search
found no production frontend caller for the challenge routes. `a7949bd` removes the
routes, cache, HMAC, and secret preamble while retaining control-pipe EOF ownership
and both real ticket issuers. Actual TCP tests prove both retired routes return 404;
the private control-pipe and admitted-human ticket paths remain covered.

### D-02 — local receipt, remote proof, and per-frame proof have different threats

Disposition: `Removed` at `3ffb9eb`, `77cae0e`, `0d24741`, and `57fd6ec`;
Phase 0B.

`authenticated_channel.rs`, `server_proof.rs`, and `authenticatedFrames.ts` HMAC,
counter, base64-encode, copy, and asynchronously serialize every frame, adding
roughly one-third wire expansion. The native proof key and loopback frames cross
different paths, but repository evidence shows synthetic payload tampering rather
than a packaged endpoint-substitution or active-relay reproduction. The current
separate-path topology is a hypothesis, not evidence that another proof lifecycle is
required. Remote key delivery and frames cross the same HTTPS/WSS ingress, so remote
HMAC is not independent authority and is removed.

Before retaining any local receipt, record the attacker capability, a controlled
packaged reproduction or equivalent concrete topology evidence, and why the native
owner's exact child liveness/identity check, issuer-runtime-local one-use ticket, and
separate ingress checks cannot fail closed. Without that evidence, remove the local proof key and receipt and
use ordinary bounded JSON after the ticket is consumed. Local per-frame proof requires
separate evidence of an in-scope active relay; if proven, authenticate the Snapshot
and every later bidirectional product frame as raw bounded UTF-8 bytes with direction
and counter under one cached key. In every outcome remove base64, repeated key
derivation, snapshot proof, and permissions digest, and keep
one-use tickets, TLS/origin/ingress checks, strict schemas/limits, the finite `C/H`
handshake, sequence, request-ID replay, uncertain-ACK recovery, and one-time
native/sidecar product-surface equality.

No qualifying endpoint-substitution evidence was found. `3ffb9eb` therefore removes
the client challenge, HMAC receipt, snapshot digest, and duplicate permissions digest
from both generated wire types and their client/server owners. It deliberately leaves
the still-active per-frame key and envelope visible for the next independently
verifiable removal rather than hiding a dual protocol or compatibility path.

`77cae0e` completes the product-frame cutover to one 256 KiB strict JSON owner while
retaining the finite receipt/Snapshot/catch-up sequence, event continuity, request-ID
replay, uncertain-ACK recovery, ingress admission, and the one keepalive required by
the server's five-minute idle-input deadline. `0d24741` then removes the disconnected
HMAC modules and the second 64-hex random key from every ticket grant, including HTTP
grants that never used it. `57fd6ec` removes the obsolete authenticated-frame test
vocabulary and unused ticket/counter harness parameters. Repository search finds no
remaining proof-key, connection-nonce, authenticated-envelope, or frame-HMAC owner in
current server, desktop, frontend, or test source. No fallback or compatibility path
was retained.

### D-03 — remote human HTTP authorization performs a redundant ticket exchange

Disposition: `Simplify`; Phase 0B.

For profile, preferences, pins, search, and attachments, the browser presents its
session bearer to mint an exact-purpose ticket, then the target consumes that
ticket and revalidates durable authority. This adds an HTTP round trip, another
authorization snapshot, shared map/lock work, memory, and a separate capacity
failure without protecting against XSS or an unauthenticated network peer. Let the
target route accept the session bearer only from its bounded Authorization header
and perform one route-specific durable authorization. It never enters a URL, body,
log, event, prompt, fixture, or durable row. Keep WebSocket upgrade tickets and desktop purpose tickets: they
cross distinct WebSocket and private-control-to-WebView boundaries.

### D-04 — fixed DeepSeek HTTPS uses a custom SSRF defense without SSRF input

Disposition: `Simplify resolver; Decide proxy policy`; Phase 1.

`remote_https.rs` accepts one compile-time DeepSeek host, disables redirects, and
still replaces DNS with a public-IP-only resolver plus `.no_proxy()`. Normal TLS
hostname verification already rejects DNS rebinding to an impostor; the custom
resolver breaks split DNS, VPN, and corporate networks without stopping a trusted
CA compromise. Remove it for the fixed endpoint. Keep HTTPS, TLS, no redirects,
timeouts, response/tool bounds, and the separate strict SSRF owner for user-chosen
Custom API endpoints. Keep `.no_proxy()` only if an explicit credential/proxy
policy and operating evidence justify it.

### D-05 — runtime handle contains a parsed but unused identity

Disposition: `Simplify narrowly`; Phase 1.

`runtime_handle.rs` encodes boot identity, launch token, and another random UUID;
the parser validates then discards the last UUID, while the generation-unique
launch token is also stored separately. Remove only that suffix in the clean
current schema. Preserve platform/boot/token proof, the independently adoptable
owner ID, and execution/effect `(handle, owner, token)` stale-CAS snapshots. A new
cross-repository authority type is not approved unless it demonstrably reduces
state, comparisons, and glue.

### D-06 — recovery and staging mechanisms are justified, exact cadence/cost is not

Disposition: `Keep mechanism; Measure cadence and scan`; Phase 0B.

The runtime reconciler owns a real post-checkpoint owner-loss window and scans only
unresolved/blocking rows, but no evidence makes one second uniquely correct.
Guardian staging cleanup prevents observed orphan growth, but its bounded 1,024
entry scan lacks start/stop p95 evidence. Measure idle SQL/CPU, recovery delay,
entries/bytes scanned, copy cost, and cleanup latency before changing either
cadence or bound; do not replace them with an unproven fallback.

### D-07 — invite self-description claims have no proven consumer

Disposition: `Unknown; prove or remove`; Phase 0B.

The signed human-invite token contains fixed descriptive claims such as
`host_verifies`. Room, server, lineage, scope, expiry, and credential binding have
real consumers; repository search did not prove an external consumer for every
constant claim. Confirm the finite current clients, then remove unconsumed claims
instead of preserving self-description for possible future readers.

## Keep findings

### K-01 — asset lifecycle separation and accounting

Disposition: `Keep`.

The current implementation separates profile current+pending replacement,
pre-join avatar transfer, room-owned applied appearance, and message attachments.
`asset_storage.rs` owns one hard physical occupancy calculation using
`current - replaced + new`; it does not evict referenced/old assets to satisfy a
quota. Expired pending and exactly unreferenced replacement assets are deleted by
their lifecycle owner. The old per-user 64-item/128-MiB generic profile policy is
not the current owner.

### K-02 — trust-boundary revalidation is not policy duplication

Disposition: `Keep`.

Message size, runtime handle/profile, room/turn identity, bearer, model identity,
and asset reference checks legitimately repeat at independent untrusted producer,
wire, durable decoder, process, and commit boundaries. Share semantic constants or
pure predicates, not the authority decision or boundary-specific failure state.

### K-03 — current bounded periodic/retry owners with evidence

Disposition: `Keep and continue review`.

- `runtime_reconciliation.rs` scans only unresolved lifecycle/provider-turn pages,
  with page/concurrency/timeout bounds. It owns recovery when a task dies after a
  durable external-effect checkpoint.
- `event_publication.rs` is wake-driven and arms bounded exponential retry only
  after publication failure; it is not ambient 250-ms database polling.
- `roomSocketClient.ts:58-60,300-351` sends one connection keepalive after three
  minutes of quiet because the server has a finite idle connection contract. It
  has one outstanding nonce and explicit failure/reconnect behavior.
- configured stable-entry/tunnel startup waits and OAuth handoff waits are bounded
  user-operation waits, not ambient polling.

Every later review must still check period, cap, cancellation/cleanup owner, cost,
and failure visibility. Custom-channel/voice timers in F-05 do not qualify.

### K-04 — provider process custody has observed failures

Disposition: `Keep provider process custody; review every other layer separately`.

Guardian, boot identity, lease marker/token, OpenCode exact child/peer ownership,
and bounded staging cleanup address observed Codex companion escape, retained
processes after Stop, a forced-death 64-MiB orphan, and more than 10 GiB of staged
orphan data. Keep this core and measure its costs as D-06 requires. This evidence
does not approve host challenge, frame crypto, remote purpose-ticket exchange, or
fixed-host DNS policy; those have separate dispositions above.

### K-05 — large files with one strong state owner

Disposition: `Keep now; stop at parity and reassess only in the resumed cleanup`.

`guardian.rs` (947 LOC at audit) owns one process-custody state machine, and
`provider_turn_effect.rs` (867 LOC) owns one durable external-effect transition
flow. Splitting either now would likely increase state transfer and public glue, so
their size warnings do not justify mechanical decomposition. `useCanonicalRoom.ts`
(819 LOC) mixes active room projection with unsupported actions and duplicated wire
types; remove those Phase 0 defects first, then reassess its remaining owners.

The structure gate reports 125 existing 500+ LOC files. Per the current product
sequence, that baseline cohort does not create 125 implementation detours: complete
retained parity, stop, and obtain user direction before the systematic size-warning
cleanup. This does not permit new mixed ownership. During every slice, split distinct
domain, authority, lifecycle, invariant, or change-reason owners immediately at any
size; keep one large cohesive state flow only when a split would add exposed state,
interfaces, dependencies, or glue and make its invariant less clear.

### K-06 — invite use-count upper bound has one policy owner

Disposition: `Keep application-owned invariant; do not duplicate without threat`.

`room_invites.use_count` has one application policy owner with an atomic
precheck/CAS. Adding a SQL upper-bound `CHECK` would create a second policy owner
without a demonstrated bypass; malformed rows already fail closed. Keep the
current decision unless a direct writer or corruption path that bypasses the
application owner is demonstrated.

### K-07 — catalog/selection validation has an untrusted producer

Disposition: `Keep validation, remove selection fallback`.

CLI discovery output and executable/model identity can be stale or substituted.
Keep bounded parsing and exact selection validation. Commits `5b94ac6` and `1959a08`
remove the preferred-model-to-first-model substitution at the Rust and frontend
projection owners; absence now remains an explicit unselected state.

### K-08 — exact request replay owns a real ACK-loss window

Disposition: `Keep`.

A durable command can commit before its ACK is lost. Reusing the same request ID
with server deduplication prevents duplicate mutation. Keep bounded replay and its
visible uncertainty; measure retry count/backoff rather than replacing it with a
new ID or silent success.

### K-09 — central identity/public ingress proofs cross real authorities

Disposition: `Keep`.

Ed25519 central registration and public-ingress proof bind an externally reachable
directory/proxy to the expected host key and origin. Their independent external
authorities are concrete; they do not depend on the still-open D-02 transport
decision.

### K-10 — startup product-surface equality is a small real compatibility gate

Disposition: `Keep`.

The native host and sidecar can be packaged from mismatched builds. One startup
revision/digest equality check detects that concrete deployment error without
per-frame crypto, polling, or another authority.

### K-11 — equal write-budget numbers do not imply one policy

Disposition: `Keep separate; document the coincidence`.

`principal_mutation_admission.rs:12-14` and `provider_write_budget.rs:9-11`
both currently use 60-second/3,600-operation/8-MiB windows, but their principals, lifetime, debit/replay,
and exhaustion semantics differ. They are reachable independent owners. Do not merge
their mechanisms or constants into a generic limiter without product evidence that
one actor policy intentionally owns both; equality of numbers alone is not evidence.

## Owner and acceptance routing

This table routes findings; it does not add another contract layer.

| IDs | Existing owner | Phase | Minimum observable acceptance |
| --- | --- | --- | --- |
| F-01 | Antigravity driver/module graph | 1 | no production import/build/runtime transcript edge; missing native completion is explicit |
| F-02 | Codex/OpenCode native drivers | 1 | one documented completion/session authority per provider, with focused protocol fixtures |
| F-03 | common adapter plus provider teardown | 1 | abort/cleanup returns a typed result and failed cleanup prevents unsafe reuse |
| F-04 | protocol product surface plus capability projection | 0B | closed through `7b2168f`; both manual reviewers approved C0/H0/M0/L0 |
| F-05 | frontend composition/product-surface gate | 0B | normal startup/room use issues no request or timer for an absent/deferred owner |
| F-06 | Agent Session projection/profile owner | 3 | roster, timeline, search, restart, and editor obey Agent/participant SSoTs |
| F-07 | provider registration/operation descriptor | 0B and 1 | Phase 0B gates false UI operations; Phase 1 closes provider-native operations and exact model selection |
| F-08 | HTTP admission/transport owner | 0B | closed at `c0cb3e2`; 127 classified-public body requests retain one local total slot, excess public receives 503, and release restores public admission |
| F-09 | human admission owner | 0B | closed through reviewed HEAD `0fd931d` at `C0/H0/M0/L0` |
| F-10 | provider credential store | 1 | one explicit credential source with visible revoke/restart behavior |
| F-11 | Rust protocol exporter plus endpoint decoders | 0B | generated semantic types/constants are shared; snapshot/live/history/search/request envelopes, bounds, errors, and strict rejection remain endpoint-local |
| F-12 | DeepSeek complete-turn owner | 1 | measured complete-turn latency/cost and one cancellable wall-clock/cost budget |
| F-13 | provider factory/custody launcher | 1 | bounded typed guardian/helper cause reaches provider-unavailable/start failure |
| F-14 | browser request-identity owner | 0B | secure UUID absence fails before send; replay still uses one exact ID |
| F-15 | frontend stylesheet entry | 0B | behavior side-effect import is gone and packaged layout/cascade is unchanged |
| F-16 | room socket/catalog subscription owner | 0B | closed watch terminates/disables once and a focused regression proves no spin |
| F-17 | OpenCode SSE decoder | 1 | malformed `data:` fails immediately; valid comments and split chunks still pass |
| F-18 | invite controller and admission surfaces | 0B, 7 | old host-token path is absent; human flow remains exact; bridge controls stay unavailable until Phase 7 |
| F-19 | domain settings/profile contract exporter | 0B | active UI hints match the decided product limit or document an intentional narrower UX bound; server remains authoritative |
| F-20 | message-search exposure plus custom-channel search owner | 0B and custom-channel slice | lobby-only search is labelled as lobby/current scope until a real room-wide union exists; no fallback merge |
| C-01 | persistence participant codec | 0B prerequisite to next participant mutation | exact load/save primitive checks cardinality; authorization/transitions stay local |
| C-02 | protocol exporter/snapshot decoder | 0B | capabilities have one generated owner and no parallel permissions digest |
| C-03 | provider-turn envelope contract | 1 | semantic constants/predicates have one owner while every trust boundary still validates |
| C-04 | domain profile-reference predicate | 3 | persistence reuses format predicate but retains existence/lifecycle checks |
| C-05 | RoomPortal tool catalog | 1 | one canonical descriptor/name set; DeepSeek selects a supported subset only |
| C-06 | Codex config owner | 1 | RoomPortal exposes endpoint/capability data; no Codex CLI syntax remains there |
| C-07 | master/architecture/spec/exposure/verification owners | 0A | each document owns one concern and current statuses/baselines agree |
| C-08 | provider registration/operation descriptor | 0B | no provider-name UI policy can advertise an absent server operation |
| C-09 | room-settings owner | 0B | prove a current non-deferred consumer or remove `activity_plugin` cleanly |
| C-10 | frontend production graph | 0B | dead/deferred backup code cannot mount, import behavior, poll, or imply a route |
| C-11 | domain vocabulary plus existing lifecycle/turn/reconciliation owners | 1 | finite serialized states have one owner while each transition authority, effect, and error remains local |
| C-12 | entity-specific persistence row owners | Phase 1 and prerequisite to next affected mutation | exact row primitives remove codec/SQL drift without generic repositories or moved authorization |
| C-13 | existing browser/session/provider protocol owners | 0B and 1 | identical wire constants have one producer; every trust boundary still validates independently |
| C-14 | existing Codex bundle owner | 1 | pure bundle identity is shared while every TOCTOU revalidation boundary remains |
| D-01 | local startup/control and core HTTP routes | 0B | no host challenge/secret path; desktop and human socket tickets still complete |
| D-02 | room socket protocol/client and ingress topology | 0B | remote proof is absent; local receipt and frame proof remain only if separate controlled evidence defeats the smaller child-identity plus one-use-ticket design; otherwise grants contain no proof key and frames use bounded JSON; finite subscribe, sequence, replay, and failure contracts hold |
| D-03 | human-session target authorization | 0B | one bounded-header session authorization per HTTP operation, no bearer disclosure; desktop/socket tickets unchanged |
| D-04 | DeepSeek fixed-host HTTP client | 1 | ordinary TLS client passes fixed-host tests; Custom API SSRF policy remains separate |
| D-05 | provider runtime handle codec | 1 | discarded suffix absent; boot/token/owner and stale-CAS regressions pass |
| D-06 | reconciliation and guardian staging | 0B | idle/recovery/start-stop measurements justify any cadence/bound change |
| D-07 | human-invite token claims | 0B | each retained claim has a current consumer or is removed |
| K-01 | four asset lifecycle owners plus physical ceiling | keep | replacement/reference/expiry tests retain exact deletion and occupancy behavior |
| K-02 | each independent trust boundary | keep | share values only; boundary-specific fail-closed errors remain |
| K-03 | reconciliation/publication/socket operation owners | keep/measure | cadence, cap, cancellation, cost, and visible exhaustion remain documented |
| K-04 | provider process custody | keep/measure | observed escape/orphan cases remain closed; scan/copy costs are recorded |
| K-05 | guardian/effect state owners and size-warning cohort | post-parity hold | stop first; after explicit resume compare state/interface/dependency/glue count before any structural split |
| K-06 | invite persistence mutation owner | keep | malformed counter fails closed; no second upper-bound policy without bypass evidence |
| K-07 | provider catalog/selection | keep | untrusted discovery is bounded; missing preferred model never substitutes another |
| K-08 | room command replay owner | keep/measure | ACK loss resolves one exact ID without duplicate mutation or infinite retry |
| K-09 | central identity/public ingress | keep | wrong host key/origin fails before registration or proxied authority |
| K-10 | native/sidecar startup surface | keep | mismatched builds fail once at startup without frame-level proof |
| K-11 | human mutation and provider RoomPortal budget owners | keep | distinct debit/lifetime/exhaustion contracts remain separate unless one product policy is proven |

## Resource observations

At audit time, regenerable repository-local artifacts occupied approximately:

| Path | Observed size |
| --- | ---: |
| `target/` | 57 GiB |
| `desktop/src-tauri/target/` | 4.5 GiB |
| `frontend/node_modules/` | 177 MiB |
| `desktop/node_modules/` | 14 MiB |
| `frontend/dist/` | 1.3 MiB |

No files were deleted during the read-only audit. Before the next implementation
run, resolve which build/verification artifacts are still active and remove only
stale regenerable outputs owned by this project. Dependency trees required for
active work are not treated as arbitrary trash.

### Build-artifact lifecycle correction

The implementation run remeasured the Cargo owners before changing them. The root
`target/` occupied 65 GiB physically and Cargo's dry-run inventory reported 453,268
files and 117.5 GiB of logical bytes. `target/debug/deps` occupied 55 GiB,
`target/debug/incremental` 7.6 GiB, and 415,713 files were Cargo's macOS default
unpacked `.rcgu.o` debug units. The excluded desktop workspace separately owned a
4.6 GiB target with 35,777 more `.rcgu.o` files. No Cargo, rustc, or
AgentsAssemble process was active, and only these regenerable project build owners
were cleaned; source, dependency trees, application data, and user changes were not
touched. Available disk rose from 21 GiB to 95 GiB. A target-qualified Cargo clean
also removed shared host intermediates, contrary to the narrower expected effect,
so the final cleanup design never assumes that target-qualified cleanup is selective.

Commit `8cda5f6` first disabled macOS `split-debuginfo=unpacked`. A controlled cold
domain-test build fell from 290 MiB and 568 `.rcgu.o` files in 21.44 seconds to
259 MiB, zero `.rcgu.o` files, and 20.78 seconds. Source-level inspection later
proved that this initial `off` setting also removed source DWARF, so it did not
preserve the debugging contract. Commit `ee76d37` corrects the owner to macOS
`packed`: the same controlled build occupies 285 MiB, creates no `.rcgu.o` files,
and emits a dSYM whose compile unit names the Rust source. The accepted 26 MiB cost
over `off` retains debugger information while still avoiding the default unpacked
object explosion. Other targets, including Windows MSVC, are unchanged.

Commit `b62c4cc` gives the desktop build the existing repository target instead of
a second Cargo owner. Desktop warning-denied clippy and all 25 desktop tests pass
from that shared target, and `desktop/src-tauri/target` remains absent. The initial
cleanup owner in `9e13cf3` ran automatically before verification. Critical ChatGPT
Pro found that Cargo clean does not own a target-wide exclusion lock and therefore
could delete files used by another Cargo or Tauri operation. Commit `36494b6`
separates non-destructive routine checking from destructive maintenance: `make
verify` checks before and after compilation and fails closed with an explicit
instruction, while only operator-invoked `make artifact-prune` runs Cargo clean.
Commit `0b9d5db` makes that explicit maintenance clean the active shared target even
below its ceiling, avoiding a mixed-debug cache without adding a classifier.

Daybreaker found one Low measurement defect in the first review: the physical-byte
counter counted each hard-link directory entry rather than each allocated inode. On
the then-retained cache, that reported 12.013 GiB instead of 11.800 GiB by
double-counting 218.090 MiB across 483 hard-link entries. Commit `52d9383`
deduplicates regular files by `(device, inode)` and pins that boundary. Pro also
found that `st_blocks` is not a portable Python stat field. Commit `36494b6` uses
allocated blocks where the platform exposes them and otherwise uses logical file
bytes; a zero or unavailable file identity never deduplicates distinct entries.
This keeps one measurement owner without inventing platform-specific disk APIs.

The packed full-workspace cache exceeded the original 20 GiB ceiling at 20.56 GiB,
which would have forced a cold build instead of retaining the next quick build.
Commit `5d812e1` sets the observed ceiling to 24 GiB and checks it again after full
verification. The final warm cache occupies 20.667786 GiB physically and
20.134152 GiB logically across 20,006 unique regular files; 483 additional hard-link
entries are counted once. It contains zero `.rcgu.o` files and 33 dSYM bundles, and
an inspected provider dSYM names `crates/agentsassemble-provider/src/lib.rs` as a
compile unit. A packed cold build/test run passed the product suites in 822.72
seconds and exposed the too-small original ceiling. The final warm `make verify`
then passed architecture/source gates, 102 frontend files/639 tests, 25 desktop
tests, every workspace and real TCP boundary test, warning-denied clippy, both
artifact checks, and diff check in 315.16 seconds.

The owner rejects symlinked or non-directory targets, accepts no arbitrary path,
and adds no timer, background worker, age heuristic, retry, fallback, or swallowed
failure. Focused tests cover retention and over-limit detection, explicit active
maintenance, obsolete-owner detection, hard-link identity, platforms without block
counts, unavailable file identities, non-destructive failure, and symlink rejection.
The trade-off is explicit maintenance after a deterministic check rather than unsafe
automatic deletion, and a bounded 24 GiB warm cache rather than either repeated cold
verification or unbounded disk growth.

Critical ChatGPT Pro and Daybreaker Blue High independently found the same final two
defects in `42f0af5`: the hard-link regression still read Unix-only `st_blocks`
directly, and each successful artifact check repeated the complete target walk only
to print its size. Pro classified them Medium/Low and Daybreaker Low/Low. Commit
`537c1b9` routes the test expectation through the portable allocation owner and
removes the second traversal. Two consecutive walks of the retained cache measured
0.426 and 0.524 seconds, so the removed pre/post duplicates were concrete filesystem
work. Both reviewers approved `537c1b9`, exact correction `42f0af5..537c1b9`,
complete correction `9d02acf..537c1b9`, full batch `9a4b5f6..537c1b9`, cumulative
`8903445..537c1b9`, and HEAD `537c1b9` at `C0/H0/M0/L0` with no actionable finding.

Later complete verification exposed a remaining growth owner rather than a capacity
shortage: `target/debug/incremental` alone occupied 8.10 GiB and the retained target
again crossed the 24 GiB maintenance ceiling at 26,162,495,488 bytes. A trial profile
change without first removing the old profile produced a 29,214,638,080-byte mixed
cache, confirming that stale and current build profiles must not coexist. The build
owner now disables Cargo incremental output while retaining dependency and binary
artifacts; no timer, background cleanup, classifier, fallback, or automatic deletion
was added. After one explicit full maintenance clean, complete verification passes in
432.17 seconds with 1,846,460,416-byte maximum RSS. The retained target occupies
14,836,060 allocated KiB, contains no incremental data and no `.rcgu.o` files, and an
immediate workspace all-target `cargo check` completes in 0.26 seconds. The accepted
trade-off is rebuilding a changed local crate without Cargo's intra-crate incremental
objects while preserving the fast unchanged-workspace cache and the existing packed
source-debugging contract. Subsequent focused provider rebuilding raised the retained
cache to 14.63 GiB. The active ceiling is therefore 18 GiB: about 3.4 GiB above the
largest current-profile observation, replacing the obsolete 20.6 GiB rationale while
retaining deterministic source/profile variance.

## Current verdict and exit

Verdict: `APPROVE` for the Phase 0A inventory, dispositions, ownership map, and
implementation order at reviewed content checkpoint `9711232`. This verdict does
not approve or complete the routed product findings.

The following exit criteria are satisfied:

1. every finding has one confirmed owner and `Fix`, `Consolidate`, `Keep`, or
   `Deferred/Unknown` disposition;
2. `docs/PRODUCT_REIMPLEMENTATION_PLAN.md`, architecture, exposure inventory,
   active slice, and workboard describe the same current state;
3. the Phase 0 correction order has observable acceptance and verification paths;
4. the full previously unreviewed pushed range from the last reviewed baseline
   through the new head receives critical-web and Daybreaker manual review of every
   individual commit and the cumulative diff, including structure, duplicated
   policy, overimplementation/over-defense, ownership, lifecycle, polling/timers,
   fallback, and swallowed failure. Every defended layer must name its reachable
   use case, observed failure, or in-scope threat and the smaller alternative it
   rejected.
