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

## Acceptance and verification

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
restart/resume, and runtime/persona configuration require that custody. External
ownership is explained without advertising a later managed start. Supported
external pause, resident resume, stop/cleanup and activity display remain available.
No server permission, session identity, process ownership or provider execution
changes. Acceptance includes first external admission, stopped/error external
sessions, managed controls and external cleanup through the existing shared UI.

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
