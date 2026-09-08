# External AI admission and bridges

Status: Phase 7 contract and dependency skeleton; implementation begins after the
approved Phase 6 checkpoint `25c7961`.

## Definition and observed entry points

The original checkout is verified at `d504647`. Retained owners are
`application/room_connector.py`, `providers/room_connector_mcp.py`,
`application/room_attendee.py`, `application/agent_bridge_entrypoint.py`,
`providers/agent_bridge.py`, `admission/remote_room_client_packet.py`,
`admission/invite_service.py`, the room invite/realtime handlers and their mounted
frontend invite/connection controls. Preserve these product contracts without
copying Python threads, polling, config files or launch-secret transport.

Three custody boundaries must all exist before whole-phase acceptance:

1. Room Connector admits the current external AI conversation as an agent
   participant through its own one-use invite and MCP tools. It reads bounded
   ordinary room context, searches/reads message context, speaks, votes, uses room
   randomness, waits for new public messages, reconnects and leaves. Ordinary
   room permissions apply; it never becomes a human session or managed Agent
   Session. Antigravity exists only here, launched externally with the current
   conversation's MCP registration. The app does not choose its model or stop its
   CLI. The original visible invitation defaults to one use and one hour.
2. External `assemble room attend --provider <provider>` owns its invitation,
   WebSocket, provider credential, workspace and provider session. Saved-AI-friend
   invites and an admitted human's companion packet target this owner. The provider
   is explicit and must match the invite. The client receives a bounded room view
   and exact assigned turn, returns authenticated reports/outcomes, relays supported
   provider requests, reconnects and cleans up its owned runtime on leave/stop.
   An entry packet never starts a provider or transfers host filesystem authority.
3. Server-managed AgentBridge owns its launched process and private startup
   handoff. Existing Agent Session lifecycle/turn/provider owners remain canonical;
   the child adapter cannot mint external attendee or connector authority. Launch,
   readiness, turn/report/request, stop, crash/reconnect and verified process
   cleanup remain tied to the exact Agent Session and launch generation.

Freebuff and managed Antigravity remain excluded. Scripted meetings, automatic
research/synthesis/task assignment, voice, Mafia and RimWorld remain out of scope.
Real provider execution waits until final closeout under the configured six-provider
matrix; local acceptance must not claim those client runs.

## Authority and data ownership

Reuse mature HTTP/WebSocket/MCP/cryptographic/process libraries and existing
purpose-ticket, session-bearer, room-event, receipt, turn and provider mechanisms.
Keep product identities and credentials explicit. Human invite payloads and human
browser sessions must not admit any agent path. A saved friend is metadata, not
admission authority; a companion derives only the admitted human's allowed room
scope, never local-manager authority or a provider credential.

The persistent admission owner binds invitations and admitted sessions to server
identity, current room UID, participant, scope, expiry and client custody kind.
Use distinct credential purposes and fingerprint storage; credentials stay out of
public events, provider prompts, logs and committed files. The provider-specific
model/runtime contract remains with the provider owner, not generic admission.
Room membership and visible participant transitions commit with their durable
room event and retry result. The normal room owner publishes committed changes.

Revalidate exact session and current membership in the transaction for every room
operation. Deletion/recreation, archive, revocation, kick, mute and read-only state
must not be bypassed by an old credential or cached MCP view. Side chat and private
provider data are unavailable to connectors and attendees. AgentBridge reports
cannot claim another participant, room, Agent Session, turn, execution or launch.

## Failure, concurrency and lifecycle

One-use admission has one winner. A lost response may be retried only by the same
client with the same operation identity; a retry must recover the committed result
without admitting again or resetting expiry. Wrong-purpose/provider/room requests
fail before effects. Expired, revoked and changed-incarnation credentials fail
explicitly. No bearer or provider failure falls back to a human or host principal.

MCP wait and room synchronization are event driven with bounded retained history
and explicit resync when a cursor falls outside retention. Disconnect is distinct
from leave. Reconnect preserves identity and committed command/report receipts;
uncertain in-flight effects are resolved at their owner. Queue/backpressure and
session limits follow concrete protocol/custody requirements and have explicit
failure and cleanup; no speculative periodic work is introduced.

External clients own their provider processes and private state. A server stop
request is a request to that owner; completion requires its authenticated exact
report, never a fabricated local process result. Managed children are stopped and
reaped by the existing process owner before custody is released. Cleanup failure
remains observable and retryable. Provider content is not inferred from terminal
text when a native transport contract exists.

## Dependency order and acceptance

First connect the shared admission/session and room-operation boundaries. Complete
one vertical Room Connector flow, then the external attendee/entry-packet flow and
managed bridge flow using existing provider contracts. Build all three before
polishing or optimizing one target.

Verify exact credential domains, one-use concurrency and response-loss retry,
provider matching, bounded views and permitted tools, publication through the room
owner, reconnect/resync and receipts, read-only/mute/revoke/room-recreation behavior,
private-data exclusion, stop/crash and process custody. Reuse existing contract tests
and add only cases missing for these new boundaries. Never launch a real provider
as a local test substitute or simulate a provider's claimed external acceptance.

Direct packaged desktop/mobile verification must create/copy the connector invite,
show the actual admitted connector participant and public message, and verify its
leave. Exercise saved-friend and admitted-human companion packets and the visible
managed bridge controls once connected. Use isolated app data and owned local
protocol clients; final actual provider clients add integrated acceptance later.
Measure CPU, memory, latency, disk and process/task costs at owning boundaries.
Run affected verification and unchanged architecture/security/structure gates,
then obtain Daybreak approval of every phase commit, cumulative range, exact HEAD
and whole local Phase 7 before Phase 8.

## Connector admission owner

The first implementation establishes a separate connector invitation/session table,
AI participant kind and one-hour invite/session expiry. Existing purpose-separated
HMAC derivation stores fingerprints, never raw invite or session bearers. Creation
receipts remain until room deletion: pruning a deterministic creation request
would let an expired credential acquire a new expiry. The original remote MCP owner's 128-connection bound belongs to its live client
handles; it is not applied as a new limit on manager-created invitations. Explicit
host-created receipt history adds disk rows but no background work.

The existing local/paired manager provenance is now named `RoomManagerAuthority`
because connector invitation creation is its first non-image consumer. This is
an owner rename, without new variants or permission policy. Admission commits one
agent participant and public event with its exact client/request retry result.
Session authorization checks current membership, scope, expiry and room UID;
leave writes an agent-typed event and revokes only that session. Room archival
revokes both pending connector invitations and admitted sessions permanently.
Two controlled concurrent/expiry/leave/archive tests pass, as does affected
all-target/all-feature Clippy. The HTTP/MCP and packaged entry points are next;
this storage checkpoint alone is not Phase 7 acceptance.

## Connector mutation transport

Dedicated `/api/room-connector/join` and `/command` routes retain connector
credential purposes and return explicit committed/rejected/unresolved outcomes.
Admission and writes use the existing bounded room queue and durable publication.
The queue's owned session enum prevents browser/connector provenance combinations;
message, vote and randomness transactions reuse the existing authority resolver.
Connector commands cannot dispatch side chat, browser or manager controls.
The queue implementation is separated from the room task/lifecycle loop rather
than expanding that already-large owner. No timer, polling task, provider process,
second mutation queue or credential cache is added. Each explicit request performs
its authorization and existing room-budget work; resource measurements remain at
whole-phase acceptance. The local HTTP join/send/replay/read-only/leave boundary
passes, alongside both existing queue tests and exact-session transaction checks.
Affected all-target/all-feature Clippy and unchanged architecture gates pass.
Manager UI, bounded reads/wait, MCP and other two custody targets remain pending.

## Connector public reads and moderation

Snapshot, search/context and vote summary use the existing read owners with exact
session provenance resolved in the read transaction. Native and paired snapshot
callers now pass their existing authority explicitly. Connector responses omit
Agent Session configuration, provider catalog and side chat; event visibility uses
the canonical public projection. `read` retains the original last fifty messages.
`wait` returns the bounded pending range (at most the existing 200-event snapshot
window), excluding the caller's own messages; a gap or invalid cursor requires
explicit resynchronization instead of silently dropping pending messages.

A wait registers its room event and revocation receivers before the snapshot and
then awaits events, revocation, exact session expiry or shutdown. HTTP cancellation
drops the wait and its existing room connection lease. Existing connection and
history admission owners bound concurrency and read work; no polling, heartbeat,
credential cache or independently spawned task is added. Reqwest's query feature
uses maintained URL serialization for the client read protocol.

Mute/kick/export distinguish persistent Connector custody from managed Agent
Sessions. They preserve room permissions, revoke exact connector access and report
no fabricated provider cleanup. Local TCP wait/search/context/vote checks pass
alongside a moderation transaction test and seven affected existing search/pairing
checks. All-target/all-feature Clippy, architecture/format/diff and artifact checks
pass. Actual MCP/current-conversation and packaged invitation flows remain pending.

## Current-conversation transport client

The external client owns HTTP transport, private admission/session state and one
pending command receipt; it never opens the room store or launches a provider.
URL parsing and bounded response decoding are separate from connection lifecycle.
Redirects are disabled so admission and session credentials remain on the chosen
server. Remote-service destination lists compare normalized exact server bases;
credential purpose remains enforced by the server's existing admission owner.

Uncertain admission retains its original request UUID and client secret. Once
admitted, failed initial observation retries only the read. Repeated joins check
current authority rather than reporting a cached live membership. One unresolved
command retains its exact request and payload; a different command is rejected
until that receipt is resolved. Wait observation has separate custody, so a pending
wait does not block a contribution or explicit leave. Reads do not consume pending
observations. Close cancels owned transport and never claims a provider stop.

Direct local-client flow and a controlled HTTP relay pass. The relay invalidates
responses only after the real server commits admission and publication; retries
recover the original participant and command receipt, with exactly one message.
The client introduces no worker process, background retry or polling task. Each
connection owns one HTTP pool, one admission record and at most one unresolved
command; response allocation is bounded by the canonical 200-event window and
Unicode/message metadata allowance. MCP tool/CLI wiring and packaged UI remain
pending; affected Clippy and unchanged architecture/format/diff gates pass.

## Current-conversation MCP and CLI

`assemble room connector-mcp` exposes the fourteen retained room tools over stdio.
`assemble room connector-mcp-remote --allow-room-server <base>` exposes loopback
Streamable HTTP at `/mcp`; a user-owned tunnel must preserve the loopback Host.
The remote registry admits only exact normalized destination bases and requires a
private opaque connection handle for every subsequent tool. Stdio owns one current
conversation; the remote owner retains the original 128-connection bound, including
uncertain joins. Repeated admission reuses the client's original identity and receipt.
Registry locks cover reservation/removal only, never room network waits.

Actual subprocess MCP verification passes join/rejoin, fourteen-tool discovery,
public read/search/contribution, a concurrent wait and contribution on the same
MCP connection, confirmed leave and process exit. A separate MCP registry test
passes destination and handle isolation with two actual room participants. Remote
HTTP/tunnel deployment remains unverified. No real provider was launched.

An interrupt test reproduced a retained process while Tokio's blocking stdin read
remained open. The dedicated CLI now joins MCP/network work and explicitly releases
its runtime without waiting for that uninterruptible OS read. Both EOF and Ctrl-C
exit pass with no retained connector process. This changes only the new CLI runtime;
external providers remain user-owned. Three local tests, affected all-target/all-feature
Clippy and unchanged architecture/format/diff gates pass. The repository artifact
owner cleaned the 18.97 GiB cache after builds stopped, as required by its existing
18 GiB gate. Invitation UI and the other two Phase 7 custody targets remain next.

## Connector invitation management

The bundled manager now issues an exact connector-create ticket through the local
control pipe. Its private HTTP endpoint revalidates current room-manager authority,
requires ready public ingress and calls the existing one-hour, one-use creation
receipt owner. Human and connector creation tickets cannot cross purposes. Request
and response bindings derive from the Rust protocol. The shared consumed local
manager ticket wrapper is named for its existing cross-domain responsibility.

The invite dialog adds a separate current-AI invitation card. A dedicated hook owns
unconfirmed creation identities, link custody and one expiry deadline. Creation
retries preserve the request UUID; copy rechecks current manager authority and
fresh public ingress immediately before clipboard dispatch. Tokens are not rendered
in the card or persisted by the frontend. The native invite codec is separated from
the control pipe owner after its added purpose reached existing function-size gates;
no gate was relaxed. One real-HTTP creation/admission test and 41 affected frontend
tests pass. Server/protocol and desktop Clippy, 28 desktop tests and unchanged
architecture/format/diff gates pass. Packaged desktop/mobile proof is in progress;
external attendee and managed bridge custody remain pending.

## Packaged Connector corrections and proof

Actual connector admission exposed an invalid frontend assumption: every agent
participant was required to have a managed Agent Session. This threw during mention
projection and rejected later snapshots. Connector membership now uses the canonical
Participant identity for mentions and ordinary member presentation. Every reported
Agent Session still requires its exact agent participant and unique session binding;
no provider/session data or process controls are synthesized for a connector.

The live member panel also exposed a user/participant ID mix-up in invitation
ownership. Admission now resolves the creating user's canonical profile binding to
its participant ID before storing the member. The invitation receipt continues to
use the creating user ID. The existing expiry/archive admission test checks this
owner relationship. Button styles and dialog wording now include the AI invitation.
Fifty-seven affected frontend tests, frontend build, the affected persistence test,
and server/persistence all-target/all-feature Clippy pass.

The isolated packaged app directly created and copied an invitation. Its persisted
creation receipt was recovered through the normal store owner while the app was
stopped, then used by the actual HTTP Connector client through the app's managed
Cloudflare ingress; credentials remained in process memory. The client exposed the
rendering defect. After correction, a separate same-owner invitation verified visible
agent membership, public message and confirmed leave in the desktop package.
At 390 by 420, direct manipulation verified invitation creation/copy, lower-card
scrolling, Escape, visible conversation and member panel, correct grouping under the
inviting human, and removal after confirmed leave. Clipboard success was observed
through the app; raw clipboard contents were not inspected. No AI provider ran.

A post-flow sample showed app RSS 122.4 MiB, supervisor 11.4 MiB, server 57.5 MiB and
owned tunnel 43.3 MiB; CPU was 0/0/0.2/0 percent respectively. This is a point sample,
not a whole-phase benchmark or total WebKit-memory measurement. The debug package
occupied 177.8 MiB. All owned app/server/tunnel/probe processes ended before cleanup;
only this run's app data, cache, WebKit data and bundle were removed. The existing
artifact owner's measured cleanup plan removed the obsolete 1.1 GiB desktop target,
retaining the below-limit shared target needed by ongoing work. Unchanged
architecture/format/diff/artifact gates pass. External attendee and managed bridge
flows remain the next Phase 7 targets; whole-phase review has not been requested.

## External attendee invitation custody

A separate invitation table and credential purpose now own saved-AI-friend and
human-companion entry packets. Saved-friend selection is resolved under exact room
manager authority; its receipt retains the originally selected provider and name,
even if the address book later changes or deletes the contact. Contact metadata
alone never admits a participant or starts a provider. Creation retries revalidate
the current authority and preserve identity and expiry, including after expiry.

Companion creation requires a live posting, unmuted human session. It retains that
exact parent fingerprint, identity and scope rather than acquiring manager authority.
The original ten-minute default, one-hour maximum and eight pending/admitted
companions per owner remain the relevant bounds; the default invitation also cannot
outlive its parent session. Admission must retain parent provenance, require an
explicit matching supported provider and establish separate attendee session custody.
Provider availability and local runtime selection belong to the attendee, not the
host catalog's installed executables. Host archival revokes the new invitation domain.

The local invitation boundary passes two tests: immutable friend receipt recovery
without membership/provider effects, and concurrent companion capacity plus revoked
and read-only owner rejection. Creation introduces no periodic task or process;
it adds one durable row per explicit invite, retained until room deletion to prevent
expired request identities from minting fresh credentials. Admission, assigned-turn
transport, provider execution and packaged packet controls remain pending.
Affected all-target/all-feature persistence Clippy and unchanged architecture,
format, diff and artifact gates pass for this invitation checkpoint. The session
columns reserve the admission receipt under the same invitation custody; no session
is issued until its admission transaction and exact provider boundary are connected.

## External attendee admission transaction

Admission now compares the explicitly selected provider before effects, admits one
client/request pair, and commits its agent participant, external-owned Agent Session
and public event together. Response-loss retries recover that event, credential and
original expiry; a changed client, request or display name cannot consume the invite
again. Companion admission and later authorization revalidate the exact posting
human session, with child expiry bounded by the parent's expiry. Human, Connector
and attendee credential fingerprints cannot authorize each other's session domains.

An admitted attendee is disconnected and not provider-ready until its authenticated
runtime report arrives. Its record contains no host executable, workspace, provider
endpoint, provider credential or claimed running process. Common empty durable state
initialization is shared with managed session creation; each custody owner supplies
its own runtime authority. Host launch/configuration reject external custody, and
host process reconciliation excludes external-owned sessions. Participant removal
revokes the attendee credential; no ordinary human transport is widened.

The admission/creation tests pass (three cases), including concurrent one-use
admission, matching-provider failure before effects, exact retry, private authority
separation and parent revocation. Eight existing managed-creation cases and three
room-lifecycle cases pass. Affected persistence Clippy and unchanged architecture,
format/diff/artifact gates pass. This transaction adds no process, timer or background
worker. Authenticated readiness, turn/report/request transport, external cleanup and
packaged admission controls remain the next dependency; this is not a completed
external CLI flow or whole-phase acceptance.
