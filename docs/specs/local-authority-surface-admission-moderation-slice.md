# Local Authority, Product Surface, Admission, and Moderation Slice

Status: active implementation design; Pro critical review approved 2026-08-25

Comparison baseline: original
`d5046473010d1353a81ee38337360e6d98f7bd6f`; Rust
`6de2671848b951fb16dc13bb2dd2dfeb25c1e88f`.

Implementation checkpoint, 2026-08-25: immutable local authority/zero-room
bootstrap, server-wide human profile, canonical directory/create-room flow, and
derived server/native product surfaces are published. The proof-bound finite
subscription section is implemented in the current candidate with strict
one-use ticket credentials, exact-byte Snapshot binding, a transactional bounded
`(C,H]` reader, client high-water readiness, and no string-ticket/non-desktop
fallback. The current admission candidate implements process-wide connection
leases, pre-parse raw scopes, permanent fresh-human mutation debit, and a separate
in-flight owner. Participant role/mute/provider-custody work below remains active
and is not implied complete by this checkpoint.

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

Participant roles are exactly `HumanLocal`, `AgentWorker`, `AgentObserver`,
`ExternalHuman`, and `Service`. Role is collaboration/routing metadata and never
operator authorization.

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
owns runtime cleanup, receives this process debit; only an exact durable replay or
prepared resume avoids a second debit. The retry ledger stores a fixed-size,
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

`participant.mute=true` rejects human targets and atomically commits agent mute,
scheduler exclusion, public event, command result/replay, exact current turn
identity, and a prepared interrupt effect. It never stops or detaches the
persistent runtime or Agent Session. If busy, it interrupts only the exact current
turn as the reachable original product does.

Only new active-turn assignment increments `turn_generation`. Active `turn_id`
and generation remain immutable; mute reads them and terminal persistence
validates them. Interrupt effects have a separate ID and sequence.

Every terminal-publication transaction revalidates exact room, session,
participant, turn, generation, runtime handle/owner epoch, `muted=false`, and no
unresolved effect. A mismatch cannot publish ordinary success.

### Provider-start serialization

Assignment creates the active turn and durable `ProviderTurnExecution` in one
transaction. Execution phases are `Assigned`, `StartAuthorized`, `Running`,
`InterruptPending`, `Quiesced`, and `Terminal`.

Before start authorization, the provider owner installs an exact in-memory
`ActiveProviderTurnSlot` in `Preparing`. The authorization transaction verifies
active identity, unmuted participant, no unresolved effect, and `Assigned` before
recording `StartAuthorized` plus monotonic start epoch.

If mute committed first, authorization is denied, provider call count stays zero,
and the Preparing slot quiesces into typed interruption. If authorization committed
first, mute targets the already-installed Preparing or Running slot, transitions
durable execution to `InterruptPending`, and interrupts that exact per-turn token.
SQLite commit order is authority; another DB read immediately before provider I/O
is not a TOCTOU substitute.

### Durable interrupt effects

Effect states are `Prepared`, `Claimed`, `InterruptIssuedWaitingTerminal`,
`AlreadyIssuedWaitingTerminal`, `NotCurrentWaitingAuthority`, `Unconfirmed`,
`Finalized`, and `Superseded`. All but the last two fence provider start and
ordinary terminal publication. Claims carry owner, generation, and expiry and
are safely reacquired. `NotCurrent` remains unresolved while the database still
says the exact turn is active.

Issuing a token or out-of-band interrupt is not terminal evidence. The provider
owner must unwind `send_turn`, observe cleanup, remove the exact slot, and settle
its terminal latch before emitting typed interruption with effect, turn, runtime
handle, and owner-epoch identity. Only then may the room actor finalize and
progress later work. A confirmed runtime exit is checkpointed in the same UOW.

Duplicate mute for one generation shares one durable effect owner and may not
issue a second physical interrupt or strand a second fence. Command replay never
reclaims a post-commit effect; only the reconciler owns unfinished effects.

### Restart and runtime custody

Before network admission, startup reconciliation claims every nonterminal provider
execution, including executions without an interrupt effect. Runtime custody binds
handle, owner, owner epoch, custody lease, provider runtime instance nonce, and
observation ID.

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

Before process-wide admission proceeds, lifecycle recovery uses clean schema 22. Its
private runtime identity is one indivisible handle/owner/lease-token tuple. Runtime-v5
cross-binds the launch token with the hashed OS boot identity; an old-boot absence proof
is accepted only when every available durable and lease witness names that same boot and
launch generation. macOS uses immutable `kern.bootsessionuuid` through the maintained
safe `sysctl` boundary. Missing, malformed, substituted, or unknown evidence remains
`Ambiguous`; schema 21 is rejected without conversion or compatibility behavior.

`InterruptIssuedWaitingTerminal` is never finalized from issue state alone. Exact
request quiescence, exact runtime gone, or exact-generation provider terminal
observation is required. Unmute commits canonical `muted=false` plus pending
reconciliation and advances scheduling idempotently. Stop cleanup suppresses only
exact benign already-stopped/not-running outcomes.

## Frontend and verification

The latest original frontend remains provenance. Shared resolvers/components own
provider presentation, profile-card/modal stacking, panel geometry, conditional
nickname input, and harness terminology; no approximate shell is introduced.

High-value deterministic contracts cover bootstrap concurrency/restart, lease ABA,
permanent debit/replay, proof transcript and `H == C`/gap/deadline, roster/roles,
mute-before-authorize, authorize-before-mute, Preparing-slot mute, duplicate mute,
pre-registration, every execution crash phase, claim expiry, owner-adoption ABA,
runtime nonce mismatch, typed quiescence, terminal suppression, runtime reuse, and
unmute progression.

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
