# Local Authority, Product Surface, Admission, and Moderation Slice

## Director delegation: owner confirmation (slice 3, required behaviour)

RoomPortal managed/API and attendee/companion calls and connector MCP calls support
`request_delete_text_channel(channel_id, expected_settings_revision, request_id)`;
`start_agent(agent_id, request_id)` remains direct for `meeting_read_only` and requests
confirmation for workspace-write/full-access. `request_create_agent` carries the
owner's own `agent.create` fields except acknowledgement, start intent and session
identity, binds the catalog-selected stopped-agent draft, and executes that same
creation core after approval with the full-access acknowledgement rule.
`request_reconfigure_agent` carries the owner's own `agent.configure` change fields
(model, reasoning, permission level, working folder) plus the catalog revision,
binds the agent's current configuration revision and the resolved resulting profile,
and executes that same configuration core after approval; the AI schema carries no
acknowledgement and no configuration revision.
`request_remove_agent(request_id, agent_id)` carries only the target agent id, binds
that agent's creation identity, and executes the same participant-removal core as the
owner's own kick of an agent (its `room.manage` gate, canonical-owner protection and
existing runtime cleanup) after approval; a missing, replaced or already-removed
target refuses with `target_changed`. Nothing is mutated or reserved before
approval. `request_kick_participant(request_id, participant_id)` carries only the
target participant id, binds the human target's current identity, and executes the
same participant-removal core and the same live-session revocation follow-up as the
owner's own kick of a human (its `room.manage` gate, canonical-owner protection and
host/operator protection) after approval; a missing, replaced, re-admitted or
already-removed target refuses with `target_changed`, and an AI target, the owner,
the host and anyone the owner could not remove are refused at request time.
`request_create_invite(request_id, display_name, invite_scope, ttl_seconds,
max_uses)` carries only the owner's own human-invite fields, runs the owner's own
invite-policy validation at request time and again at approval, binds the normalised
policy, and mints after approval through the same `create_human_invite_for_manager`
core under the owner's own `RoomManagerAuthority` taken from the recorded approving
session and re-resolved inside the mint transaction — the desktop's own local
session (local-bootstrap lineage, recognised by its local socket provenance, never
by participant id alone) or the exact central-owner operator session — never from
delegate or owner provenance alone; an `Operator` owner reference that is not
central-owner cannot become that authority, and a recorded approver whose session no
longer re-resolves mints nothing. Approving an invite additionally requires the
deciding session itself to hold that authority right now: any other deciding session
(including an ordinary paired operator browser resolving to the same local-operator
participant) gets `permission_denied` for approval while decline stays allowed, and
nothing is minted. The decision transaction pins the exact approving session
(fingerprint/device/origin), its liveness, central-owner provenance and current
manager binding; current-origin equality, secure-channel presentation and
owner-lease liveness remain enforced where that state lives (socket ticket exchange
and per-command session revalidation, as for every other room command); a human
guest/member owner, a companion delegate or any owner whose provenance cannot be
re-resolved as that authority is refused with `owner_authority_unavailable` at
request time and never queued. The link, token and join code reach only the bound
owner and only in the live approval acknowledgement, exactly once; they never enter
a room event, the stored request, receipt, command replay, log or AI result, and the
AI learns only that an invite was created. Companion, attendee and connector invite
creation is not delegable. Role assignment and provider approvals stay unreachable.

The host persists a typed immutable request with normalised parameters and resolved
defaults, room incarnation, exact owner provenance, delegate/session/execution binding,
request UUID, target revisions, creation, expiry and state. Five-minute expiry is
shortened by either authority's deadline. Exact retries return the same request/current
state/receipt; changed payload or decision conflicts. No hashes beyond existing command
admission conventions. Pending requests are capped per owner/room and rate-limited;
creation debits existing write/admission budgets of both identities, with no retry debit.

Only the exact authenticated human owner receives requested/updated events and pending
snapshot entries. The stored action, current target name, AI and owner identities,
consequences and expiry are the decision surface. Channel deletion uses the settings
owner: its message text is blanked/deleted, pins and search records removed and channel
ID retired; event sequence/actor records are retained. Do not promise physical erasure.
The approval rebuilds the channel list exactly as the owner's own settings update
does: the removed entry is dropped and the survivors keep list order with dense
positions, so deleting a channel that is not last still validates.
Only that human may approve/decline; another operator, guest or AI cannot substitute.
Approving a full-access agent start requires that human's explicit full-access
acknowledgement inside the approval; without it approval is refused and the request
stays pending.
One transaction claims and validates the immutable decision, both current authorities,
provenance and unchanged targets (`target_changed`), then uses the same human mutation
core or reserves the existing lifecycle operation. Concurrent approvals execute once;
exact decisions replay receipts. Declined, expired, cancelled and failed are terminal;
uncertain external effects remain executing and reconcile through existing lifecycle
custody, never blind replay. Completion events name the AI, on-behalf-of owner and
confirmation ID; pending/declined details remain owner-private. AI retries receive only
status/receipt, without private owner request data or secrets. The existing
`read_room_status`/`room_status` observation includes up to 16 correlated outcomes
for that AI alone; ordinary room context does not render arbitrary event payloads.

Pending requests from every surface (managed Portal, attendee, connector) are not tied
to the AI's turn or connection: the AI ending its turn, an attendee connection closing or
a connector reconnecting never cancels them. A request is created by the delegate's live
call and then belongs to the owner's decision. It ends only by the exact owner's decision,
its expiry, or loss of current authority. Current authority is re-proved at approval, at
an AI retry, and in every room-input reconciliation: the delegate participant is still
joined, unmuted, an AI, holds `director` and is bound to the same owner; the owner's bound
authority still resolves; and the delegate's session custody is live (provider session
server-owned, attendee session unrevoked, connector session live). A provider delegate
whose runtime ended (participant `detached`, membership intact, still director/unmuted/bound)
keeps its pending requests across that churn — detach is runtime absence, not removal —
until a later live turn, restart or re-add re-attaches it; live tool calls still require
the joined participant with exact turn authority. Turn generation,
execution, input cursor, runtime lease and connection are not required at that point.
Removal, kick, leave, mute or role change of the delegate, or loss of the owner, ends the
request as `cancelled` with `confirmation_cancelled`; nothing executes. A cancellation
receipt keeps `code: "confirmation_cancelled"` and records the underlying rejection as
`cause: {"code", "message"}` so a later cancellation explains itself. An AI retry of the
same request ID from a later turn or connection of the same delegate identity returns the
stored state or receipt; a different action or payload conflicts. Connector requests keep
these rules unchanged. Do not invent a connector execution protocol. Persist non-bearer
references only; no session swapping.
New schema/table follows the normal version migration and archive catalog. Requests
and their private events are ephemeral and excluded from backups/server moves.

Frontend work is restricted to generated wire types, socket validation and projection;
views/styles/app are owned elsewhere. Expose pending owner requests, typed decision
and ACK. An approved action that ends `failed` leaves a dismissible one-line Korean
result where the card was (the action-specific failure line plus the mapped reason
when the receipt code is known, else the neutral line); declined, expired and
cancelled stay quiet. Acceptance: non-mutation, exact owner-only approval/delivery/reconnect,
payload/target binding, concurrent exactly-once, authority revocation, explicit decline/
expiry/authority-loss cancellation (not turn end), archive exclusion and both actions across all three surfaces.
Run affected crate tests, fmt, architecture-check, generated TS checks and affected
Vitest; server integration uses `--test-threads=2`. This is acceptance, not completion.

## Round-4 delegated event attribution (2026-10-12)

Channel create/rename, mute/unmute, pin and agent start/stop on managed provider,
attendee and connector transports name the AI executor in top-level `actor` and
`actor_id` with type `agent`; the bound human authorizer remains in
`on_behalf_of_participant_id`. Apply this to every event committed by the action,
including private cancellation, pending/error events and restart recovery. Preserve
target participant/session fields and recipient visibility. Direct human events
keep their existing actor. The event producer owns attribution; projection,
archive allowlists/provenance and generated frontend event contracts must preserve
these existing fields without inventing a human executor. The wire `RoomEvent`
flattens `extra` (server `#[serde(flatten)]`), so the timeline projection reads the
flat attribution keys (`action`, `actor_participant_id`,
`on_behalf_of_participant_id`, `target_id`, plus `request_id`/`confirmation_id`/
`pinned`/`target_channel_id` when present) from the same event object the
snapshot/history load and live socket frames deliver; the server `content` stays
English. Verify live transport receipts/publication and recovered durable events,
including failed recovery.
Frontend views remain unchanged; report any changed displayed strings.

## Delegated direct runtime, module responsibilities (2026-10-16)

Behaviour contract (unchanged by the size refactor): every delegated direct
action, on portal/attendee calls, connector MCP calls and the room-actor
mutation queue, goes through `room_runtime::delegated_direct`
(`crates/agentsassemble-server/src/delegated_direct_runtime/`). The request
branches (`request/`) run one owner-confirmed request per branch: current
in-flight admission, a store call charged with the shared owner+delegate
delegated-charge identity, debit resolution, durable publication only on
success and the exact same reply. The create/reconfigure branches additionally
complete an approved-but-undriven confirmation idempotently and return the
recorded receipt. `apply.rs` owns the non-confirmed direct execution path
(lifecycle start/stop and atomic mutations) and its publication.
`drive.rs` owns approved execution drives: catalog re-validation with
`target_changed` refusal drives (create/reconfigure) and the post-decision
confirmed drives (create/reconfigure/start) with their unchanged guard,
committed-event extension and uncertain-effect warning logs. Results, error
codes, receipts, events, publication order, budget charging and logging stay
exactly as recorded above for every entry point: `execute_delegated_direct`,
`provider_room_tool_runtime::handle_provider_room_tool`, the room mutation
queue, and the post-decision drives called from the room command handler.

## Round-4 delegated mute correction (2026-10-12)

Provider, attendee and connector director calls must refuse `muted=true` when
the target is their exact bound human owner, before canonical mutation or budget
debit. Delegated unmute remains allowed under the existing current-authority
checks; human direct mute/unmute is unchanged. Verify refusal leaves the owner
unmuted and the same director can still unmute.

## Director delegation: remaining direct actions (slice 2)

Continuation acceptance: finish lifecycle start/stop, all three transport surfaces,
fresh action-specific discovery, shared owner/delegate budgets, and revocation/replay
regressions. Build on channel/mute persistence in `3a9bf65a` and retain all three
reachable transports. Runtime execution must retain mute interruption and ordered
turn progression through the existing room actor. Lifecycle admission, pre-effect
authorization and durable completion must retain exact delegated provenance.
Slow lifecycle effects return `in_progress` only after the lifecycle owner commits
its reservation, with the exact operation ID. The room actor continues execution
and publishes completion; portal request deadlines remain unchanged.

Required behavior: extend the existing pin delegation owner with
`create_text_channel(name, request_id)`,
`rename_text_channel(channel_id, name, expected_settings_revision, request_id)`,
`set_participant_mute(participant_id, muted, request_id)`, and
`start_agent(agent_id, request_id)` / `stop_agent(agent_id, request_id)`.
These operator actions require the exact bound owner's current corresponding
capability, otherwise `owner_not_permitted`; director never grants operator rights.
Start accepts only `meeting_read_only`; workspace-write/full-access targets return
`owner_confirmation_required` without mutation or provider reservation. Confirmation,
channel deletion, agent creation/configuration/removal, invites and roles stay excluded.

Every call revalidates owner provenance and current delegate custody in the mutation
transaction. Preserve immediate owner mute/removal/read-only/disconnect and delegate
role/mute revocation, dual principal budgets, full-payload/room/action/owner/delegate
replay binding, exact retries and fail-closed restored/re-created identities.
Channels construct only the requested delta against current settings and invoke the
settings owner, retaining count/name/revision/retirement checks. Mute shares the
canonical target restrictions and exact execution interruption. Start/stop share
preparation, effect authorization, completion and recovery with human requests;
completed attribution must survive recovery without duplicate public lines.

Affected entry points: managed/API RoomPortal, attendee/companion RoomPortal and
connector MCP/HTTP; portal lists each currently permitted action, connector lists
statically and reports fresh `delegated_actions` through room_join/room_status.
Discovery is advisory. Canonical action events and room-visible completion attribution
commit together; lifecycle acceptance is distinct from completion. No frontend UI.
Verification: affected crate tests, cargo fmt, architecture-check; cover each action's
success/attribution and immediate revocation, guest ceilings, channel settings isolation,
unsafe-start non-mutation, cross-room targets, replay conflicts and shared owner budget.
The continuation authorizes as many independently buildable commits as needed, each
under 1,000 changed lines; do not push. This section records acceptance, not
implementation completion.


## Director delegation: pin slice

Human roles remain non-authorizing. An AI `director` delegates a fixed action list
within its human owner's current authority; this slice implements only pin/unpin.
No operator rights, role assignment, CapabilitySet change, other delegated action,
confirmation flow or frontend UI is included. Authority is proved inside the pin
mutation transaction: joined/unmuted AI director with exact turn/connection/session
custody, and its bound human owner with current writable, joined/unmuted authority.
Persist only server-owned non-bearer provenance; never substitute another session
of the same person or promote remote custody to local bootstrap custody.
Missing historical bindings fail `owner_authority_unavailable`. Disconnect ends
delegation when it ends the bound authority (secure-channel close, owner release,
expiry or revocation). Only local host authority needs no live socket; there is no
new durable authorization lifetime. Portal discovery and every call recheck authority.
Connector discovery is static; its join/status report current role and delegated
actions, as specified in external-ai-admission-slice.md.
Both owner and delegate pay the existing write/admission budgets. Exact request-ID
replay has one mutation and one room-visible attribution event; changed input conflicts.
Acceptance covers human non-escalation, guest ceilings, mute/removal/read-only/demotion
on the next call, wrong-session rejection, missing binding, disconnect and exact replay.
Verify affected crate tests, cargo fmt and architecture-check in three buildable
commits: provenance/schema catalog, shared mutation/admission, then tool wiring.


Status: implementation evidence retained; D-01 closed at `a7949bd` and D-02
closed by the bounded JSON socket cutover

Comparison baseline: original
`d5046473010d1353a81ee38337360e6d98f7bd6f`; Rust
`938a088`.

Audit correction at Rust baseline `8a5f75a`: the finite snapshot/catch-up,
one-use socket ticket, event sequence, request-ID deduplication, uncertain-ACK
replay, bounds, and product-surface equality remain required. D-01 removed the
uncalled HTTP host challenge/startup secret. Remote proof, local receipt,
and per-frame authentication are also removed from the approved target unless separate
controlled packaged reproductions, or equivalent concrete topology evidence, first
defeat the smaller native-child/one-use-ticket boundary and then establish an in-scope
active relay. Separate private-control and loopback paths alone are not evidence;
base64, repeated key derivation, and permissions digest are not retained. Historical
evidence below does not approve broader layers.

Historical implementation checkpoint, 2026-08-25: immutable local authority/zero-room
bootstrap, server-wide human profile, canonical directory/create-room flow, and
derived server/native product surfaces are published. The historically proof-bound finite
subscription section is implemented in the current candidate with strict
one-use ticket credentials, exact-byte Snapshot binding, a transactional bounded
`(C,H]` reader, client high-water readiness, and no string-ticket/non-desktop
fallback. Process-wide connection leases, pre-parse raw scopes, permanent fresh-human
mutation debit, a separate in-flight owner, and canonical participant roles are
published. The participant-role socket-boundary correction is published and has passed
both exact-diff web review and Daybreaker manual security review. The
mute/provider-custody implementation is now present in the local candidate,
including exact execution/effect persistence, provider-neutral turn control,
provider-specific interruption, restart reconciliation, and the copied frontend
command/event path. It remains active rather than completion evidence until the
candidate is pushed, packaged real-client/provider verification is cleaned up,
and both post-implementation reviews approve the exact public diff.

## Response validation cleanup (2026-10-05)

User-required behavior: directory, room creation, room-session admission and socket
subscription accept unknown response fields while retaining required fields, runtime
types, authority/room/participant identity, revision and cursor checks. Remote-host
JSON remains untrusted; generated TypeScript types do not validate it at runtime.
Daybreak Finding 1 correction: restore local surface digest recomputation before
all directory/session bindings, including remote hosts. The digest must identify
known surface content for the lifetime pin and admission verification cache; a
replacement B with A's revision/digest must fail before binding or session storage.
Keep native bootstrap comparison, exactKeys relaxation and receipt digest cleanup.
Admission stores its verified key only after binding succeeds; all replacement
join/pair/recovery/owner sessions must pass that binding before publication.
Verify A then forged B through binding and admission, retaining stale cancellation.
No auth/ticket changes.
The old frontend requires digest in directory/session surfaces and subscriptions;
removing server fields now would break cached clients. This is expand only: retain
Rust producers, protocol/generated fields and compatibility tests. Later contract
work must account for native bootstrap and admission cache consumers before removal.
The ten directory exact-key calls and subscription exact-key function/call are
replaced with existing known-field checks; other validators are outside this change.
No new abstraction, fallback, storage change or deployment. Preserve stale-result
cancellation and existing failure/retry paths. Verify affected frontend tests and
one pre-push make verify; no packaged/manual verification or signed build.

## Definition

This slice replaces the fixture-shaped bootstrap with a restartable local
authority, derives every reachable product surface from its real owner, makes
WebSocket readiness finite and gap-free, centralizes pre-parse admission,
and completes canonical participant role/mute behavior.

The slice is intentionally larger than one screen. The frontend must not compose
bootstrap, room, stream, role, or mute behavior until the server or native host
that owns it carries the same authority through the real flow. Placeholder rooms,
hard-coded profiles, compatibility readers, fake capabilities, client-owned
orchestration, and best-effort side effects are forbidden.

## Local bootstrap authority

Schema installation creates infrastructure only. It does not create a room,
participant, or user profile. A separate bootstrap sidecar records immutable
`authority_lineage_id`, `Empty | Initializing | Complete`, owning request ID,
schema revision, creation metadata, selected canonical user/profile IDs, and a
digest of the immutable initialization contract.

`Complete` is verified from lineage, required canonical rows, and reference
ownership. It is not a comparison against mutable initial display values, so a
normal later profile edit cannot be overwritten by bootstrap retry.
The immutable digest uses a versioned length-delimited encoding of schema
revision, server/lineage/request/user/participant IDs, canonical marker creation
and completion metadata, and every field of the initial profile. Mutable current
profile revisions are not reinterpreted as bootstrap input.

Bootstrap runs under `BEGIN IMMEDIATE`. Marker and product rows transition in one
writer transaction. An exact request replay returns its stored result; a different
concurrent request receives a typed conflict. Startup recovery validates only a
positive allowlist of bootstrap-owned rows and resolves interrupted initialization
to `Complete` or explicit `repair_required`. It never seeds, repairs, or
reinterprets data through a fallback.
`Empty` additionally requires every product table in the schema owner's inventory
to contain no rows. One declarative table descriptor owns installation and product
classification; the gate installs those declarations into SQLite and compares the
actual `sqlite_master` table set to the same descriptors. A partial room, event,
reservation, budget, result, attachment, profile, or session therefore cannot be
omitted by a second hand-maintained inventory or absorbed into a new authority.

Room and server-operator tickets require a verified `Complete` marker. `Complete`
with zero rooms is normal. The real room directory and create/join flow owns the
first room; startup does not fabricate `general`. Tauri owns its local bootstrap
controller and `HostProductSurface`. The socket validated for bootstrap is the
same canonical authority socket handed to Core, not a probe closed before a second
owner is opened.

The directory response carries both `server_id` and `authority_lineage_id` and
is closed-schema validated before zero rooms can be accepted. Startup binds that
authority for the webview lifetime. Every later list and create response must match
that immutable pin; desktop independently compares the same response with a fresh
native bootstrap grant, but a matching replacement grant can never rebind or
overwrite the existing webview pin. Room creation binds one UI-owned UUID request
ID and canonical payload hash to the server operator in the same transaction as
room/settings/membership/event state. The same writer transaction revalidates the
complete bootstrap digest before replay or mutation, and an ambiguous HTTP result
replays the same request rather than creating a new intent. Only exact replay
succeeds; room creation never doubles as rename, reopen, or membership restoration.

The canonical local human profile is server-wide and therefore remains readable,
editable, and avatar-capable before the first room exists. A fresh one-use
private-control-derived operator credential reaches that profile without
inventing a room membership. Room-session credentials retain the same profile
surface for admitted participants. The route consumes either credential exactly
once and selects only its matching profile authority; a server-operator credential
cannot authenticate a room socket. Profile projection may update only the human
display/avatar fields in existing memberships and never owns room role, join,
mute, permission, or Agent Session profile state.

### Central server registration custody

The currently composed desktop startup flow registers the exact local server after
central person authentication. First creation writes one Ed25519 PKCS#8 private key
exactly once to the owner-only `central-directory/host-ed25519.pk8` file beside the
database authority. Before key creation, SQLite commits one fresh UUID initialization
nonce to its singleton host-initialization marker. The key file is an exact versioned
envelope containing that same nonce and the base64url PKCS#8 bytes. The subsequent fresh
initialization transaction stores `server_id` and the matching raw 32-byte public key in
SQLite; it never stores the private key. Only a marker-only database and a key envelope
with the exact same nonce can resume an interrupted initialization. An initialized
database never creates a missing replacement, and a brand-new database path refuses any
pre-existing orphaned key before creating the database. A stale key restored after that
check still has a different durable nonce and remains rejected on every later open
rather than turning the marker-only database into a retry authority. A database-only
backup therefore cannot clone signing authority, while a missing, orphaned, malformed,
over-permissive, hard-linked, symlinked, nonce-mismatched, or public-key-mismatched key
fails closed. Older schema and raw-key formats remain rejected rather than migrated or
read through a compatibility path.

The public projection is exactly
`{"crv":"Ed25519","ext":true,"key_ops":["verify"],"kty":"OKP","x":"..."}`.
Those keys are encoded in lexical order as compact UTF-8 JSON with no trailing newline;
`x` is the raw 32-byte Ed25519 public key encoded base64url without padding. The
fingerprint is base64url-without-padding SHA-256 over those exact JSON bytes.
Registration signs the exact UTF-8 transcript
`AA-HOST-REGISTER-1\n{server_id}\n{owner_person_id}\n{issued_at}\n{nonce}` with no
trailing newline. `nonce` is exactly 18 OS-random bytes encoded base64url without
padding, `issued_at` is whole Unix seconds, and `signature` is the raw 64-byte Ed25519
signature encoded base64url without padding. Private PKCS#8 material is never serialized
into an HTTP response, log, error, test fixture, or frontend state.

`POST /api/central-directory/registration-proof` is mounted and advertised only by the
desktop-native server composition. It is admitted only by a fresh one-use
central-registration bearer obtained through the private desktop control pipe; generic
server-operator, profile, and room tickets cannot authenticate it. The route consumes
that credential before parsing a bounded exact JSON object containing only
`owner_person_id`, validates the central directory's ASCII identifier grammar and
length, and returns the public JWK, its fingerprint, and one signed proof. The Tauri
bridge binds issuance to this exact POST path and omits the original browser-only
device-token header because native control-pipe custody is the actual local-operator
authority; broad CORS headers and parallel browser authentication are not introduced.
The private control grant also carries the expected server ID, raw public-key `x`, and
canonical fingerprint. Before forwarding anything to the central directory, the
desktop frontend rejects unknown response fields, non-canonical base64url values,
binding mismatches, a non-matching JWK fingerprint, the wrong owner, or an Ed25519
signature that does not verify over the exact registration transcript with that pinned
native key. Trusting a loopback response merely because it is self-signed is forbidden.

This increment does not advertise the original public `GET /api/server-info` or remote
`POST /api/server-info/challenge` until their public-invite/admission owner is complete.
It also does not claim the non-desktop startup path, whose local bootstrap authority is
not implemented. Those paths remain absent rather than returning placeholder identity,
borrowing the desktop credential, or accepting a device token the Rust runtime cannot
authenticate. Acceptance for this increment is a fresh packaged desktop central guest
flow that persists one stable server registration, reaches the strict zero-room
directory, survives restart with the same server ID/key fingerprint, rejects ticket
replay and malformed identifiers, and never emits secret material.

## Product surfaces and strict protocol

`ServerProductSurface` is generated from the actual HTTP router and WebSocket
action/stream registries. `GET /api/rooms` projects that same surface revision and
digest. `HostProductSurface` is generated from the intersection of registered
Tauri commands and their permission capabilities. WebSocket never advertises
native-host commands.

The frontend composes in identity, authority, then product-surface order. A hook,
effect, stream, or control absent from its owner surface is not mounted. Copied
source and reachable composition are reported separately in
`docs/FRONTEND_BACKEND_GAPS.md`.

Subscribe, stream, action, and action-payload decoding is strict and typed.
Unknown fields, streams, or actions fail with a typed protocol result. Canonical
`message.send` is content-only. The roster comes only from authenticated
WebSocket snapshot/events; an HTTP participant merge is not another authority.
When room membership becomes Joined, the room transaction includes its complete
canonical public participant in `participant_joined`. The strict event contract
upserts that projection directly, while role, mute, status, and ownership remain
room-owned rather than being synthesized from Agent Session metadata.

Participant roles are exactly the five currently reachable product values:
`human`, `director`, `implementer`, `reviewer`, and `agent`. The Rust domain uses
one typed enum with those exact snake-case wire values. Historical aliases and
normalizing fallbacks are not part of the Rust contract. Role is room-owned
collaboration/routing metadata with the AI-only delegation exception above, never
operator authorization; human profiles,
Agent Sessions, and presentation inference cannot overwrite it.

`participant.role.update` requires freshly resolved `room.manage` authority and
atomically commits the complete participant projection, one
`participant_updated` event, and the replay result. The copied desktop and mobile
member surfaces consume that canonical participant field directly. Agent-name
inference is presentation-only and may run only when no room participant exists.
Participant kind and ownership remain independent: role changes cannot move a human
into an Agent Session group or turn an Agent Session into a person, and a locally
created Agent Session records the authenticated owning participant ID used by the
roster.

## Connection, raw ingress, and mutation admission

Before HTTP 101, one process-wide owner computes and acquires global, principal,
and room connection leases atomically. Rejection charges no scope. Only active
principal and room keys are retained, so their cardinality cannot exceed the
global connection ceiling; generation-bearing leases prevent an old release from
freeing a replacement key.

The WebSocket codec owns hard byte bounds for frame, reassembled message, and
write buffer memory. This contract does not claim a fragment-count or rate limit
unless a separate codec-owned implementation is added.

Before JSON parsing or room queueing, the process-wide socket admission owner
atomically charges global, principal, and room byte/rate scopes. The debit is
permanent and includes unknown or unsupported raw frames. The room actor does not
retain a sharded transport charge. Keyed windows are bounded and reject new scope
keys when expired-window pruning cannot free capacity. Such a capacity rejection
still charges the global window and every already-tracked applicable keyed window;
it never inserts an untracked key, so repeated rejected reassemblies cannot bypass
the bounded process owner.

After strict decoding, implemented-action classification, and exact replay or
prepared-resume detection, a fresh human mutation receives a permanent
process-wide principal debit. Permission denial, validation failure, missing
targets, conflict, room busy, timeout, disconnect, cancellation, SQLite rollback,
and provider failure do not refund it. Every fresh action, including a stop that
owns runtime cleanup, receives this process debit; only an exact committed result
or nonterminal lifecycle resume avoids a second debit. A terminal rejected
lifecycle reservation remains the exact durable rejection owner but receives a
new process debit on every replay because the preceding outcome was definitive.
The retry ledger stores a fixed-size,
domain-separated identity digest instead of request strings and has both
principal-count and total-mutation memory ceilings. Only a separate in-flight
permit is RAII-released, and that permit moves
with the queued room command. Durable room budget remains in the command
transaction and rolls back with it. Ambiguous commit outcome is resolved from
request ID and idempotency record. Human-principal and Agent Session RoomPortal
mutation governors remain separate. The RoomPortal budget key is the server-owned
Agent Session ID, never a provider-selected conversation identity. A committed or
definitively rejected actor outcome closes its in-memory retry exemption without
refunding the original debit; only an unresolved exact intent can reuse that debit.
Durable room admission still recognizes the stored terminal rejection before
reserving another room-budget slot.

Once a client command has crossed the WebSocket send boundary, loss of its ACK is
an unknown outcome, never an ordinary timeout that frees its request ID. The
client retains the exact serialized request ID, action, and payload, closes that
connection, and replays those same bytes over a fresh authenticated channel until
the durable idempotency result is recovered. Only a command that never crossed
the send boundary may expire as an ordinary timeout. Explicit client shutdown
reports `outcome_unknown` for an unresolved sent command instead of authorizing
a new-ID retry. ACK/NACK frames carry a required server-owned resolution of
`committed`, `rejected`, or `unresolved`. Only a committed ACK or a definitive
rejected NACK may retire the private request identity. Queue saturation, lost
room-owner reply, principal/persistence failure before authoritative replay, and
any unclassified server failure are unresolved; the browser closes and replays
the same serialized command JSON. Missing or invalid resolution fails closed
the same way rather than falling back to error-code inference. Repeated
unresolved replies use a bounded per-request exponential delay that a successful
reconnect cannot reset.

Each action owner classifies resolution from its execution phase. Transactional
validation/authorization/state failure is definitive only when that transaction
cannot have committed. After a durable create/lifecycle prepare, an uncertain or
applied provider effect, or a post-commit publication/completion boundary, a
nonterminal failure is unresolved regardless of its generic error variant. Only
an explicitly committed terminal provider failure may be rejected. Its exact
public rejection remains durable and replayable without another effect, event,
or write-budget debit. An unconfirmed lifecycle effect stays unresolved and is not
executed again until authoritative runtime reconciliation changes its state. Exact live
replay may perform one bounded provider observation: only proven absence or exact
owned-runtime adoption can reopen the original request's effect path, while uncertainty
remains unresolved and every replacement request stays blocked. After a server restart,
the reservation's immutable server-runtime generation prevents the old request from
entering the live-effect path. Proven absence terminally rejects an old start/create-start
request while retaining its Agent Session for a new lifecycle request; a proven or
previously confirmed stop commits the old stop result. Provider attachment is a separate
effect: a process observation cannot reopen a provider-session creation whose response was
lost unless the driver retains exact retry authority. A response for a request ID the
browser does not currently own is also a protocol failure;
it closes the channel rather than being ignored.

## Finite subscription decision after D-02

The target retains the canonical receiver/barrier, exact Snapshot cursor `C`, finite
high-water `H`, contiguous durable catch-up, one establishment deadline, generated
strict frame schemas, size limits, event sequence, request-ID deduplication, and
uncertain-ACK replay. One-use ticket plus origin/TLS admission establishes the
connection principal. Remote transport uses ordinary bounded JSON with no second
proof from the same ingress authority. Local transport also uses ordinary bounded
JSON unless a controlled packaged endpoint-substitution reproduction names an
in-scope actor and demonstrates why exact child liveness/identity plus the fresh
issuer-runtime-local ticket and separate ingress checks are insufficient. Only that evidence may restore one minimal
receipt. A separate active-relay reproduction is required before the Snapshot and
every later bidirectional product frame may authenticate raw UTF-8 bytes, direction,
and counter with one cached key. No base64 envelope, repeated key derivation, Snapshot
digest, or permissions digest remains.

## Historical proof-bound subscription (superseded by D-02)

The server acquires the canonical receiver/barrier before snapshot construction.
It pre-serializes the exact final Snapshot UTF-8 frame at cursor `C`, then fixes a
finite durable publication high-water `H >= C`.

The versioned, length-delimited `Subscribed` proof transcript binds context,
server challenge, ticket/connection nonce, room/principal/participant IDs,
protocol version, sorted accepted streams, server-surface revision and digest,
canonical permissions or their exact digest, `C`, `H`, and the exact final
Snapshot bytes digest. Proof appears only in `Subscribed`, avoiding recursion.

That receipt establishes a connection-specific authenticated channel. A distinct
frame key is derived from the ticket proof key and connection nonce. After the
plain, receipt-bound Snapshot, every server catch-up/live/event/ACK/NACK/catalog/
resync/pong frame and every client command/ping frame uses one strict envelope.
Its HMAC binds a versioned length-delimited context, connection nonce, direction,
an independently contiguous counter starting at one, and the exact decoded inner
JSON UTF-8 bytes. Base64 is canonical, inner product frames remain bounded at
256 KiB, and the authenticated wire envelope remains bounded at 384 KiB. A
counter replay/gap, direction reflection, noncanonical payload, or proof failure
is rejected before projection or command execution and closes the connection.

`Subscribed` and Snapshot are encoded and size-checked before either is sent.
The server then delivers `C+1..H` contiguously. Receiver lag or a gap is refilled
from durable `(C,H]`; an unresolved gap or overflow resynchronizes/closes and
never reports readiness.

The client verifies proof and snapshot digest, then verifies every authenticated
catch-up frame before projection, and becomes ready only when
`delivered_seq == H`. `H == C` becomes ready immediately after Snapshot; later
events are normal live delivery. Connection generation is rechecked after every
asynchronous cryptographic operation and before projection or readiness, so an
old socket cannot mutate the successor connection's state. One absolute deadline
covers strict Subscribe,
principal/surface lookup, barrier, snapshot, high-water, encoding, sending,
catch-up, and readiness. Traffic cannot extend it.

## Participant role and mute authority

A role update validates permission, target, and exact enum, then commits
participant state, event, result, and replay atomically.

`participant.mute` uses its independent capability, not `room.manage`. Its strict
payload contains exactly `participant_id` and `muted`. Both human and Agent
participants are valid targets. The room-owned `Participant.muted` field is the
canonical mute authority; a human profile or Agent Session profile cannot override
it. Human mute has no provider effect. An Agent effect is selected only from the
canonical participant type and the exact room-scoped Agent Session, never from a
role label.

The command transaction reauthorizes the current principal and target membership,
binds `(room, principal, request_id, action, payload_hash)`, and atomically commits
the participant state, public event, result/replay, and, for an active Agent turn,
one prepared interrupt effect. A repeated request replays that result and cannot
create another effect. Mute excludes future floor assignment and the existing
canonical message and room-random ingress reject a muted human or Agent. It is not
a blanket ban on room administration.

Mute never stops or detaches a persistent runtime or Agent Session. If the target
Agent is busy, only the exact current turn is interrupted. Unmute never guesses that
the turn has stopped and cannot authorize a replacement generation while an
execution or effect remains blocking.

### Durable execution and scheduling authority

Only a new active-turn assignment increments `turn_generation`. Assignment commits
the session's active turn, inflight input ownership, and one
`ProviderTurnExecution` in the same transaction. The execution identity is
`(room_id, session_id, turn_generation)` and every Agent Session foreign key uses
the room-scoped `(room_id, session_id)` key. Generation is monotonic within that
session. Active `turn_id` and generation are immutable.

Execution phases are `Assigned`, `StartDispatching`, `Running`,
`InterruptPending`, `Quiescing`, `StartAmbiguous`, `InterruptAmbiguous`,
`RecoveryRequired`, and terminal phases. A partial unique index permits at most one
blocking execution per `(room_id, session_id)` and includes every listed
nonterminal or quarantine phase. A second blocking execution for the same immutable
runtime launch handle is also forbidden, even across owner transfer.

`StartAmbiguous`, `InterruptAmbiguous`, and `RecoveryRequired` are blocking
quarantine, not terminal success. Their execution retains its inflight inputs,
does not copy them to pending, keeps `requeue_finalized=false`, and blocks new
assignment and runtime reuse. Timeout, task death, or uncertain transport alone
cannot terminalize or requeue work.

`schedule_requested` belongs to the exact room-scoped durable Agent Session, not
to an execution. Human participants have no scheduling row or synthetic execution.
Agent unmute commits `muted=false` and sets the flag. The shared scheduler consumes
it only when the same transaction either creates a real assignment or proves that
no pending input exists. An unresolved execution, ordered-floor ownership, or
current mute preserves the flag. The quiescence finalizer and an execution-free
unmute use the same scheduler owner, so early unmute, remute, terminalization while
muted, and later unmute cannot lose a wake.

### Provider-start and interrupt serialization

The turn task is the sole owner of provider I/O. It exposes a cloneable exact-turn
control handle rather than sharing a driver mutex across network, PTY, database,
queue, or task-join awaits. Registry and slot locks are held only while copying or
transitioning small authority values; database transactions, registry locks, and
slot locks are never nested.

Before provider start, the owner installs `ActiveProviderTurnSlot::Preparing`. The
durable CAS then verifies the exact room/session/participant/turn/generation and
current `muted=false`, consumes the unique start authorization, and records
`StartDispatching` plus a unique dispatch nonce before any provider I/O.
`begin_exact_turn(operation_id)` atomically returns either `NotStarted` or
`Started { control, completion }`.

If mute commits first, start authorization fails and provider call count remains
zero. If start authorization commits first, mute reserves the exact interrupt and
joins the Preparing handshake. It may use no provider control after proved
`NotStarted`; only `Started` can expose exact control and its completion latch.
Cancelling the waiter cannot cancel the handshake owner or orphan an unrecorded
external start. SQLite commit order is authority; another read immediately before
I/O is not a substitute.

`ProviderEffect` phases are `Prepared`, `Claimed`, `Dispatching`,
`IssuedWaitingQuiescence`, and `Finalized`, with explicit ambiguous/quarantine
outcomes. Claiming performs no provider I/O. `Dispatching` is durably recorded
immediately before the first non-idempotent interrupt byte or request. An expired
claim may be reacquired only where the provider replay class proves safety;
`Dispatching` is never blindly replayed. Duplicate mute for one generation shares
one durable effect owner and one physical interrupt at most.

Provider control remains behind the common exact-turn contract while replay and
quiescence stay in small provider modules:

- Codex uses exact provider turn identity and the official turn interrupt. The
  `turn/start` response is cancellation-shielded until `{threadId, turnId}` is
  captured. A crash after request bytes but before that identity never resends
  start blindly.
- OpenCode uses exclusive current-turn session custody and observe-before-retry.
  An unresolved `/abort` blocks session reuse and is never blindly replayed.
- Other retained drivers use their correlated native receipt through this same
  control owner. Unsupported retained interruption is unavailable; terminal silence
  and process replacement cannot supply that receipt. The external Room Connector
  flow has its separate Phase 7 authority and no managed-session control contract.

The live room owner retains the exact execution and dispatch nonce for every spawned
provider task. A task that exits without a typed result is not log-only: proved
pre-I/O `NotStarted` takes the safe failure path; death after start dispatch records
`StartAmbiguous` or `RecoveryRequired`; death after interrupt dispatch records
`InterruptAmbiguous` or `RecoveryRequired`. Post-I/O task death retains inflight
ownership and requeues zero inputs.

When the claimant has ended, `record_provider_turn_task_death` (claimed,
dispatching or issued), `mark_provider_interrupt_ambiguous`, and
`mark_provider_interrupt_recovery_required` must clear `claim_owner` and
`claim_expires_at` in the same CAS transaction that preserves the ambiguous or
recovery phase. This hands custody to live reconciliation or startup recovery
immediately, without waiting for the dead owner's 30-second lease. Existing exact
execution/runtime/dispatch fences and rejection of competing live claims remain.
Regression acceptance: each transition allows a different recovery UUID immediately
and after reopening the store, preserves effect identity/phase, and rejects a
second claimant. No provider replay, schema, transport or timer changes are needed.

### Terminal finalizers and restart custody

Every ordinary success, decline, tool, and terminal-publication transaction CASes
the exact room/session/participant/turn/generation, complete current H/O/T custody,
current `muted=false`, and absence of an unresolved effect. It cannot publish or
clear authority through an interrupted or stale generation.

`ExactTurnQuiescedRuntimeRetained` is available only after the send owner, provider
transport, new RoomPortal admission, committing tool reservations, exact slot, and
completion latch are quiescent. Its single UOW records `Interrupted`, finalizes the
effect, emits exactly one interrupted `turn_finished`, requeues inflight input once,
and retains the attached idle runtime plus H/O/T. If the session is unmuted and has
a durable schedule request, the same UOW invokes the shared scheduler.

`ExactRuntimeGone` requires positive exit proof for the exact H/O/T. Its single UOW
records `Interrupted`, finalizes and requeues once, clears H/O/T and provider session
authority, and enters the existing clean stopped/detached state. It never advertises
an attached runtime. If neither exact quiescence nor exact runtime-gone is proved,
the execution/effect remains quarantined and the relevant control/reuse capability
fails closed.

Runtime launch handle H is immutable and nonempty per launch generation and cannot
be reused. Owner O is fresh and nonreused for every custody transfer. The full H/O/T
tuple is rebound by atomic CAS, cleared only by exact runtime-gone, and cannot be
resurrected after terminalization. Runtime blocking uniqueness uses immutable H;
control and finalizer fencing use the complete current H/O/T. These private values
never appear in public events, logs, prompts, fixtures, or client state.

Before network admission, startup reconciliation scans every nonterminal execution
and effect in bounded pages and with bounded concurrency. Only `Assigned` with a
proved pre-dispatch state may start. `StartDispatching` and later states permit only
exact same-runtime adoption/control, provider-safe observation, or exact runtime-gone
proof; none authorizes blind start or interrupt replay.

Lifecycle reservations retain the exact private principal, payload, and creating
server-runtime generation required to finish an already-authorized recovery transition.
They are candidate-CAS input, never public snapshot data. Lifecycle phases distinguish
`prepared`, where no provider effect is authorized, from `effect_inflight`, where an exact
handle/owner and custody lease have been durably authorized before provider I/O;
`unconfirmed` records an uncertain return, and `effect_applied` records a confirmed stop
awaiting its durable result. A provider start API that bypasses this authorization boundary
does not exist in production.

One cancellation-owned reconciler scans the durable Agent Session keyspace in fixed-size
cursor pages throughout the server lifetime, covers every nonterminal lifecycle phase,
and observes at most a fixed number of captured candidates in parallel. The browser command
owner and reconciler must first acquire the same exact in-memory request claim. The winner
retains that claim across observation, candidate CAS, exact cleanup, and terminal commit;
the loser returns unresolved, so a check-then-act race cannot run browser reentry beside
server recovery. A changed CAS is discarded rather than reloaded as authority.

An abandoned `prepared` command is terminally rejected without provider I/O, and an
`effect_applied` stop is finalized without issuing stop again. `Gone` terminalizes the
captured request. For an exact same-sidecar `Adopted` or `LeaseUncertain` runtime whose
effect cannot safely be replayed, recovery first commits the observation under candidate
CAS, reloads only that newly owned candidate, stops its exact handle/owner, commits `Gone`,
then releases the confirmed-stop tombstone. If stop or proof fails, authority remains
fail-closed. `Ambiguous` and timed-out observations never authorize cleanup. Cancellation
may stop before observation or application, but it cannot interrupt an exact stop after
that external effect begins and leave its checkpoint owner unjoined.

Exact runtime-gone observation finalizes runtime checkpoint, interrupted turn,
session state, pending progression, execution, and effect in one UOW. Confirmed
`O1 -> O2` adoption must prove same handle, instance nonce, custody chain,
previous/new owners, and active generation before atomically rebinding session,
execution, and effect and incrementing owner epoch.

The new owner installs a recovery slot and resumes exact request interruption and
quiescence. If exact reattachment is unavailable but the supervisor can stop only
the exact owned runtime and prove exit, that explicit custody terminalization path
ends in runtime-gone. If neither can be proved, the effect remains fail-closed and
the provider mute capability is absent. Reachable parity providers implement the
verified control contract before advertising mute.

Before process-wide admission proceeds, lifecycle recovery uses clean schema 25. Version 25
binds `participant.mute` command-result replay to its canonical `event_seq`; older result
representations are rejected with their schema rather than converted. Its
private runtime identity is one indivisible handle/owner/lease-token tuple. Runtime-v5
cross-binds the launch token with the hashed OS boot identity; an old-boot absence proof
is accepted only when every available durable and lease witness names that same boot and
launch generation. macOS uses immutable `kern.bootsessionuuid` through the maintained
safe `sysctl` boundary. Missing, malformed, substituted, or unknown evidence remains
`Ambiguous`; every non-current schema is rejected without conversion or
compatibility behavior.

An issued interrupt is never finalized from issue state alone. Exact turn
quiescence or exact runtime-gone proof is required. The corresponding finalizer is
the only owner allowed to requeue inflight input and sets `requeue_finalized=true`
in the same UOW. Stop cleanup suppresses only exact benign already-stopped or
not-running outcomes.

### Implementation and optimization record

The candidate stores the immutable provider input, room view, delivery kind,
Agent IDs, and tabletop availability in the assignment transaction. Restart
recovery reads that envelope only for a proved pre-dispatch generation; it does
not rebuild authority from newer room settings or re-render history. The extra
bounded write avoids an unbounded restart query/reconstruction path and preserves
the exact originally assigned user flow.

One detached turn owner holds the provider driver while small runtime-slot locks
only copy or transition authority. Interruption uses a `watch` completion latch
and a cancellation token, so it has no polling loop, long-held global mutex, or
second provider call owner. The driver cell waits on `Notify` only while ownership
is absent. These choices reduce lock residency and idle CPU without creating a
cache or weakening exact H/O/T and generation checks.

The server-lifetime recovery owner advances independent provider-turn and lifecycle
cursors on one delayed one-second tick. Each provider-turn page contains at most 64
captured rows, observes at most eight concurrently, and bounds each observation to
two seconds; delayed missed ticks prevent catch-up bursts. Exact in-memory ownership
and retained results avoid OS observations on healthy active turns. This bounded
scan intentionally trades sub-second recovery latency for predictable SQLite, CPU,
and file-descriptor load while ensuring transient lease uncertainty is revisited
without a process restart.

Exact active-turn state is heap-allocated only while a runtime owns a turn, keeping
the common runtime-state enum small for idle sessions. The room command and startup
reconciliation futures are pinned behind explicit owner boundaries instead of
inflating the long-lived room/server actor future by roughly 18–19 KiB. This spends
a bounded short-lived allocation at command or startup admission to reduce every
resident actor's future footprint; it changes no authority, ordering, or retry
semantics.

SQLite partial unique indexes enforce one blocking execution per room-scoped
Agent Session and immutable runtime handle. Ordinary publication, RoomPortal
tools, mute, effect phase changes, terminalization, requeue, and scheduling use
exact CAS inside their existing room transaction rather than check-then-write
round trips. Startup scans fixed pages of 64 and provider observations retain the
existing bounded concurrency. Assignment envelopes are decoded only during
restart recovery; the normal provider path reuses the already materialized value.

An exact live recovery may move a prepared, claimed, dispatching, issued,
ambiguous, or recovery-required interrupt to quiescence only after the same
in-memory H/O/T, execution, turn, and generation token is found. Signalling that
token is idempotent and does not resend provider start. Without that token, or
without positive runtime-gone proof, the candidate retains blocking quarantine
and requeues zero inputs. A dispatch-time task death records the effect and
execution as one `InterruptAmbiguous` pair rather than leaving mismatched phases.

## Frontend and verification

The latest original frontend remains provenance. Shared resolvers/components own
provider presentation, profile-card/modal stacking, panel geometry, conditional
nickname input, and harness terminology; no approximate shell is introduced.

High-value deterministic contracts cover bootstrap concurrency/restart, lease ABA,
permanent debit/replay, `H == C`/gap/deadline, roster/roles,
mute-before-authorize, authorize-before-mute, Preparing-slot mute, duplicate mute,
pre-registration, every execution crash phase, claim expiry, owner-adoption ABA,
runtime nonce mismatch, typed quiescence, terminal suppression, runtime reuse,
idle unmute, early-unmute/remute, ordered-floor wake preservation, quarantine
requeue zero, later exact-proof requeue one, cross-room session identity,
immutable-launch uniqueness, cross-room finalizer rejection, and unmute progression.
The audited baseline's proof-transcript tests remain historical evidence only and are
not a target requirement without D-02's positive evidence gate.
The D-02 correction first requires a packaged endpoint-substitution attempt against
the exact owned child and fresh ticket. If it cannot establish an in-scope actor and
false acceptance, the local and remote grant/frame fixtures contain no proof key,
receipt proof, frame proof, or authenticated-envelope fields. Any retained receipt
or frame proof instead requires its own positive reproduction and focused rejection
test.
The implemented candidate additionally covers immutable assignment-envelope
rehydration, pre-dispatch task death, post-dispatch ambiguous task death, exact
live-control recovery from quarantine, late provider-result suppression by the
effect owner, and stale/muted RoomPortal tool fencing.

Each completed owner is also inspected for avoidable CPU, memory, latency,
task/process, serialization, and disk-write cost. Optimizations remain bounded and
evidence-driven: startup reconciliation runs once before admission, live scans and
observations have fixed page/concurrency/timeout limits, and no speculative cache or
duplicate authority is introduced merely to improve a synthetic metric.
Every material optimization records its prior cost or symptom, intent and owning
boundary, preserved product/security invariants, accepted trade-off, and measurement or
verification evidence in the active design or verification record. That rationale is part
of reviewability rather than an optional code comment.

`make verify` and packaged Computer Use are required. Computer Use verifies
identity/bootstrap, zero-room directory/create/join, chat/roster/panels/profile and
Agent Add overlays, surface-gated absence, role/mute busy interruption, runtime
reuse, unmute, reconnect, and finite catch-up. Only resources created by that
verification run may be stopped or deleted.

Each complete vertical slice is independently committed. Batch timing is owned only
by the active `Standing project workflow` in `AGENTS.md`. When that workflow invokes
review, it covers the exact pushed range. An
incomplete owner remains unexposed and is not completion evidence.

Historical pre-retirement completion evidence is recorded in `docs/VERIFICATION.md` through public code
checkpoint `035416c` and verification record `3b31a76`. That package used the
then-required Codex Terra, Antigravity Flash, and OpenCode Muse Spark matrix.
Current provider scope and final verification are owned by `docs/PRODUCT_REIMPLEMENTATION_PLAN.md`. Exact busy mute
terminalized without a late final for all three providers; the OpenCode defect found by
that run was corrected, re-run from a fresh package, and approved by both manual
reviewers before this boundary closed.
