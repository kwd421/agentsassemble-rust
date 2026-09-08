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

The external attendee client retains one provider-bound admission request/client
secret through response loss, then retains only the admitted session bearer.
It shares URL normalization, bounded JSON response reading and redacted rejection
decoding with Connector while retaining separate credential purposes and client
state. HTTP redirects are disabled. Tool/control retries use caller-retained exact
request identities; transport errors never expose a credential-bearing URL or raw
remote diagnostics. The client does not open a room database or infer host authority.
The local TCP client test drops admission, leave and cleanup acknowledgments after
their upstream commit and recovers each original receipt. Both existing Connector
client/retry tests pass against the shared transport and loss-injection helper.
Affected all-target/all-feature Clippy and unchanged mandatory gates pass. Native
attendee socket/runtime integration and packaged entry packets remain in progress.

The native attendee socket now uses the same typed request/frame codec as the
server. A fresh private-header handshake claims only network custody. TCP/TLS,
upgrade and the initial claim have one ten-second deadline; writes are bounded by
ten seconds and frames by the existing 256 KiB protocol limit. WSS uses rustls with
WebPKI roots. Send and receive remain separate so interrupts cannot be consumed as
an assumed ACK. Ping/pong maintains network liveness only; socket closure never
asserts runtime shutdown. The client adds no polling, provider process, queue or
background task. Pending requests and local runtime ownership stay with its caller.

The local native-client socket test passes admission-before-connect rejection,
ready acknowledgment, connection replacement, ping and cleanup retaining the exact
runtime identity after socket close. All four existing attendee socket flows pass
with the shared codec, including turn/result retry, stop and interrupt. TLS increased
one integration-test future to 20,408 bytes; it is boxed at that test's case boundary
without changing a gate. Affected Clippy and mandatory gates pass. Provider runtime
execution, request relay and packaged entry flows still remain before acceptance.

Initial session/profile construction now lives in the domain owner so the external
CLI can use its validated local provider selection without opening a room database
or constructing a host principal. Custody is explicit (`Server` or `External`);
construction creates no lease, process, membership or turn authority. Managed
creation still resolves persona and message cursors inside its original transaction.
Five managed create/start tests and twenty attendee persistence cases pass, as do
affected Clippy and unchanged mandatory gates. CLI runtime wiring remains next.

The external local runtime owner now consumes that profile and an exclusively owned
provider adapter. It reserves an actual local runtime lease before launch, projects
readiness from the adapter's confirmed session and native interrupt capability, and
retains ownership across HTTP/socket loss. Stop requires an exact positive adapter
cleanup observation; mismatched cleanup tuples or pre-stop reports fail. A server
that never committed readiness may request an empty tuple, but local cleanup must
still finish first. Local lease artifacts are released only after committed cleanup
acknowledgment. Runtime initialization errors remain visible and still require stop.
The private turn delivery now includes its canonical input history sequence, stable
across reconnect, for the client's existing room-tool authority contract.

A local child-process fixture passes start/reuse, readiness, leave, pre-stop rejection,
positive exact cleanup, wrong-lease rejection and artifact-release acknowledgment.
The sixteen existing managed-session boundary tests pass after their existing
catalog fixture moves to its shared test owner. The affected turn-delivery case,
Clippy and unchanged mandatory gates pass. No real provider was run. Native turn
execution/tool/request relay and CLI/packet entry still remain before acceptance.

The native execution owner now accepts only its runtime's increasing exact turn
generation, retains one owned task and completed report across socket replacement,
and compares recovered input/authority without re-entering the provider. A recovered
dispatch not yet entered by this same live local owner may start once; an already
reported provider turn without local custody fails. Completed results retain one
started/report request identity until acknowledgment. Uncertain or stopped-runtime
errors require cleanup and are never converted to ordinary successful completion.
The domain vote owner now encodes the canonical payload consumed by attendee reports.

The real local socket/child-process test passes execution replacement, altered-input
rejection, one observed native turn/start, one public result and report-receipt
recovery after closing before ACK consumption. Its initial test client omitted the
required room subscription; the test flow and bounded receives were corrected, and
all owned test processes were observed gone after stopping that run. Seven vote
contract cases pass, including canonical payloads for all four variants. Affected
Clippy and unchanged mandatory gates pass. This adds one owned task per active turn,
retaining bounded assignment/result data, with no timer or room-state polling.
Interrupt/tool/request relay and CLI/packet entry remain in progress.

Native portal tool calls now retain their existing reservation while crossing the
attendee HTTP boundary. Read responses share one typed server/client codec. Random
calls fix one request UUID and canonical payload when the native reservation enters
execution; a retry changes only current connection custody. The caller explicitly
completes the native reply after a result, preserving uncertainty until its owner
resolves or rejects it. Attachment decoding shares the managed provider projection;
its native command still validates exact attachment ID, metadata and byte bounds.
No retry loop, extra room queue or provider process is added by this relay.

The existing local child-process/socket test now exercises native MCP search,
message context, randomness and bound attachment reading through real HTTP. It
verifies one random result/event across a repeated call, exact attachment bytes,
one native execution and one terminal room result across reconnect. It passes in
3.32 seconds, alongside all four existing attendee socket cases. The test's initial
random-event assertion used the wrong event type; it now checks the canonical
`message_final` projection's `message_source=room_tool_result`. Affected Clippy and
unchanged mandatory gates pass. Interrupt/request handling and CLI/packaged entry
remain pending; no real provider was executed.

The native interrupt owner now retains one exact server delivery, interruption task
and positive report across connection replacement. Entered turns use the provider's
existing exact-turn control and quiescence deadline; a never-entered newer generation
uses this exclusive live owner's runtime observation without creating provider input.
Only a committed interrupt receipt releases local turn custody. Uncertain native
interruption produces no report and requires the existing explicit cleanup path.

The local socket/child-process test passes pre-entry interruption, native retained
interruption and rejected native interruption, including replacement, receipt retry,
one provider start/interrupt and positive final cleanup (10.15 s). The shared managed
interrupt fixture passes its existing boundary test (3.28 s). Early provider-leader
exit did not prove descendant cleanup: the existing guardian returns
`provider_turn_interrupt_unconfirmed` and `provider_stop_unconfirmed`; that negative
observation is not a runtime-gone success claim. Clippy and unchanged architecture,
19 policy, format, diff and artifact gates pass. This adds one owned task per active
interrupt and reuses the canonical bounded quiescence wait, with no room polling.
CLI entry/event-loop integration, provider-request handling, entry packets and
managed bridges remain pending; real providers have not run.

External CLI discovery now selects one existing provider registration before starting
the catalog owner's discovery task. The managed app still discovers all registered
providers through the same owner. Unknown/excluded providers fail before any probe;
the selected path does not probe unrelated providers or fetch their remote catalogs.
The local Custom API selection test verifies the one-provider publication and
excluded-provider rejection without launching a CLI or contacting a provider.
Thirteen existing catalog cases and this selection case pass, along with affected
Clippy and unchanged mandatory gates. An initial empty test-name filter was corrected;
the reported counts are from the actual matched tests.

The CLI execution loop will own socket replacement and retained execution/interrupt
receipts until committed acknowledgements. Reconnection uses the original one-second
delay, bounded by admission expiry and cancellation. Network keepalive derives from
the server's idle deadline; no room-state polling is introduced. Exit stops the exact
local provider before submitting positive cleanup; uncertain cleanup preserves owned
workspace artifacts and remains visible. Shutdown receipt recovery has a finite
thirty-second budget and retains request identity throughout.

`assemble room attend` now consumes its invitation through hidden terminal input
(`rpassword`) or bounded piped stdin, selects the existing provider catalog contract,
and owns the default temporary or explicitly selected workspace. Its event loop
connects native execution, interruption and bounded room-tool/attachment reservations;
pending reports and random IDs survive replacement. Recovered result receipts precede
new readiness, so the next turn can be delivered. SIGINT/SIGTERM, expiry and remote
stop flow through positive local shutdown and exact cleanup; failures preserve the
temporary workspace. Discovery, admission and cleanup errors remain visible.

The actual CLI child-process test passes discovery, provider-bound admission, native
MCP publication, one public result and operator stop with confirmed cleanup (3.71 s).
Only the selected local Codex protocol fixture runs, with isolated PATH/CODEX_HOME
and temporary data. The first fixture returned plain assistant text instead of the
required room-tool result; replacing that fixture behavior with `publish_message`
proved the actual retained contract. No fixture provider process remained afterward.
The two client receipt/socket tests, native execution/tool-relay test and three-case
native interrupt test also pass. Affected Clippy, architecture/19 policy, formatting
and diff gates pass. The artifact gate detected 20,398,231,552 allocated cache bytes;
the existing maintenance owner is applied with Cargo/Tauri idle, without changing
the 18 GiB gate. Integrated reconnect/interrupt CLI races, provider-request relay,
entry-packet UI and managed bridges still precede Phase 7 acceptance.

### Friend and companion entry-packet boundary

The friend endpoint consumes a distinct exact-room native manager ticket, including
room incarnation and bootstrap lineage. The companion endpoint accepts only its
currently admitted human bearer; read-only, departed and other credential purposes
fail. Both call their existing invitation transaction owner, require ready public
ingress and return one private/no-store typed packet with the canonical CLI selector,
hidden-input invitation and expiry. Neither creates an agent session or launches a
provider. The native command registry, capability and generated permission agree.

Saved contacts may contain a provider alias. The provider registration resolver is
now invoked on that exact stored metadata inside the invitation transaction, before
insertion, yielding the same canonical kind the attendee presents on join. This avoids
a stale preflight read or a second provider registry in persistence. Unsupported
contacts fail, while receipt recovery after contact deletion preserves the original
invitation. The shared packet decoder checks request, room incarnation, public origin
and the exact command shape before exposing copyable text.

Actual HTTP checks pass friend creation/retry after deletion, wrong-purpose rejection,
normalized attendee admission, unsupported-provider rejection, human read-only denial,
companion creation/retry, parent departure rejection and attendee-owned final cleanup.
Parent departure does not claim provider absence: an explicit WebSocket handshake
rejection now terminates CLI reconnection and reaches its existing cleanup owner.
Timeouts, rate limits and transport uncertainty retain bounded retry behavior.

The two invitation persistence tests, five manager HTTP tests, ten private-control
tests, 28 native desktop tests and 17 frontend API/bridge tests pass. Affected Clippy,
production frontend/CSS, unchanged architecture/19 policy, format, diff and artifact
checks pass. Adding the native command required updating its exact expected inventory
to 28. Private-control decoding now owns its typed rejection instead of passing an
untyped error tuple to the dispatch owner; its existing ten boundary tests pass.
No provider turn ran in these packet tests.

The manager invite modal now shares one saved-friend directory between its human
and AI selectors, and one manager/origin/expiry owner between connector and attendee
packets. Uncertain receipts are retained per friend and purpose. Admitted posting
humans have a companion card in desktop and mobile member panels, backed by one
state owner for both layouts. It preserves form-specific retry receipts, drops
responses from retired sessions, and copies only current, unexpired packets. The
existing clipboard dispatch owner is shared without changing its behavior.

Affected frontend verification passes 53 cases, including per-friend receipt
identity, full packet copying, companion retry/expiry/session replacement, contact
selection and existing invite/member panels. Production frontend and CSS checks
pass; unchanged architecture/19 policy, format, diff and artifact checks pass.
The companion UI adds no polling or provider process; its single expiry deadline
is derived from returned packets. Direct packaged entry-packet verification and
full Phase 7 acceptance remain pending.

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

External turn-tool reads authenticate the bearer and current connection together
with the exact execution generation in the read transaction. Room, participant,
turn and input boundary come from stored custody. Lobby search/context and bound
input attachments reuse their current owners; attachment preparation retains its
pre-start phase restriction. Managed turn tools retain their server-custody check.
These reads add no queue, timer, cache, receipt or provider process. Reconnect,
interrupt, expiry and terminal completion must reject stale tool reads; the HTTP
surface is private/no-store and cannot select a side channel or host path.
Local TCP search/context checks pass for the current connection, with rejection
of its replaced connection, changed execution and completed turn. Bound attachment
pre-start reads pass and pending/unbound or post-start reads fail. Existing managed
search, exact attachment binding and both randomness tests pass; affected
all-target/all-feature Clippy and unchanged architecture/format/artifact gates pass.

External randomness is generated by the existing server random owner and queued
through canonical room mutation/publication. Its transaction revalidates current
connection and exact turn, then shares tabletop mode, participant, result validation,
per-turn limit and room write budget with managed tools. The original request UUID
and payload bind a receipt to the first committed result; reconnect or a lost reply
cannot roll again. Exact replay requires live admission/connection but can recover
a result after that turn completes. No client-supplied random result is accepted.
The concurrent distinct-result/lost-reply/terminal-replay test passes. Local TCP
roll/replay/conflict checks observe the matching canonical room event exactly once.
Both existing managed randomness tests and the shared attachment-read regression
pass, as do affected all-target/all-feature Clippy and unchanged mandatory gates.

Revalidate exact session and current membership in the transaction for every room
operation. Deletion/recreation, archive, revocation, kick, mute and read-only state
must not be bypassed by an old credential or cached MCP view. Side chat and private
provider data are unavailable to connectors and attendees. AgentBridge reports
cannot claim another participant, room, Agent Session, turn, execution or launch.

## Shared provider-request contract

The reachable original request owner allows permission choices, question answers
(including secret answers) and HTTPS external-action acknowledgement. Only the
current owning human may resolve a request; room management alone does not substitute
for ownership. Open/resolve/closed transitions are tied to one exact session,
launch and execution. There is at most one open request per session, with a bounded
15–900 second deadline. Duplicate commands recover their existing result; changed
payloads, replaced executions and expired authority fail before delivery.

The Rust domain owns the typed request/answer validation and secret-free durable
projection. The persistence transaction owns pending/resolving/terminal state and
owner-only events; live delivery owns secret answers until completion or cancellation.
Secret values never enter command receipts, event history or diagnostic output.
A lost live secret delivery is an explicit failed resolution, requiring a new provider
request instead of reconstructing answers from storage. Both managed and attendee
transports consume these same owners. Native drivers translate supported upstream
request/response shapes; unsupported requests fail explicitly rather than receiving
an automatic permission grant. Stop, interrupt and runtime loss cancel live delivery.

Acceptance includes an actual local request/owner response round trip, wrong-owner
and replaced-launch rejection, concurrent/exact retry, deadline/stop cancellation,
secret-free persisted state and ordinary-view projection, then packaged desktop and
390px controls. These are pending until the transport and native consumers exist.

The shared domain model now validates offered choices, exact question coverage,
answer multiplicity and HTTPS action URLs. Its distinct durable resolution removes
secret answer values and retains only answered question IDs. Two focused contract
cases pass, including secret exclusion and invalid/duplicate choices. Domain
all-target/all-feature Clippy and unchanged architecture/19 policy, format and
artifact gates pass. Both Cargo lockfiles include the existing URL parser dependency;
no gate was changed.

Schema 69 adds session-owned request state with one pending request per session.
Both managed and admitted external open operations reuse the current execution
authority inside an immediate transaction, derive the human owner from membership,
and commit one owner-only event. Exact concurrent retries recover that event without
renewing its deadline; changed payloads, replaced connections, expired requests and
wrong custody are rejected. Deadlines use the stored millisecond precision in both
the first response and replay. The focused storage case covers these boundaries and
ordinary-view hiding. Resolution, terminal lifecycle and live delivery still need
integration before request acceptance; this checkpoint does not expose a new route.

Schema 70 binds external requests to their opening connection and retains resolution
and terminal event identities. Human resolution revalidates actual ownership,
membership, posting authority, admission and exact execution in the same transaction.
Concurrent matching responses yield one non-serializable delivery claim; a keyed,
purpose-separated response fingerprint detects changed secret answers without storing
their values or a guessable raw hash. Delivery completion, exact-execution cancellation
and stored-deadline expiry own separate terminal transitions and private events.
Three focused persistence cases pass, covering single delivery, secret-free storage,
wrong-owner rejection, terminal replay, expiry, cancellation and replaced connections.
Persistence all-target/all-feature Clippy passes. Live broker, startup reconciliation,
transport/native consumers and packaged controls remain pending for this contract.

Existing session-state transitions now reconcile their one pending request using
the same current owner/execution authority as resolution. Disconnect, replacement,
turn completion, stop and interrupt cannot retain a usable pending request. Mute,
leave and shared participant access revocation explicitly cancel requests owned by
or originating from that participant. Before network admission, startup fails all
remaining live requests in bounded transactions; it never recreates secret answers.
These transitions feed existing canonical event catch-up. They add an indexed
pending-session lookup per state transition, with no periodic scan or new task.
The local fixture exercises disconnect, completion, interrupt, mute and lost-delivery
startup; all 320 persistence tests pass. Actual live response delivery remains pending.


The live exchange separates the answer slot from native delivery acknowledgement
and the durable completion receipt. It never serializes answers. Dropping either
execution owner wakes cancellation; a cancelled wait can resume without submitting
a second delivery report. The focused exchange test verifies that an answer alone
cannot complete delivery, the answer slot is single-use, and a lost native owner
wakes the broker. Provider-request protocol consumers remain pending.


The room actor now owns a bounded live request broker shared by managed and attendee
entry methods. Its 64-entry queue and 64 live requests per room return explicit
capacity errors; the durable one-pending-request-per-session rule is unchanged.
Exact open retries retain the original recipient. Human answers use a typed path
outside generic command receipts; only the first durable claim reaches that recipient.
Upstream acknowledgement completes persistence before releasing the native waiter.
A lost waiter cancels its exact execution request. Canonical room inputs reconcile
the indexed durable pending set only while live requests exist; no periodic scan or
per-request task is added. Each pending request has one cancellation/acknowledgement
future and one bounded deadline. The monotonic deadline cannot be renewed by replay
or extended by a wall-clock rollback. Storage failures remain explicit failed or
unresolved delivery, never a successful native receipt.

Three broker integration cases pass against the real store and room actor: concurrent
secret answers have one recipient and require native acknowledgement, disconnect and
lost native custody cancel promptly, and a controlled deadline expires the waiter.
Secret answers are absent from stored events. Server all-target/all-feature Clippy,
unchanged architecture/19 policy, format and artifact gates pass. WebSocket, managed
native ingress and provider/UI consumers remain pending; this is not packaged proof.


The human response action is now `provider.request.resolve` on the existing room
socket. Its request identity is the provider request UUID and its payload is the
typed resolution. The direct socket owner bypasses generic receipt persistence,
revalidates local room incarnation or durable browser provenance, and delegates the
ownership/execution decision to the same request transaction. Its ACK contains only
the request and resolving-event identities plus exact-retry status. Room management
cannot answer another human's request. The real WebSocket case passes owner response,
local-manager refusal, exact retry, secret-free ACK and native delivery completion;
the three broker cases still pass. Generated action bindings, frontend production
build/CSS check, server Clippy and unchanged architecture/19 policy and artifact
gates pass. Visible request controls and attendee/native relay remain pending.


The attendee socket now opens provider requests and relays the one live answer on
its exact connection. Native delivery reports are acknowledged only after the broker's
durable completion. The connection retains one metadata-only delivery receipt for
exact ACK-loss retry; changed reports conflict. Replacement cancels the old exchange
and cannot inherit its answer. Failed/unconfirmed delivery produces an explicit
unresolved response. Waiting for answer, acknowledgement or cancellation adds no task
or polling loop. The socket fixture verifies the secret answer round trip, both
retry points, changed delivery rejection, and replacement fencing. All nine affected
broker/attendee socket cases pass, as do server Clippy and unchanged architecture/19
policy, format and artifact gates. CLI/native request creation and visible controls
remain pending; ordinary native sessions still reject unsolicited response frames.


Managed turn creation and recovery now carry one explicit room ingress bundle for
tools, attachments and interactive requests. Request descriptions are validated by
the domain before entering the separate bounded native queue; storage revalidates
request authority at its own boundary. Native callers receive the same live exchange,
so losing a queued caller cannot retain an answer recipient. No global callback,
additional task or provider-specific authority branch is introduced. The five broker
cases and the existing local Codex canonical-turn fixture pass, along with server
Clippy and unchanged mandatory gates. Native request mapping and external CLI ingress
consumption remain the next consumers; no real provider execution is claimed.


The external CLI session now carries the native request ingress into its exact local
execution. One connection-local relay owns the open envelope, live answer, native
completion and server ACK. It retains at most one active exchange and four queued
native opens; extra concurrent opens fail explicitly. Native completion cannot finish
until the remote receipt arrives. Disconnect, interruption, closed requests and lost
ACK custody clear the live exchange instead of replaying an answer after reconnect.
A protocol rejection or the existing 30-second ACK bound remains a visible failure.
The controlled callback test passes both committed completion and lost-connection
failure. The actual CLI child-process fixture still passes admission, one result and
confirmed remote stop (3.93 s). Server Clippy and unchanged mandatory gates pass;
native provider request mapping and packaged response controls remain pending.

Codex now translates its retained native command/file/permission approvals and user
questions into the shared request ingress for both managed and attendee turns.
Thread/turn matching occurs before admission; early requests preceding the native
turn/start receipt remain in the existing bounded queue. Human response waits sit
outside the native inactivity deadline, with exact-turn interruption dropping the
live exchange. Policy amendment objects stay native-side; durable descriptions use
the existing redaction owner and oversized prompts fail validation. A flushed
JSON-RPC reply is the upstream delivery boundary, followed by the room receipt.
The local process case verifies early secret question delivery, one exact native
response, the durable receipt barrier and official interrupt cancellation. Native
policy choice mappings and 23 affected Codex tests pass, as do provider all-target/
all-feature Clippy and unchanged architecture, policy, format and artifact gates.
Other native mappers, visible controls and managed bridge custody remain pending.

OpenCode now carries same-session permission/question events through its native
HTTP reply endpoint and the shared room receipt. Ordered/multiple/custom answers
retain their upstream meaning; only explicitly offered project-wide permissions
can be selected. The existing bounded SSE parser yields requests to the turn owner,
which holds one active-time deadline across prompt HTTP and SSE while excluding
bounded human waits. Ordinary HTTP requests retain their own deadline. HTTP
connection tasks now use the library's abort-on-drop owner so cancellation during
headers or body reception cannot detach the provider connection. The exact local
process case, all 16 affected OpenCode cases and five HTTP cases pass, including
receipt ordering and observable connection EOF on cancellation. Provider all-target/
all-feature Clippy and unchanged architecture, policy, format and artifact gates
pass. This does not close the other native mappings, visible controls or bridges.

The owner-only pending-request projection now belongs to the same read transaction
as room snapshots. It includes the typed prompt, session, deadline and open/resolving
state, never answer values. Revalidated browser ownership filters the indexed
pending set; internal and AI snapshots carry no requests. Resolving requests remain
visible after their opening event leaves the bounded event window, and terminal
requests disappear. Browser wire validation and generated types include this field;
product surface revision 13 rejects clients predating the new snapshot contract.
The focused persistence case, five live broker/socket cases, 100 affected
socket/canonical frontend tests, frontend build/CSS validation, affected server
Clippy and unchanged mandatory gates pass.
Visible request state and response controls remain the next integration step.

## Failure, concurrency and lifecycle

The attendee's explicit leave uses its sealed cleanup custody, including after
ordinary session expiry. It revokes only that membership and requests the existing
exact runtime cleanup in the same transaction as the public leave event and retry
receipt. It does not grant room reads or complete shutdown without the external
owner's report. A transient socket disconnect continues to preserve runtime custody.
The expired-session concurrent leave/cleanup test and existing local TCP cleanup
flow extended to both kick and self-leave pass. The latter verifies private HTTP
retry acknowledgments, socket closure and canonical cleanup publication. Affected
all-target/all-feature Clippy and unchanged architecture/format/artifact gates pass.

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

## External attendee admission transport

The dedicated `/api/room-attendee/join` endpoint commits admission through the same
bounded room mutation queue and durable event publication owner. Provider IDs and
canonical kinds resolve from the existing registration owner without probing host
executables, model catalogs or credentials. Unsupported and mismatched providers
fail before membership effects. The shared HTTP credential codec preserves separate
Connector and attendee purposes; neither session gains human transport authority.

A real local HTTP test passes with an intentionally empty host provider catalog:
wrong/excluded providers are rejected, successful admission publishes an inactive
external session, exact retries recover the credential and expiry, and competing
clients and wrong-purpose transports fail. The existing Connector HTTP test passes
after extracting its shared fingerprint mechanism. Affected all-target/all-feature
Clippy and unchanged architecture/format/diff gates pass. This path owns one request
and one existing queue slot, with no additional task, timer or provider process.
Invitation packet controls, authenticated WebSocket execution and external cleanup
remain pending; admission alone does not claim a working provider conversation.

## Attendee connection and runtime boundary

Each WebSocket receives a fresh connection identity under its admitted attendee
credential. Replacement invalidates the previous connection's reports in the same
transaction that changes custody; an old socket's closing callback cannot disconnect
its replacement. A connection identity is distinct from the client-owned provider
runtime identity and the canonical assigned execution. Disconnect preserves runtime
and active-turn custody, including the ordered floor. New assignments require a
current connected attendee and current admission authority. Startup invalidates old
network connections before admitting requests, without declaring remote runtimes
stopped. Ready/reconnect reports must preserve the existing exact runtime tuple;
only confirmed external cleanup may release it for a different runtime.

The connection/readiness persistence owner now enforces this boundary. Its single
row per admitted attendee records `connected`, `ready` or `disconnected` and the
current connection UUID. Ready binds the reported runtime tuple and public profile;
repeat reports produce no duplicate event, and a replacement cannot report a new
runtime before cleanup. Network loss preserves provider-active and Busy/Stopping
custody while disabling assignment through the connection state. Automatic host
turn reconciliation skips externally owned executions instead of observing them
through the host provider adapter.

Four local tests pass: connection replacement and revocation cleanup, startup
network invalidation, exact running-turn preservation on reconnect, and queued
directed input remaining unassigned until readiness. Forty-four existing room-turn
cases and the managed blocking-turn restart case pass. Affected all-target/all-feature
Clippy and unchanged architecture/format/diff/artifact gates pass. Startup processes
at most 64 connection rows per transaction; there is no new periodic worker or
provider process. These methods are not yet exposed as a ready/report WebSocket;
authenticated turn delivery/reporting and exact external stop remain in progress.

## Shared terminal turn transaction

Message, vote, decline and failure completion now have a single transaction-local
implementation under `room_turn_completion`. Existing managed APIs retain transaction
ownership and call that implementation. This gives the attendee report owner a place
to revalidate connection provenance and recover a durable report receipt in the
same transaction as canonical turn completion, without prechecking authority and
then borrowing a trusted host principal. No terminal policy or event contract is
duplicated. The room-turn facade is reduced from 764 to 548 lines; the completion
owner is 308 lines. Forty-four existing room-turn tests, affected Clippy and unchanged
architecture/format/diff gates pass. Attendee report transport is the next consumer.

## Authenticated attendee result receipts

External message, vote, decline and failure reports now use the shared terminal
transaction under exact current attendee connection provenance. Room and participant
identity come only from the admitted session. The canonical command receipt owner
stores the report hash and public result atomically with completion. A same-report
retry recovers the committed result before requiring an active turn, while revoked
admission and replaced connections cannot recover or submit reports. Changed request
content conflicts. Runtime leases and dispatch nonces never enter the public receipt;
raw external diagnostics remain with the client. Failure does not claim process stop.

Two controlled local tests pass: concurrent response-loss retry with one public
message and connection/nonce/conflict/revocation rejection, and canonical vote,
decline and failure finalization with receipt recovery. Affected Clippy and unchanged
architecture/format/diff/artifact gates pass. Each report uses one existing durable
receipt and room write budget, without a new queue, timer, or process. Network turn
assignment, WebSocket reporting and external cleanup are still pending; this is a
storage boundary checkpoint rather than external CLI acceptance.

## Exact external turn delivery

The current ready connection can now consume or recover its one canonical assigned
execution. First delivery calls the shared transaction-local start owner. Repeated
delivery and reconnect retain the original nonce, execution, provider turn and
immutable bounded input, explicitly marked for client-owned resume/reconciliation.
The projection contains only that attendee's runtime authority and existing room
assignment envelope; no durable host session configuration crosses this boundary.
Interrupt/recovery effects retain their separate lifecycle owner rather than being
returned as fresh work. Started acknowledgements revalidate the connection in the
same transaction and permit only an exact Running replay.

The start owner is separated from execution finalization, and both managed recovery
and external delivery share stored-envelope loading and validation. Three external
turn tests and forty-four existing managed room-turn cases pass, including report
completion through the new delivery/start APIs, response-loss recovery, readiness,
replacement, changed start IDs and foreign-room rejection. Affected Clippy and
unchanged architecture/format/diff/artifact gates pass. There is no new persisted
queue, worker or timer. RoomRuntime/WebSocket integration and client runtime cleanup
remain pending before the external attendee flow can meet acceptance.

## Attendee room publication integration

Connection claims, readiness, terminal reports and exact disconnect now pass through
the existing bounded RoomRuntime mutation queue and durable publication owner.
Responses distinguish committed reports from unresolved reply loss. The same turn
publication helper advances managed assignments; external assignments remain with
the authenticated connection's durable delivery owner and never spawn a host adapter
task. No parallel queue or polling loop was added. The attendee branch is boxed at
the room dispatch boundary after Clippy measured a 17,000-byte combined future;
the existing future-size gate remains unchanged.

The local integration test admits an external session with an empty host catalog,
uses an actual human HTTP admission and WebSocket message, delivers its exact turn,
then verifies queued result publication, receipt replay and disconnected-but-active
external custody. It passes alongside the existing HTTP admission/isolation case.
Affected all-target/all-feature Clippy and unchanged architecture/format/diff/artifact
gates pass. The external attendee WebSocket, external provider CLI and lifecycle
cleanup are still the next dependencies; no real provider was run for this proof.

### External attendee WebSocket checkpoint (2026-09-08)

`/api/room-attendee/ws` accepts only the attendee session bearer in its private
Authorization header. Its connection task subscribes before claiming the exact
network generation, then routes ready/result/disconnect mutations through the room
owner. Started reports use the canonical same-transaction start owner. A committed
room event wakes the socket to load its single canonical assignment; lag reloads
that same authority and adds no assignment queue or polling. A replacement socket
must report ready again and receives the original execution, nonce and input with
`resume=true`. The client must reconcile any uncertain local start before I/O.

The transport uses existing process/principal/room connection and raw-frame budgets,
256 KiB frames, bounded writes, tracked shutdown, exact expiry and five-minute
inbound inactivity. Public room events and parent revocation signals trigger current
connection authorization; all private sends revalidate current custody. Its only
retained delivery state is the last sent execution ID, suppressing repeated delivery
within one socket. Disconnect commits network unavailability without claiming a
provider stop; failure to commit that cleanup is surfaced without private diagnostics.

Real local HTTP/WebSocket verification covers wrong-purpose admission, ready,
human-input delivery, replacement/reconnect with identical execution and input,
retired-socket closure, start retry, result retry and a single public result without
private runtime credentials. The two existing attendee admission/queue integration
checks also pass. This checkpoint does not complete external provider lifecycle,
provider-request relay, CLI, entry packets or managed bridge acceptance.

### External cleanup custody checkpoint (2026-09-08)

Removal/archive cleanup now creates an exact external cleanup ID on the existing
pending runtime-cleanup transition. Host recovery does not observe or stop an
external blocking turn. A separate sealed cleanup authorization proves the original
attendee bearer, current room incarnation and external provider custody, including
after expiry or membership revocation. It grants no ordinary room access. The only
private delivery is the cleanup ID and exact runtime identity; the corresponding
positive stop report atomically uses the existing turn-absence and room-cleanup
owners and writes an immutable report receipt. Replays return that public receipt,
while changed request payloads, cleanup IDs or runtime leases fail before effects.
Readiness cannot cross a pending cleanup fence. An admission that never accepted a
runtime identity needs no fabricated process observation to clear its empty custody.

Both new cleanup cases pass: running/removal with concurrent exact replay and stale
proof rejection, plus idle/export and never-ready removal. All 307 other persistence
cases passed during the affected receipt/transaction-owner regression run; the new
case's incorrect test event name was corrected and both new cases then passed.
All-target/all-feature server and persistence Clippy passes. The measured 18,208-byte
cleanup future is boxed at its new call boundary without changing the lint gate.
Cleanup publication/HTTP and external stop/interrupt controls remain to be connected;
this checkpoint is persistence proof, not external process execution proof.

### External cleanup transport checkpoint (2026-09-08)

The private `/api/room-attendee/cleanup` GET reads only an exact pending stop request;
POST accepts its bounded positive report through the existing attendee room queue.
The session bearer remains purpose-specific, and these handlers use only the sealed
cleanup authority. Revoked membership cannot reconnect its normal socket or publish
turn output. A lost cleanup acknowledgement replays the committed event ID without
republishing another result. This HTTP boundary is the external owner's explicit
post-revocation cleanup path, not a substitute for ordinary room authorization.

The real local socket test readies an external runtime, kicks it through the normal
manager socket, observes closure while provider custody remains active, reports the
exact stop over HTTP, and observes the canonical stopped event. It also checks
wrong-purpose rejection, private/no-store responses, replay identity and no public
runtime-lease disclosure. All four attendee HTTP/queue/socket tests and affected
all-target/all-feature Clippy pass. External CLI and explicit stop/interrupt controls
remain pending; no real provider process was started for this local proof.

### External operator stop checkpoint (2026-09-08)

`agent.stop` now reserves its canonical command and marks external custody pending
without calling a host provider adapter. Its existing cleanup request reaches the
exact attendee socket before turn delivery; that socket closes after delivery and
the positive report uses the post-revocation HTTP boundary. Retry preserves the
same cleanup identity across a changed server runtime generation. This exception
is owned by the exact pending external stop, with the normal reservation hash and
operation checks intact; managed effects still require host reconciliation.

The sealed cleanup report reuses canonical lifecycle reservation validation, exact
turn terminalization and stop finalization in its transaction. Original operator
and cleanup-report receipts commit together, including after room archival. The
stored operator identity only identifies the previously authorized command. No new
room authority, provider process, queue, worker or timer is introduced. Host stop
dispatch is separated from its custody routing; no structure gate was changed.

All 310 persistence tests pass, including exact report/retry, changed-generation
custody and archive completion. The three real local attendee socket cases pass,
including operator stop delivery and recovery of its committed command receipt.
The new socket test initially waited for the wrong response operation; using the
canonical `nack` shape resolved that test failure. Affected all-target/all-feature
Clippy passes. Interrupt/mute effects, provider-request relay, CLI, entry packets
and managed bridge acceptance remain pending; real providers have not run.

### Shared interrupt transaction owner

The canonical interrupt-wait transition and retained-runtime finalization now expose
transaction-local implementations. Managed callers keep their original transaction
ownership; external reports can next combine exact connection authorization and
receipt storage with these same state transitions. The wait result is loaded before
its transaction commits, preserving the exact committed effect against later writes.
No interruption policy, effect table, queue or timer is duplicated. Ten existing
mute/recovery tests and three explicit-interrupt tests pass, together with affected
all-target/all-feature Clippy. External interrupt delivery/reporting remains next.

### External interrupt proof checkpoint (2026-09-08)

Ready reports now bind the external runtime's retained-interrupt capability to its
existing connection record; replacement preserves it and contradictory readiness
fails. Schema 68 follows the existing exact-version rejection contract without data
conversion. Explicit interrupt rejects unsupported external runtimes before effects.
The canonical effect moves from prepared to dispatching when delivered to the exact
connection, with one immutable nonce across reconnect. Delivery does not claim that
provider I/O or quiescence succeeded, and introduces no host claim lease or timer.

A positive retained/gone report compares the complete private delivery, then uses
the shared turn owner and writes its public retry receipt in one transaction.
New proofs require current session and connection authority. Exact receipt recovery
uses the sealed cleanup authority so runtime-gone detachment cannot lose an already
committed result; it does not grant ordinary room reads or result publication.
Connection-ID authorization is reused from its current owner without repeating
session validation inside the transaction.

Seventeen attendee persistence tests pass, including replacement, exact concurrent
retry, retained/gone outcomes, mute, unsupported capability and contradictory ready
reports. After the connection-owner extraction, its three affected cases pass again.
All five attendee HTTP/queue/socket cases pass with the explicit ready field, as does
affected all-target/all-feature Clippy. Server command routing and interrupt transport
are the next consumer; this storage checkpoint is not external CLI acceptance.

### External interrupt command and transport checkpoint

Operator interrupt and mute now return only host-owned effects to the host adapter;
external effects remain in the same canonical table for their attendee socket.
Explicit interrupt checks the external readiness capability at its transaction
owner, while managed preflight retains the actual host driver's exact-turn proof.
The socket delivers one interrupt identity before normal turn delivery and recovers
that same identity on replacement. It does not restart the turn or claim quiescence.

The private interrupt-report HTTP endpoint requires the attendee bearer and current
connection ID for new proofs, routes them through the bounded room queue and returns
the shared cleanup acknowledgement. A gone-runtime report can detach membership and
still recover its exact committed receipt. Public publication retains canonical
event ordering, without private runtime credentials. No provider process, queue or
timer was added; the connection retains only its last delivered interrupt ID.

Actual local socket/HTTP verification passes explicit interrupt and mute with
retained runtimes, runtime-gone detachment, changed connection rejection, report
retry and one public turn completion. The other three attendee socket cases pass.
Ten managed mute/recovery cases, three managed explicit-interrupt cases and the
host pre-slot interruption test pass. Affected all-target/all-feature Clippy and
unchanged architecture/format/diff checks pass. External CLI runtime/tool relay,
entry-packet controls and managed bridge remain pending before phase acceptance.
