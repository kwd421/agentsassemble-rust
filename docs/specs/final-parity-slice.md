# Final parity and integration

Status: Phase 9 begins after Daybreak approved Phase 8's correction, cumulative
range and exact `f776e2e` at C0/H0/M0/L0. Final acceptance is still pending.

## Definition and boundary

Close the retained product against original `d5046473010d1353a81ee38337360e6d98f7bd6f`
and the completed Phase 1–8 contracts. The exposure inventory remains
`docs/FRONTEND_BACKEND_GAPS.md`; measured runs and reviewer dispositions remain
`docs/VERIFICATION.md`. Prior exact reviewed evidence is reused, with current
integration coverage rather than repeating every isolated case.

The existing Rust owners retain authority over identity, room participation,
messages, Agent Sessions, provider custody, the three external-agent admission
paths, operational state and assets. The frontend must not invent a room or select
a deferred game through copied startup parameters. Voice, Mafia and RimWorld remain
unimplemented and inactive; their deferred source is preserved. This phase adds no
legacy fallback, compatibility shim, new provider or architecture-gate exception.

First confirmed gap: `createStartupRoute` still invokes `roomFromMafiaParams`,
fabricating a game-labelled room and selecting `live` for a Mafia URL. Disconnect
that startup branch while preserving canonical direct-room and admission routes.
Inspect the emitted production module graph and code for deferred mount/request/
poll/heartbeat paths, then exercise the packaged entry points.

The emitted API audit also finds a copied `/api/local/workspace-picker` branch.
Original `providers.py` explicitly limits that operation to the local operator.
The Rust owner is Tauri's `choose_local_workspace`; remove the absent HTTP branch
and preserve the native bridge's explicit unavailable error for browser callers.
Do not expose host filesystem selection through an admitted browser credential.

The baseline full Rust verification passes 867 tests but retains 31,892,033,536
artifact bytes, exceeding the unchanged 18 GiB gate. Fifty-four test dSYM bundles
alone occupy 20.37 GiB. Use Cargo's `line-tables-only` debug information in the root
test profile, preserving file/line backtraces and the existing test assertions,
overflow checks, optimization settings and SHA-256 override. The tradeoff is no
default type/variable inspection in test binaries; full debug remains an explicit
Cargo profile override. Ordinary development and release profiles stay unchanged.
After all baseline work stops, use the existing artifact owner to retire the
replaced artifacts, then verify the same full suite and measure the resulting size.

Actual Codex startup exposes an upstream transport mismatch: the installed native
Code Mode Host accepts a `grpc://127.0.0.1:0` listener and publishes a canonical
`http://127.0.0.1:<port>` gRPC endpoint. Its former `ws://` invocation exits before
readiness. Update the existing host launcher and strict readiness parser to that
observed contract, preserving the same executable bundle, loopback-only binding,
process group, descriptor isolation and cleanup authority. Do not add transport
fallback or infer successful cleanup from a closed pipe. Verify the real CLI
readiness, existing guardian/transport cases and a complete packaged Codex turn;
distinguish failed-start cleanup evidence from the existing macOS uncertainty
contract. A controlled companion exit reproduces launcher fork/exit before cleanup;
the guardian deliberately cannot publish absence in that state. Preserve its
uncertain generation and blocked retry instead of inferring absence from dead PIDs.

Actual Cursor `auto` startup exposes another executable boundary: the installed
shell launcher resolves its sibling Node, JavaScript chunks and native modules.
Single-file staging makes those dependencies disappear. Bind the installed Cursor
package at its executable owner, include its dependencies in the durable identity,
and stage the verified package with the launcher's relative paths intact. Preserve
bounded filesystem work, immutable launch bytes, exact process custody and the ACP
driver's permission/model contract. Do not execute mutable original package paths,
add a wrapper/environment fallback or weaken the existing cleanup proof. Verify
dependency mutation/missing-file rejection, the installed launcher and complete
packaged `auto` response/stop. The failed original generation remains uncertain.
The first corrected package is rejected by persistence's still-single-file commit
check. Share the Cursor package inspection/identity contract at the domain authority
boundary, used by both bounded filesystem owners. Keep private staging and process
lifetime with the provider; preserve persistence's pre-commit revalidation and
transaction ordering. Do not duplicate the manifest policy or bypass the check.

The staged native Cursor now initializes successfully, but its CLI model slugs
do not match ACP model authority (`auto` versus `default[]`, with similarly
different reasoning/speed representations). Use Cursor's parameterized ACP picker
and `cursor/list_available_models` response for discovery and attachment. Native
model parameters own available choices and defaults; expose context/thinking
variants in the model selector and thought-level/fast parameters through existing
controls. Re-read that authority at attachment and require the complete returned
configuration to confirm every requested value before binding the session. Unknown,
missing, ambiguous or changed options fail closed; do not infer aliases from CLI
text or add a protocol fallback. Shared ACP owns typed extension exchanges and
configuration receipts, while Cursor owns parameter meaning. Catalog inspection
creates no native conversation and retains bounded process cleanup. Verify native
metadata, changed/contradictory receipts, existing Grok ACP behavior, and a packaged
Cursor Auto turn and stop; other real model turns remain outside authorization.

## Cursor qualified MCP permission contract (2026-09-14)

The official2026.09.10-fd3934a package now refreshes structured tool input before
permission. An isolated real Auto diagnostic confirms an empty initial tool call,
a same-ID update with providerIdentifier=agentsassemble_room and
toolName=read_discussion, then a same-ID permission request without rawInput.
Current Rust reads only tool_name and requires identity repeated in the permission
request, so this actual supported Cursor exchange cannot authorize the room tool.

Cursor selects its explicit qualified-MCP notification contract at configuration;
Grok retains its existing request-tool-name contract. Existing session/active-turn/
room-observation authority and call-ID lifetime remain the owner. Only a previously
qualified exact room tool on that same call ID can authorize AllowOnce. Missing,
wrong-server, unknown or conflicting identity must reject; titles never authorize.
No new MCP operation, permanent approval, global CLI configuration or fallback is
introduced. Old Cursor versions that never emit complete identity still fail closed.
Verify the captured sequence, conflicting/cross-server/unknown identities and
session/turn clearing through existing ACP tests, then the new official package's
actual Auto room response and stop in the signed app. Existing installations remain
unchanged during the isolated diagnostic. Final source requires whole review.

## Public persona metadata acceptance (2026-09-14)

Whole-source R1 reproduces through actual HTTP import: a300,000-byte name returns
200 and is persisted. Required public session metadata cannot be trimmed during
snapshot fitting. The library owner must reject a normalized card whose complete
JSON-encoded public summary (including possible thumbnail URL) exceeds1KiB before
replacement, and revalidate stored selections before create/configure commits.
This bounds the persona contribution at64KiB for64 sessions, including JSON escapes,
without truncating private descriptions/lore or changing the WebSocket frame limit.
Invalid replacement leaves the previous card intact; invalid stored selection
must not partially create/configure a session. Verify actual HTTP rejection,
selection/create/configuration/reopen and full-capacity socket delivery. No existing
user data is deleted or silently repaired.

## Vote and channel exact retry (2026-09-14)

Whole-source R2: the socket owns exact request replay, but vote and custom-channel
forms discard its handle. Keep uncertain intent with the existing form/draft owner.
Same form payload retries the original receipt; changed content/attachments or
closing the vote form starts a new intent. Channel retries still use the transcript
owner's busy and connection checks; room replacement remains rejected by the socket.
Do not change layout or infer success from matching text. Verify receipt-loss retry,
intermediate offline failure, intentional edits and room identity changes, then
actual packaged vote/channel retry and one durable publication.

## Acceptance and verification

Reconnect recovery (2026-09-14): an ahead-of-history subscription can receive the
server's `resync_required` before its receipt. Recognize this exact room-events
frame and request an initial snapshot using the existing reconnect owner. Keep the
last verified cursor, accepted room UID and pending commands until the ordinary
receipt/snapshot checks resolve the room lifetime. A changed room UID rejects old
pending intent before new-room readiness; a resync hint alone cannot authorize
regressed same-room history or replay pending commands. Malformed responses retain
the existing failure path. No layout, authority, timer or alternate transport is
added. The transport regression must cover both lower and higher recreated-room
cursors and must not claim packaged recreation from synthetic frames alone.

The real 390px admitted-browser flow exposes an inherited presentation gap:
`MobileRoomInfoPanel` reports no items in its media/pins/links/files tabs even when
the room contains an attachment. Original `d504647` has the same unconditional
empty branch; these collection views never had a retained data consumer. State
that these tabs are unsupported instead of asserting an empty channel, and record
the limit in the exposure inventory. Preserve working timeline downloads, header
pins and search. This is a presentation correction, with no new collection API,
client-derived collection, request, timer or authorization change. Verify the
existing component cases, frontend build and packaged narrow-screen wording.

An actual admitted-browser ballot is committed once but leaves the poll controls
waiting: the common event decoder rejects the empty actor in the domain's
`privacy_minimized_vote_transition`. Accept that existing anonymous cast/withdraw/
close marker in snapshots, live events, history and command receipts. Require its
vote reference, empty content and empty actor; ordinary messages retain their
nonempty actor requirement. Keep ballot identity/choice with the server's private
vote owner, without changing stored events, command replay, timeouts or privacy.
Verify the actual wire shape through the existing socket harness, then recover the
packaged room containing the committed ballot and exercise voting and closure.

- Reconcile every retained exposed feature with its original entry, Rust owner,
  state/failure semantics and frontend or explicit service-only consumer. Report
  remaining partial, indirect and intentionally unavailable operations accurately.
- Preserve local and admitted-user desktop/mobile flows, room lifecycle,
  restart/reconnect, headers/right panels/search, persona import and explicit
  selection, ordinary ordered/ambient conversation and permitted tools.
- At final integration, run only configured DeepSeek, Codex, OpenCode, external
  Antigravity through Room Connector, Grok and Cursor `auto`. Verify the separate
  external and managed AgentBridge paths using the allowed provider matrix. All
  other providers retain source/contract evidence without actual execution.
- Collect CPU, memory, disk, process/task count and latency at their owners;
  distinguish measurements from unavailable instrumentation. Preserve approximately
  20 percent CPU headroom and use sequential bounded heavy builds. Optimize only
  a measured cost or concrete failure, recording before/after evidence.
- Run affected checks and unchanged mandatory gates. Directly operate the packaged
  app at desktop and 390px/low-height sizes; local tests do not replace that proof.
- Quit only each verification app and its owned children, reset Computer Use, and
  remove exact isolated data/artifacts after use. Preserve user data and unrelated
  processes; use the existing artifact owner after active builds have ended.
- Obtain final GPT-6 Pro and Daybreak Blue xhigh approval for the complete retained
  product, original-to-Rust inventory, unreviewed individual commits, cumulative
  implementation and exact pushed HEAD. The master plan's per-slice gate owns the
  user's latest requirement to continue through Pro review, supported corrections,
  verification and final re-review without intermediate pauses.

Unavailable external dependencies or authorization gaps remain explicit limits;
they do not become successful or simulated real-provider evidence. At the accepted
parity exit, stop and report before any repository-wide 500+ LOC cleanup.

## Visible latest-message read cursor correction (2026-09-09)

The user observed an unread banner while viewing the latest message. LobbyView
computes unread state from the persisted sequence cursor, but only its manual
button advanced it. Connect the existing loaded/latest viewport to the existing
preference-write owner. Read only when the focused visible document displays the
latest feed, with no modal or fixed historical/search window. Focus/visibility and
actual scroll events trigger evaluation. Use the history owner's same 64px latest
predicate. A visible feed blocked by a native or portal modal temporarily observes
that modal's DOM lifetime, disconnects at dismissal, and rechecks every guard;
closing a modal does not require a second scroll or app switch. No timer/polling or optimistic substitute
for the preference owner is added. Deduplicate attempts by room and sequence and
leave failed persistence with the existing preference error/recovery owner.
Acceptance: the current latest view clears unread through persisted preference
state; scrolling older, hiding/defocusing, and search context do not acknowledge
new messages. Direct packaged verification includes re-entry/restart persistence.

The sidebar's lobby “mark read” action also wrote a timestamp while the unread
projection consumes sequence cursors; use the canonical lobby sequence at that
existing action owner. Confirmed cursor advancement releases attempt deduplication,
so a later user cursor change does not suppress a new visible read.

## External session action ownership correction (2026-09-10)

Pro Group 2 found that an external attendee with no prior turn could see the
managed start action and stopped external attendees could edit runtime/persona
settings, although the server correctly rejects both through `require_server_custody`.
The shared session details consumer derives server custody from the same canonical
fields: `!external_owned && process_ownership == "server"`. Managed start, stopped
restart/resume, resident pause/resume, and runtime/persona configuration require
that custody. Daybreak's correction review confirmed that `require_resident_state`
also rejects external custody; a UI-only external pause/resume callback is not a
supported contract. External ownership is explained without advertising managed
controls. Supported external turn interrupt, stop/cleanup and activity display
remain available through their existing external owners.
No server permission, session identity, process ownership or provider execution
changes. Acceptance includes first external admission, stopped/error external
sessions, managed controls and external cleanup through the existing shared UI.

Pro follow-up G2-M3: host provider catalog capability is not an external runtime's
capability. Project `external_retained_interrupt` from the existing attendee
connection owner into public session snapshots and state events in their existing
transaction. The projection is optional: an unobserved/historical value does not
grant interrupt support. Durable session records do not own this value and session
mutation must not copy the projection back into storage. Managed controls continue
using the provider catalog; external controls require the reported value to be true.
Reuse the server interrupt owner's capability lookup; do not weaken its gate or add
a capability store, polling, provider-kind inference or runtime-private data.
Verify reported false/true through snapshot and state events, UI action availability,
managed controls and recovery. External stop and supported interrupt remain intact.

## Room-menu read persistence correction (2026-09-10)

Pro Group 2 found that the room menu's explicit mark-read action only changed
directory `createdAt`. Remove that fake write. Once the exact active room's
canonical connection, channel inventory, history and preferences are ready, mark
the lobby and current custom text channels at the accepted room sequence in one
existing serialized preference write. Preserve notification settings and unrelated
channel entries. This explicit user action does not require latest-scroll visibility;
automatic read guards remain unchanged. Disable unavailable/pending actions, retain
the menu on failure with the preference owner's error and explicit reload, and close
only that menu after success. New messages after the captured sequence stay unread.

Preference POSTs must carry the caller's expected `room_uid`. Both local and remote
write owners compare it with the current active room in the same transaction as
the preference update. The current native ticket alone binds only room name and
user/participant, so a ticket issued around same-name room recreation cannot replace
this intent check. A missing UID is an error; no compatibility fallback is added.
The shared writer retains captured room/device/session identity and prevents a
queued operation from dispatching after its UI authority or room scope changes.
No new read store, timestamp fallback, timer, server orchestration or provider run
is introduced. Verify old-scroll explicit marking, batch preservation, rollback and
retry, queued scope changes and stale-incarnation rejection, plus packaged restart
persistence. The extra room comparison is bounded work on an explicit write.

Pro's follow-up G2-M4 extends the same readiness predicate to the shared channel
read handler and both existing small-screen shortcuts. Pending/failed preference
loads must not turn an unknown channel map into an empty full-map replacement.
The handler dispatches nothing until the current canonical UID, connection,
history and preferences are ready; both shortcuts expose that same availability.
Explicit shortcuts capture the accepted room sequence for lobby and custom channels;
they do not manufacture a timestamp or derive an empty cursor from unloaded events.
Keep the existing preference error/reload and serialized writer. Acceptance covers
initial pending/failure without POST and successful load preserving other channel
entries and notification values when the read cursor advances.

## Deleted in-flight input restoration correction (2026-09-14)

Whole-repository R1 identifies a deleted message being restored after an interrupted
turn, poisoning subsequent message scheduling. Preserve active execution custody
until quiescence; deletion must not erase already-performed effects or active source
authority. Every restoration owner must merge the canonical queue under its existing
transaction and omit only inputs whose canonical event has the explicit deletion
tombstone. Missing/corrupt authority remains an error. Apply the same rule to retained
interrupt, runtime loss, failed turn, stop and restart reconciliation. No schema,
queue limit, runtime retry, provider cursor or gate change is required.

Acceptance: delete an input already assigned to a turn, then finalize retained
interrupt or runtime loss; a subsequent message commits and the deleted input is
absent from future queues/assignments. Existing undeleted-input retention and
canonical queue failure tests must continue to pass.

## Departed participants in initial connection metadata (2026-09-14)

Whole-repository R2 is reproduced by 1,000 canonical Connector admissions and
leaves with at most one current external participant, followed by an actual local
WebSocket subscription. Historical membership alone exceeds the unchanged 256 KiB
frame limit. Public snapshots must exclude Left membership records, consistent with
the live participant_left transition that removes them from frontend state. Keep
Joined/Detached and moderation/removal records needed by agent creation, including
Exported. Keep all canonical participant rows and embedded historical author identity;
internal unfiltered snapshots remain available to authority owners. Filter at the
participant read query so historical departures do not become response allocations.
No pagination, frame limit increase, row deletion or UI layout change is required
for this lifetime correction. This does not establish bounds for every other kind
of snapshot metadata or certify expired-but-still-Joined participant handling.

Follow-up: the same canonical 1,000-admission regression now reproduces
snapshot_too_large for both moderator kick and export, while leave passes. Terminal
external membership is historical regardless of which departure transition ended
it. Extend the same public query to omit Left/Kicked/Exported rows unless an actual
room Agent Session references the participant. Retain that bounded session roster
for re-add eligibility and exported-session exclusion; retain every durable row and
embedded message author. The existing session capacity remains unchanged. Verify
all three canonical departure paths over real WebSockets and public snapshots of
removed managed sessions. No new storage, cursor, fallback or frame budget is added.

The completed review also includes expired Joined records. Canonical Connector
admissions at controlled past timestamps reproduce the same rejection with no new
message events requested. The public roster additionally follows the existing human
and Connector session expiry/revocation owners. Any current human session retains
the participant; an expired prior session cannot hide a replacement. Internal reads
and history remain unchanged, and a removed managed-session participant remains
available for creation eligibility. Verify actual expired Connector reconnect and
human expiry/current-session snapshot behavior without sleeps or a new cleanup loop.

Cost follow-up while whole review runs: extracted current SQL with the applicable
existing SQLite indexes takes 18.2ms at 1,000 expired human memberships and 1.6646s
at 10,000 in a controlled in-memory query benchmark. Correlated ownership checks
repeat the human-session scan for each participant. Derive the inactive human IDs
once per room in the same SQL query by grouping canonical session rows; any current
session still retains its participant. No cache, index/schema change or new state
owner. Re-run the actual socket/expiry and managed-session retention checks and the
same cost probe; distinguish query-only timings from real server latency.

## Explicit retry after an uncertain command (2026-09-14)

Completed whole-repository M1 identifies a real caller-boundary loss: bounded exact
replay rejects the Promise and deletes the only request identity, then the composer
resends the restored text with a new ID. Keep automatic replay bounded, while the
returned uncertainty error owns an explicit retry of the exact encoded command,
original room instance and original transport authority. No payload-based global
registry, server heuristic, new receipt store or automatic post-deadline replay.
A closed/replaced room transport cannot send this intent into another incarnation.

The existing per-room composer draft retains that retry operation. Sending an
unchanged uncertain draft retries the original command and is labelled accordingly;
editing text/attachments creates a new message intent. Successful receipt clears
both draft and retry. Further transport failure must not silently turn an uncertain
intent into a new one. Preserve existing composer position, top rail and panels.
Verify original committed command/ACK loss, bounded settlement, user retry with exact
ID/bytes and deduplicated receipt; stale-room refusal, edited new intent and actual
packaged interaction. Current verification and whole-source review remain pending.

## Uncertain command recovery deadline (2026-09-14)

Review progress identifies a sent command left pending when every later ticket or
subscription fails. The current close handler clears the command deadline, while
only a successful retransmission arms it again. Keep the same exact serialized
request and eight-attempt retry owner. After a sent connection closes, the existing
command timer must also bound preparation for its next replay: retain the scheduled
backoff plus the existing 20-second command wait. Failed tickets or incomplete
handshakes cannot reset this waiting deadline. Expiry reports outcome_unknown and
stops replay of that intent; it does not claim rejection, roll back a server commit,
change room identity rules or stop ordinary connection recovery. A later confirmed
replay within the bound still settles normally. Verify failed and hanging ticket
issuance, unfinished handshake, bounded exact replay, and no retransmission after
unknown settlement with controlled timers.

## Shared panel search stacking correction (2026-09-14)

Packaged follow-up: at the current desktop width, the old compact member panel's
z-index 100 covers the persistent rail's search popover. The shared-width panel
layout owner must reset that obsolete overlay stacking, while retaining panel
width, top actions, main composer and dialog layers. Verify actual search opening,
result selection/dismissal and panel switching in the signed package. No search
authority, CSS gate or mobile-layout change is needed.

## Deleted edit-event public projection (2026-09-14)

Whole-repository R3 is reproduced by edit/delete followed by a newly admitted
read-only participant receiving the edited body. Project historical message_updated
events against the canonical target's current deletion state within the same read
transaction. Apply this to initial/resumed snapshots, history pages, subscription
catch-up and queued publication. Preserve event IDs, sequence positions and original
transition type; remove the body and expose the existing message_deleted marker.
The frontend interprets that marker as the existing deleted-message state even if
the deletion transition lies outside the loaded history window. Historical reads
must not resurrect text or replace the deleted placeholder with an empty edit.

This is an authoritative public projection, not a schema migration or a new
compatibility path. It covers already-stored deletions as well as new ones. Exact
private command receipts retain their established replay contract; this does not
claim erasure of a previously delivered copy or physical database purging.

## Authenticated Connector wait transport lifetime (2026-09-14)

Whole-repository R7 connects an unbounded-until-message authenticated wait to the
outer absolute 30-second HTTP connection lifetime. The wait handler already owns
its session expiry, revocation, cancellation, room cursor and connection lease.
Only after authentication, read-budget and room-connection admission may it retain
an authenticated-wait lease on its exact HTTP connection. No anonymous or unrelated
request obtains this lease. At the existing absolute deadline, the transport closes
keep-alive to new requests but drains this admitted wait; its existing expiry,
revocation and shutdown paths terminate it. Once the handler releases the lease,
response flushing is bounded by the existing 30-second transport duration.

Ordinary/unauthenticated connections retain their header, capacity and absolute
lifetime limits. No automatic retry loop, empty successful heartbeat, larger public
limit or swallowed transport failure is introduced. Verify a real Connector wait
across 30 seconds of silence, then one delivered message; also verify that an
unleased handler still loses its connection at the original boundary.

## Builtin API and Local workspace tools (R4, 2026-09-14)

Restore the reachable original builtin `api_work_tools.py` contract: selected
workspace file listing, UTF-8 reading, literal search, and owner-approved file
write/exact replacement. Read-only rooms remain workspace-free. Existing catalog
permission selection, canonical workspace identity and execution-bound provider
requests own admission; no alternate harness or shell command is introduced.

An opened directory capability is checked against the selected workspace identity.
Relative paths exclude repository control directories. Capability resolution admits
only symlink targets within the selected workspace; subsequent descriptor traversal
does not follow substituted symlinks.
Reads/discovery retain bounded file, entry and result budgets. Writes use a private
sibling and atomic rename, so replacing a hard-linked file cannot truncate an
outside inode. Replacement rechecks the read content after approval; this is not
a filesystem transaction against independent concurrent external writers.

The driver retains each blocking filesystem task through turn cancellation and
joined cleanup. Cancellation denies work before publication where still possible;
started write effects remain uncertain for retry purposes. A cleanup timeout must
retain custody and report failure, never imply quiescence. Approval is one operation
on one relative path, through the exact session/turn/execution request owner; no
owner, denial, expiry or delivery failure permits a write. Tool file contents are excluded from approval descriptions and logs; tool results
stay in the private API conversation instead of being automatically added to room
history. An agent can still explicitly discuss a file in its ordinary room reply.

Acceptance: builtin API and Local catalogs expose the existing workspace option;
read/search return actual selected files; approved writes and replacement succeed;
denial/cancellation/path escape/control directories/symlink and hard-link cases
preserve protected data. Controlled API tool calls, affected provider tests,
mandatory gates, and signed packaged selection/approval must pass. Whole-current-
repository Pro review remains required.

## Claude SDK applied effort receipt (2026-09-14)

Real Opus Low verification exposes a bridge contract error: SDK-hosted init frames
do not publish effort, whereas the installed SDK defines applied turn effort on
the native Stop hook. Both installed CLI versions 2.1.231 and 2.1.270 omit init
effort; the current CLI's Stop hook reports low and its exact result returns 68.
Use the SDK Stop callback for applied-effort confirmation on the one active main
turn and exact session, before accepting its correlated successful result. Missing,
mismatched or foreign receipts must reject publication. Retain model, permission,
MCP identity, session and result correlation checks. Do not infer applied effort
from the requested option, echo it into a fabricated provider receipt, or add a
legacy fallback. Verify the missing-init successful native shape and rejected
missing/mismatched Stop receipts, then actual packaged Opus Low room read/reply.

## Stop after confirmed failed-runtime exit (2026-09-15)

Real Claude protocol failure confirms provider process exit and clears exact runtime
custody, but leaves recovery_required. A subsequent user Stop currently demands the
removed handle and never reaches finalization. The existing stop owner must recognize
its terminal Failed execution at the current generation together with cleared runtime
custody, no active provider session/turn and no competing lifecycle intent. Finalize
the explicit Stop through the existing durable lifecycle receipt, keeping queued input
and releasing recovery. Do not invoke a nonexistent external effect or infer exit
from a missing handle alone. Live, uncertain or partially retained custody still
requires the exact runtime stop. Verify both confirmed-gone and retained-runtime
failures, exact stop replay, and the originally affected packaged session.
