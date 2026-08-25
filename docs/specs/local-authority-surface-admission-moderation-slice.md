# Local Authority, Product Surface, Admission, and Moderation Slice

Status: active implementation design; Pro correction review pending 2026-08-26

Comparison baseline: original
`d5046473010d1353a81ee38337360e6d98f7bd6f`; Rust
`938a088`.

Implementation checkpoint, 2026-08-25: immutable local authority/zero-room
bootstrap, server-wide human profile, canonical directory/create-room flow, and
derived server/native product surfaces are published. The proof-bound finite
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

## Definition

This slice replaces the fixture-shaped bootstrap with a restartable local
authority, derives every reachable product surface from its real owner, makes
WebSocket readiness proof-bearing and gap-free, centralizes pre-parse admission,
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
collaboration/routing metadata and never operator authorization; human profiles,
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

Before JSON parsing or room queueing, one process-wide raw ingress governor
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
the same authenticated inner bytes. Missing or invalid resolution fails closed
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

## Proof-bound finite subscription

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
- Antigravity preserves the current reachable PTY behavior. Ctrl-C is issued only
  for a `Started` exact H/O/T/generation slot. A late or ambiguous Ctrl-C is never
  sent to an idle or reused runtime; exact stop proof or quarantine is required.
  This module can later be replaced without changing room, persistence, or common
  control contracts.

The live room owner retains the exact execution and dispatch nonce for every spawned
provider task. A task that exits without a typed result is not log-only: proved
pre-I/O `NotStarted` takes the safe failure path; death after start dispatch records
`StartAmbiguous` or `RecoveryRequired`; death after interrupt dispatch records
`InterruptAmbiguous` or `RecoveryRequired`. Post-I/O task death retains inflight
ownership and requeues zero inputs.

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
permanent debit/replay, proof transcript and `H == C`/gap/deadline, roster/roles,
mute-before-authorize, authorize-before-mute, Preparing-slot mute, duplicate mute,
pre-registration, every execution crash phase, claim expiry, owner-adoption ABA,
runtime nonce mismatch, typed quiescence, terminal suppression, runtime reuse,
idle unmute, early-unmute/remute, ordered-floor wake preservation, quarantine
requeue zero, later exact-proof requeue one, cross-room session identity,
immutable-launch uniqueness, cross-room finalizer rejection, and unmute progression.
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

Each complete vertical slice is committed and pushed before same-session critical
diff and Daybreaker Blue High manual-security review. An incomplete owner remains
unexposed and is not completion evidence.
