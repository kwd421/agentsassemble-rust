# Repository-wide reimplementation audit — 2026-09-01

Status: manual source, history, duplication, and defensive-complexity audit
complete; external review pending; implementation verdict `REVISE`. This record
owns findings and dispositions, not product contracts or the implementation
sequence.

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

### F-04 — signed capabilities advertise actions that do not exist

Disposition: `Fix`; high product/medium structure impact.

`agentsassemble-domain/src/model.rs:113-128` computes `room.delete`,
`participant.kick`, `provider.request.resolve`, and bridge permissions.
`server_proof.rs:42-63` signs them. `agentsassemble-protocol/src/lib.rs:92-175`
contains no room delete, participant kick, provider-request resolution, or agent
re-add action. The active frontend nevertheless calls them in
`useAppController.tsx:595-600`, `useCanonicalRoom.ts:592-615,732-745`, and
`AppOverlays.tsx:164-168`.

The client currently rejects actions absent from the signed product surface, so no
permission bypass was found. The UI and capability projection still make false
product promises. Until each complete action owner exists, remove the capability
and hide/gate the control. Later derive advertised capability from both principal
permission and executable product surface.

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

### F-07 — provider catalog and controls overstate real operations

Disposition: `Fix`; medium product/structure impact.

- `catalog.rs:402-407` substitutes the first discovered model when the preferred
  model is missing. The OpenCode preference at `catalog.rs:191` is still
  `opencode/hy3-free`, contrary to the selected Muse Spark flow.
- `registration.rs:341-364` advertises login for every provider, while the Rust
  server implements only DeepSeek credential operations.
- `AgentCreateModal.tsx` renders OpenCode credential UI, but
  `api/providerCredentials.ts` supports only DeepSeek.
- opening the Agent modal calls an absent catalog-refresh route and silently
  ignores the failure.
- `useAgentPresentation.ts:63-79` silently absorbs missing provider-usage results.

Require an exact verified preferred model or explicit unavailable/unselected state.
Advertise login, credential, usage, refresh, and model operations only from their
real provider operation owner. Do not add dummy routes.

### F-08 — one global HTTP connection budget may let public traffic starve local control

Disposition: `Measure, then Fix or Keep`; medium availability unknown.

`web.rs:327,345` takes one global 128-connection semaphore before ingress trust is
classified. `http_transport.rs:24-70` bounds headers, buffers, and absolute
connection lifetime, and public proxy requests reach the same loopback listener.
No focused TCP test currently proves that public saturation preserves local
operator/control capacity.

Run a controlled real TCP/proxy boundary test. If contention is reproducible, use
the smallest owner-level public budget or reserved local capacity; do not add a
speculative listener framework. This is not yet a proven remote exploit.

### F-09 — human invite guide and accepted aliases need current-client proof

Disposition: `Fix documentation; decide finite client set`; low.

Reusable invite state becomes terminal for the same admission key after expiry,
while the guide claims the same link can simply be reused. Correct the guide to the
actual state transition or correct the owner if current reachable behavior differs.
`human_admission.rs:309-314` also accepts every client kind outside a denied
non-human list. Original aliases are not enough to justify compatibility. Prove
the current clients, then accept a finite canonical set or record the necessary
aliases explicitly.

### F-10 — DeepSeek keyring-to-environment priority is an unapproved fallback shape

Disposition: `Decide and model explicitly`; low security/product impact.

`credentials.rs:207-263` captures `DEEPSEEK_API_KEY` and uses it when the keyring is
missing; delete at `credentials.rs:285-295` removes only the keyring entry. The UI
does expose the source and does not offer delete for environment credentials, and
a keyring error does not fall through. That limits the current security impact but
still leaves two credential owners with priority semantics.

Either remove the environment source or model it as an explicitly selected,
immutable launch source with clear revoke/restart semantics. Do not retain it as an
implicit convenience fallback.

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

Provider discovery and creation already have a Rust registration, while login,
credential, usage, refresh, model labels, and availability are also inferred from
provider-name conditionals in the frontend. This produced the false controls in
F-07. Extend the existing descriptor only with operations that an implemented
server owner can execute; generate/project that bounded surface to the client.
Provider-native authentication and model rules stay in their modules. Do not build
a speculative plugin framework.

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

The unmounted Admin and FrontendUpdateNotice components, deferred Mafia/RimWorld
paths, old broad `api.ts` calls, and unimported
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

Disposition: `Remove`; Phase 0B.

`host_ticket.rs` and the private `/api/host-challenge` plus `/api/ws-ticket`
routes add a 30-second challenge map/mutex, HMAC exchange, and startup secret.
Desktop production obtains its socket ticket over the owned private control pipe;
admitted humans use the authenticated session exchange. Repository-wide search
found no production frontend caller for the challenge routes. Remove the routes,
cache, HMAC, and secret preamble while retaining control-pipe EOF ownership and
both real ticket issuers.

### D-02 — local receipt, remote proof, and per-frame proof have different threats

Disposition: `Keep/Simplify local receipt; Remove remote proof; Measure local
per-frame proof`; Phase 0B.

`authenticated_channel.rs`, `server_proof.rs`, and `authenticatedFrames.ts` HMAC,
counter, base64-encode, copy, and asynchronously serialize every frame, adding
roughly one-third wire expansion. The native proof key crosses private control and
bundled-only Tauri IPC while its frames cross `ws://127.0.0.1`; a fresh-challenge
receipt therefore detects the concrete post-grant sidecar-death/port-substitution
case. Keep that local receipt but bind only the expected room, participant, protocol,
and finite `C/H`. Remote key delivery and frames cross the same HTTPS/WSS ingress, so
remote HMAC is not independent authority and is removed. The architecture's
same-account exclusion applies only to provider executable binding and cannot decide
room transport. Establish and scope an active local relay through controlled
reproduction or equivalent concrete topology evidence before retaining local
per-frame authentication. If proven, authenticate the Snapshot and every later
bidirectional product frame as raw bounded UTF-8 bytes with direction and counter
under the cached key; otherwise use ordinary JSON after the
receipt. In both cases remove base64, repeated key derivation, snapshot proof, and
permissions digest, and keep
one-use tickets, TLS/origin/ingress checks, strict schemas/limits, the finite `C/H`
handshake, sequence, request-ID replay, uncertain-ACK recovery, and one-time
native/sidecar product-surface equality.

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
Keep bounded parsing and exact selection validation. The unrelated preferred-model
to first-model substitution remains F-07 and must be removed.

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
| F-04 | protocol product surface plus capability projection | 0B | no advertised control/capability lacks an executable action |
| F-05 | frontend composition/product-surface gate | 0B | normal startup/room use issues no request or timer for an absent/deferred owner |
| F-06 | Agent Session projection/profile owner | 3 | roster, timeline, search, restart, and editor obey Agent/participant SSoTs |
| F-07 | provider registration/operation descriptor | 0B and 1 | Phase 0B gates false UI operations; Phase 1 closes provider-native operations and exact model selection |
| F-08 | HTTP admission/transport owner | 0B | controlled TCP saturation records whether local control progresses; design changes only on reproduced contention |
| F-09 | human admission owner | 0B | guide matches expiry/reuse state and only proven current client kinds are accepted |
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
| D-02 | room socket protocol/client and ingress topology | 0B | a fake replacement listener cannot forge the local receipt or cause Snapshot/readiness/command effects; remote grant/frames contain no proof; local post-receipt frame proof remains only on controlled reproduction or equivalent concrete in-scope topology evidence; finite subscribe, sequence, replay, and failure contracts hold |
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

## Current verdict and exit

Verdict: `REVISE`. Passing gates and prior slice approvals do not close the
repository-wide product and ownership findings above.

This audit exits only when:

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
