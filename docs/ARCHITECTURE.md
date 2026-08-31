# Rust Runtime Architecture

Status: current architecture owner

## Definition

AgentsAssemble is one asynchronous Rust runtime serving the existing React and Tauri clients while preserving every actually reachable product flow. It reimplements product semantics, not the Python module tree.

## Authority and cutover

The cutover unit is an authoritative contract owner, not a feature label. Routing selects the owner before a request is admitted. An owner has exactly one authoritative writer: Python before cutover or Rust after cutover.

- Runtime failure never falls through to the other implementation.
- Shadow comparison may read copied input but cannot write durable state, publish events, or affect a response.
- A completed cutover disables the replaced writer.
- Rust opens only a fresh authority created at the current schema or an already-current Rust authority. It does not import or convert Python or older Rust data.
- Rust writes are never followed by automatic Python rollback or a compatibility writer.

## Stable boundaries

### Room authority

Each room has one bounded command queue and one mutation task. That task serializes validation, domain transition, durable event sequence allocation, command-result persistence, and publication. Different rooms may execute concurrently. A cache, timer, queue, or projection is derived state and never a second room authority.

### Server and room-directory identity

One SQLite authority owns one stable opaque `server_id`, generated at database
creation and rejected if missing or malformed. A listening
port, sidecar process, Tauri window, browser origin, or cached room list is never
server identity. Each directory entry is projected from the durable `Room` and its
canonical room-global settings; stable room UID, label, status, timestamps, and
appearance are not independently reconstructed by React or Tauri.

Fresh schema installation creates infrastructure and stable `server_id` without
fabricating a room, participant, or profile. A separate bootstrap sidecar owns an
immutable authority lineage and restartable `Empty | Initializing | Complete`
transition under `BEGIN IMMEDIATE`. Ticket issuance requires verified Complete,
while Complete with zero rooms is a normal product state. Recovery checks only
bootstrap-owned rows and either proves the immutable lineage complete or fails
with explicit repair-required state; it never seeds or fills partial product
state. `Empty` also means that every table in the schema-owned product inventory
is empty; the inventory is gated against all current non-infrastructure tables.
File existence alone is never a durable bootstrap phase.

Creating a room commits its room record, default settings, publication cursor,
initial human membership, and exactly one `room_created` event in one SQLite
transaction. The human display/avatar projection comes from the server-wide
profile, while role, joined state, mute, and later transitions are room-owned.
Creation reserves one UUID request and canonical payload hash under the server
operator in that same transaction. Only an exact replay returns the stored room
UID without another creation event; another payload or a fresh request for the
same room ID conflicts instead of renaming or reopening it. The copied room rail may display a bounded cached projection during first
paint only while visibly unconfirmed; the authenticated server response removes
stale local entries and becomes the projection. A client-fabricated `general` is
not authority; a fresh complete authority stays empty until the user creates a
canonical room through the real directory flow.

The desktop startup gate validates the closed room-directory response and binds
both its server ID and immutable authority lineage to the immediately preceding
native bootstrap grant. Missing fields, protocol drift, a stale/reused loopback
endpoint, and legacy invite/query routes cannot turn an unverified response into
a successful zero-room authority.

The active-room participant projection has one browser owner: the authenticated
WebSocket snapshot plus its sequenced participant events. React does not fetch,
cache, merge, or silently ignore a second HTTP roster. Sequence gaps and stale
scope are resolved only by the canonical WebSocket resynchronization boundary.

Local manager-invite authority is one exact snapshot of the current server ID,
bootstrap lineage, stable room UID, canonical room ID, and local manager identity.
The private control boundary verifies the caller-captured tuple before issuing a
one-use create- or revoke-only ticket. Ticket consumption transfers that same snapshot
to persistence, whose owning mutation transaction re-resolves and compares every field
before changing an invite. A room ID, cached directory row, ingress snapshot, or
generic room HTTP grant cannot independently authorize invite management.

The frontend manager-invite API is the single create-response and revoke-result
boundary. It sends only the canonical human invite intent through the exact desktop
grant, validates the returned signed-token digest, independent join credential,
canonical public HTTPS join URL, exact room echoes, and finite server timestamp, then
returns one immutable room/invite custody value. A native or guard failure before
`fetch` is proven not dispatched; every failure after `fetch` begins is unknown except
the exact terminal revoke success and `invite_not_found` results. It does not consult
live ingress while accepting a committed response, reconstruct a URL, retain a second
raw credential, or own controller/UI lifecycle state.

### Room settings, preferences, and appearance

SQLite `rooms.settings_json` is the only room-global settings record. One strict
domain validator owns its complete shape and sorted-key compact-JSON revision, but
field recognition does not imply mutation availability. A settings command may
change only behavior whose server authority exists; channels and activity hosting
still fail explicitly, while invite scope and room-owned asset URLs use their
completed server owners. The settings, synchronized room label/time, one event,
and command replay result commit atomically. Scheduler reconciliation runs after
one fresh commit, cannot rewrite its ACK as a NACK, and is retried from later
lifecycle triggers when progression fails. An exact command replay returns only
its stored outcome and never re-executes floor progression.

Authenticated room preferences are a separate complete row keyed by stable
`(user_id, room_id)`. They may affect only that user's notifications and read
cursors. Channel preference keys accept the four builtin IDs or the original
canonical `c` plus twelve lowercase hexadecimal characters without treating the
preference row as channel authority. Fresh one-use purpose-scoped HTTP tickets,
authentication before body admission, and current membership gate every read and
write. Global settings stay WebSocket-owned; preferences stay HTTP-owned. Until
that HTTP owner is present, the copied controller exposes an unavailable
preference state, disables its notification controls, and never presents local
defaults or optimistic writes as confirmed persistence.

Room appearance bytes are neither settings nor public authority on upload. A
pending, expiring, room-and-owner-bound `ra_` capability becomes reachable only
when the existing settings transaction promotes its exact URL. Banner and icon
references are transitioned together and an old bound object is removed only when
neither field still references it. Profile and room images share one safe-raster
decoder and one global decode-admission semaphore; accepted raster input is
bounded and re-encoded to static PNG. Pending preview and bound reads are
non-cacheable, `nosniff`, and `no-referrer`; active content, arbitrary URLs,
cross-room references, and cross-owner binding fail closed.
Pending preview uses an exact current-local-manager ticket. A bound read accepts
either an exact current local member ticket or an admitted human's purpose- and
asset-bound one-use ticket. The latter retains durable session provenance and
revalidates session, membership, profile binding, room reference, and asset bytes
in one transaction; the raw session credential never authenticates the attachment
route. Rejected reads validate metadata and `length(content)` without loading the
bounded BLOB, while successful reads pay one second query for its bytes.
The desktop private control pipe exposes upload, pending-read, and bound-read as
three typed commands. Upload and pending-read retain the exact server, authority
lineage, room UID, manager user, and participant tuple through target revalidation;
the frontend cannot select authority with a path or generic operation string.

### Durable commands

`(room_id, principal_id, request_id)` identifies a command attempt. Repeating the same action and canonical payload returns its committed result. Reusing the key with a different action or payload is a conflict.

Admission has distinct process, room, human-principal, and provider-session
owners. Before parsing, the process-wide socket admission owner charges every first
subscription and later text, binary, ping, or pong frame atomically to fixed
10-second global, principal, and room windows. The respective message/byte/control
ceilings are `4,096 / 32 MiB / 1,024`, `256 / 2 MiB / 64`, and
`2,048 / 16 MiB / 512`. A new ticket, connection, or room cannot reset the
principal scope, and rejected over-limit frames stay charged until the window
turns over. After exact `room.history` parsing, the same owner independently charges
one read plus its requested event count before persistence. Fixed process/room/principal
request/event ceilings are `640/6,400`, `320/3,200`, and `10/1,000`; over-limit reads
fail explicitly without closing the socket or consuming mutation admission. All three
history scopes are preflighted and committed atomically, so rejected narrow-scope work
cannot consume a broader history budget; its raw frame remains charged separately.

After strict protocol and implemented-action classification, exact committed
replays and matching lifecycle resumes are recognized first. Every fresh human
write, including a stop that owns runtime cleanup, is then charged to one
process-wide rolling 60-second principal window (3,600 commands, 8 MiB of
canonical command bytes). The bounded retry ledger stores only a
domain-separated 32-byte intent digest and retains at most 32,768 mutations across
at most 512 principal windows. A separate 128-permit in-flight owner transfers its
RAII permit into the room command, so disconnect or caller cancellation cannot
release work the actor still owns. A third room-wide 60-second command and payload
window is durable in SQLite and is reserved in the same transaction as the command
result, lifecycle intent, or provider tool event. Only an `agent.stop` that owns
actual runtime or lifecycle-intent cleanup, and an already-owned terminal
completion, remain available at durable room-budget saturation; every fresh stop
still consumes the process principal budget, and a fresh already-stopped no-op
stop also consumes the normal durable room budget. Provider RoomPortal results
use a separate per-room-actor, per-Agent-Session rolling budget keyed by the
server-owned Agent Session ID, never a provider conversation identity, and never
borrow a human principal identity. Only an unresolved intent remains eligible to
reuse its principal debit; a committed or definitively rejected actor outcome
closes that retry exemption without refunding the original charge.

The mutation transaction orders work as follows:

1. authenticate and authorize the principal;
2. identify an exact replay or lifecycle resume before write admission;
3. admit the new request to the shared principal window;
4. stage the durable room-wide reservation and validate current state inside the
   owning mutation transaction—any later rejection rolls the reservation back;
5. apply the state transition;
6. append any canonical event and allocate its room sequence;
7. persist the correlated command result;
8. commit;
9. publish the event and return the ACK.

No ACK or event is externally visible before commit.

Commands that own external provider effects use the same global command namespace,
but the database and an OS process are not one atomic transaction. The first
durable commit for `agent.create(start=true)` reserves the request namespace,
action, canonical payload hash, derived Agent Session ID, start intent, and current
phase. Session creation, lifecycle preparation, the external effect, observed
runtime evidence, and the terminal command result are explicit durable phases.
Crashes and concurrent retries resume that one reservation; they cannot create a
second session, lifecycle intent, or provider process. The public result preserves
the original created-session, participant, and optional-start fields. Partial or
uncertain start outcomes remain explicit rather than being collapsed into success
or a stopped session.

ACK and event delivery happens only after the owning commit. The protocol contract
must state the permitted post-commit ordering: a connection may observe the event
before its ACK, and reconnect may recover the event before command-result replay.
Clients therefore correlate results and advance durable cursors, but never turn an
optimistic ordering assumption into state authority. After a command is sent, an
ACK deadline closes the connection but retains the exact request identity and
payload for authenticated replay; it never converts an unknown commit outcome
into a fresh-ID retry. Explicit shutdown reports that unresolved state as
`outcome_unknown`. Every command response carries a server-owned resolution:
an ACK is `committed`, a NACK is `rejected` only when the command owner can prove
a definitive rejection, and queue, transport, principal-resolution, persistence,
or public-projection ambiguity is `unresolved`. The client accepts only the first
two as server-terminal. While its finite budget remains, an `unresolved` resolution closes the
socket while preserving the exact private request ID and serialized bytes for fresh authenticated
replay. Successful socket authentication cannot reset the command-owned reply count or backoff.
Only a command frame accepted by `WebSocket.send` consumes that connection generation's uncertainty
budget. The pending command owns its current transmission generation and distinguishes in-progress
frame encoding from an accepted send; removing the pending command removes that state. The retry-policy
owner atomically refuses a second charge for the same generation, so a pre-send close or competing
terminal signal cannot manufacture an extra attempt.
Close-derived accounting waits behind authenticated frames already accepted into that connection's
verification queue. A valid terminal ACK or NACK settles and removes both pending authority and its
transmission state before close may classify the still-pending sent commands.
The eighth unresolved reply ends replay, rejects only that local operation as `outcome_unknown`,
and leaves the otherwise authenticated socket open; it never claims rejection or commitment and
never creates a replacement request ID. Missing, malformed, or mismatched resolution still closes
the connection as a protocol failure.

Resolution is owned at each action's effect boundary, not inferred globally from
an error enum. A deterministic failure inside a transaction that cannot have
committed is `rejected`. A safely failed provider launch is rejected only after
its terminal failure state and exact public rejection are committed together.
That request remains reserved and replays the same rejection without another
durable room-budget reservation, event, or provider call. Because the rejection
is definitive, each later exact replay receives a new process principal debit;
only an unresolved attempt may reuse its prior in-memory debit. Once
creation/lifecycle preparation has committed, a provider effect is uncertain or
applied, or completion/publication
may have failed after commit, every nonterminal failure is `unresolved` even when
its diagnostic uses a command-rejection-shaped error. An uncertain failure may
publish its committed recovery events without releasing the request identity.
Replaying an `unconfirmed` lifecycle intent performs no provider effect and no
durable mutation until reconciliation records a new authoritative observation.

### Room event cursor

Room event `seq` is positive, durable, monotonic, and scoped to one room. A client advances only over contiguous sequences. Initial, resume, gap, and resynchronization responses preserve the existing frontend meaning. Missing or inconsistent ranges fail visibly and trigger authoritative resynchronization.

Snapshot, catch-up/history, and live fanout use one viewer visibility policy. Each
durable sequence is projected for each authenticated viewer as either its public
event or a minimal `event_hidden` envelope retaining `id`, `seq`, `room_id`, and
`created_at`; an invisible event is never deleted from that viewer's cursor. ACKs,
command results, errors, and resynchronization payloads cross their own public
redaction boundary and cannot disclose private runtime profiles, provider
conversation identities, workspace/executable authority, or internal diagnostics.

### Protocol

The Rust protocol crate owns WebSocket envelopes, request correlation, error shape, snapshots, ACKs, and canonical public event types. TypeScript bindings are generated from that owner; handwritten transport copies must not become a second authority.

The existing outer envelope remains compatible:

- client: `subscribe`, `command`, `ping`;
- server: `subscribed`, `snapshot`, `event`, `ack`, `nack`,
  `resync_required`, `pong`.

Action payloads are added only by the slice that implements them.

One-use room tickets also carry a private proof key. The server registers the
canonical live receiver before reading a durable snapshot, serializes the exact
final Snapshot frame at cursor `C`, and fixes one transactionally authorized
high-water `H`. A versioned length-delimited `Subscribed` HMAC binds the ticket-
derived connection nonce, challenge, room/principal/participant, protocol and
accepted streams, current server product surface, exact permissions digest,
`C`, `H`, and the SHA-256 digest of the exact Snapshot UTF-8 bytes. The server
sends `Subscribed`, that exact Snapshot, then the bounded contiguous durable
range `C+1..H`; overflow or a missing sequence closes without readiness.

The browser compares the receipt with its already pinned native/server surface,
recomputes the connection nonce, HMAC, Snapshot-byte digest, and canonical
permission digest, and reaches ready only at `delivered_seq == H`. TCP open is
not product readiness, and queued commands cannot cross the socket before that
boundary. A single absolute deadline owns the whole establishment flow.

### Application and transport boundary

HTTP and WebSocket adapters authenticate, decode, and encode. They do not own
product mutations. Both call the same application command owner, which performs
authorization, persistence, result shaping, and post-commit event publication.
Authentication/OAuth, WebSocket ticket issuance, pre-connection directory and
admission, server-wide profile and credential operations, files, paginated reads,
and health/version may remain HTTP when that is their reachable entry point.
Connection-, cursor-, and ordered-ACK-coupled realtime commands remain WebSocket.
An HTTP mutation may commit through the application owner and then fan out its
durable event over WebSocket. Neither transport is a fallback for the other, and a
mutation is never implemented twice merely because both transports exist.

### Admission and public-ingress lifecycle

Invite preflight, browser admission, external-session admission, and operator
pairing begin over HTTP because no room WebSocket authority exists yet. An HTTP
join response is not a completed user flow: the admitted principal must obtain the
canonical room WebSocket, receive its authenticated initial snapshot, and reach
the capability-appropriate ready state. A normal `room` invite must then be able
to issue permitted room commands; a `read_only` invite must remain visibly unable
to post. The server derives that distinction from the invite and room state. A
missing realtime connection is a failed admission flow, not a read-only downgrade
or an HTTP fallback mode.

A raw human `aas1.` bearer is reduced to its canonical SHA-256 fingerprint at the
HTTP edge. Persistence resolves that fingerprint together with its active session,
expiry, room, person profile, and joined human participant in one read snapshot.
The resulting non-serializable `HumanSessionAuthorization` contains no raw bearer
and has private fields, so adapters and the in-memory grant store can retain
provenance but cannot manufacture it. Invite preflight reuses this same resolver;
there is no second session-liveness or profile-binding authority. A foreign-room
bearer remains inapplicable to preflight, while a same-room ended, expired, inactive,
or left session is unavailable and malformed same-room authority fails visibly.

The browser owns one bounded admission-intent record, separate from the durable room
session. Pending state binds only fingerprints of the invite and current browser
credentials plus the exact client/request/profile input. Durable session acceptance
or a definitive terminal response invokes one retirement operation. A verified
settled write is followed by best-effort removal; if that write cannot be verified,
the owner instead attempts and verifies direct removal of the observed pending
record. Failure of both operations remains unresolved. A settled record surviving
removal is cleanup-only and never replays admission. Ordinary expiry or clearing of
the separately owned RoomGuestSession local-storage record does not erase that
marker; direct external deletion of the admission-intent session-storage key does and
is outside this guarantee. An unresolved pending mismatch remains fail-closed unless
exact completed-session evidence binds the same invite, current browser credential,
and client. No raw credential, second storage owner, fallback, or client-side
admission authority is introduced.

Human browser admission, an externally owned RoomConnector session, and a managed
Agent Session are separate product identities. They may reuse admission and
realtime mechanisms, but one cannot be represented by another or inherit its
lifecycle owner. Operator pairing is a purpose-separated, exact-public-origin,
short-lived one-use credential. Redemption removes the secret from browser
history, restores the canonical operator identity, and makes every replay fail
without consulting an existing browser session as fallback authority.

A configured manual public ingress exists only when startup supplies both one
canonical non-loopback HTTPS origin and one bounded high-entropy reverse-proxy
secret. `PublicIngress` owns that immutable pair. The common TCP ingress policy
requires an actual loopback peer, the exact public Host and HTTPS forwarded scheme,
and a fixed-size digest comparison of the presented proxy secret before consulting
the route descriptor. Private routes never become public; same-origin public routes
require an absent or exact public Origin, while the two identity-probe routes may
accept a foreign Origin after complete transport authentication. The verified origin
is passed privately to host-identity signing rather than reconstructed from an
untrusted handler header. Missing, one-sided, or invalid startup configuration fails
closed and creates no runtime fallback or mutable public-ingress state.

The current owned managed public-ingress lifecycle includes process custody, generated
origin credential, direct public URL publication, configured stable-entry publication,
ingress revocation, and confirmed cleanup. Configured stable ownership is claimed at
activation and bound to the canonical database state root; a superseded process cannot
publish or clear its successor. Shutdown is incomplete until the exact cloudflared
process tree and any stable publication task have exited, the managed ingress and
public URL are revoked, and the stable target has durably cleared or reported an
explicit cleanup failure. Every fallible startup path after ownership claim enters
that same cleanup boundary. A daemon task that may be abandoned at process exit is
not cleanup authority. Internally this boundary may share maintained HTTP, WebSocket,
process, and Cloudflare mechanisms; optimization may not weaken origin binding,
credential separation, or completion semantics.

### Destructive mutation semantics

Client confirmation is a safety gate, not mutation authority. Message deletion,
participant kick, and room deletion execute through the room application owner,
commit canonical state once, and publish the resulting tombstone, roster removal,
room deletion, or session revocation to every affected connection. Permanent room
deletion additionally requires the exact current room name at the server command
boundary. A local list removal, optimistic tombstone, or disconnected client is
not completion evidence.

### Authentication and authorization

A credential resolves once to an `AuthenticatedPrincipal` containing stable identity, room scope, client kind, and server-derived capabilities. Client-supplied roles, operator flags, participant type, or capabilities are never authority.

Opaque, short-lived, one-use WebSocket tickets remain the room-connection
credential. Browser-compatible HTTP ticket issuance stays an adapter while it is
a reachable flow and always requires a high-entropy host secret or an authenticated
session. Desktop mode cannot start with an empty host secret; Tauri generates it
per owned runtime. Its private control pipe issues either a room WebSocket ticket
with the validated loopback WebSocket origin or one exact-purpose local HTTP grant
with the validated loopback HTTP origin. Current HTTP grants separate server-operator,
room-bound preference, message-search-read, and human-invite create/revoke,
settings-directory-read, and central-registration authority. React receives neither
the host secret nor a reusable credential. A ticket presented to the wrong transport
or scope is consumed and rejected rather than interpreted as another authority.
Public human-session grants reuse this same bounded store but retain the opaque
durable `HumanSessionAuthorization` and one exact purpose. Issuance is capped at
1,792 live public grants and eight per session fingerprint, leaving at least 2,304
of the production store's 4,096 entries for local/private authority. Grant expiry is
the earlier of the store TTL and backing session expiry. Read-only sessions cannot
mint preference-write grants. Consumption removes a wrong-purpose grant before
rejecting it; the later target adapter must still revalidate its durable session
before any read or write. Public WebSocket, own-profile, and preference read/write
exchange routes are connected and verified with that target revalidation. The room
attachment exchange remains incomplete and is not claimed reachable until its
corresponding message behavior and target revalidation are implemented and verified.
Lobby message search and context consume the same one-use `message-search-read`
purpose before query or body validation, then revalidate current membership and
`room.history` permission in the canonical persistence read transaction. Their GET
responses are private/no-store, and an unavailable custom channel is never replaced
with lobby data.
Central server registration is a third, exact-purpose one-use ticket issued only
through the private desktop control pipe and consumed only by the desktop-mounted
registration-proof POST. Its Ed25519 private key is a separate owner-only write-once
file; SQLite stores only the bound public key, so a database-only backup cannot clone
the signing authority and missing or substituted key material fails closed. Key opening
distinguishes new, interrupted-empty, and initialized database custody: only the
interrupted-empty state may reuse a key whose versioned file envelope carries the exact
UUID nonce already committed in SQLite's singleton initialization marker. A database
path that does not yet exist rejects an orphaned key before creating any new authority;
if a stale key appears after that check, its nonce cannot match the newly committed
marker and later opens remain fail-closed. The same private control response pins the
expected server ID, raw public-key projection, and fingerprint. React accepts the
loopback HTTP proof only when its exact schema and binding match those native values,
its canonical JWK hashes to the pinned fingerprint, and its exact registration
transcript verifies under the pinned Ed25519 key. A self-consistent response signed by
a substituted loopback key therefore fails before any registration request reaches the
central directory.

The local HTTP/WebSocket adapter has explicit resource budgets: admission is bounded immediately after TCP accept, incomplete HTTP headers and request bodies have real deadlines, and a consumed one-use ticket must atomically acquire a process-wide WebSocket lease before HTTP 101. The active-only lease owner admits at most 128 connections globally, eight for one principal, and 64 for one room; rejected acquisition increments no scope, and checked process-local `u64` generation IDs prevent stale release from freeing a replacement. Inner product frames stop at 256 KiB and their authenticated wire envelopes at 384 KiB, the first subscription has a ten-second deadline, and the process-wide raw governor above owns message, byte, and control-frame windows. When a bounded principal or room map cannot admit a new key, the rejected frame still charges the global scope and any already-tracked applicable scope without retaining another key. The one-use ticket proof establishes a connection key; after the receipt-bound plain Snapshot, every frame in both directions is authenticated over connection nonce, direction, a strict contiguous counter, and exact inner bytes before projection or command execution. Binary frames are rejected. The server closes a socket after five minutes without client ingress. The browser therefore schedules one authenticated keepalive only after three minutes without a client frame, resets that one-shot owner after a command, and cancels it with the exact connection; it does not poll HTTP or durable state. Room queue admission never waits and returns `room_busy` when saturated.

Authorization is evaluated from the current principal and durable room state when
the application command runs, not frozen as a hard-coded local-operator identity at
connection construction. Capability changes therefore affect later commands on an
existing connection without accepting client-supplied authority.

### Identity and profile ownership

The canonical human profile is keyed by authenticated `user_id`. The left-bottom
profile card is the UI display reference for that human identity; member lists,
message authors, and other room surfaces consume a revisioned projection of the
same profile. Participant display name and avatar stored with a room are projection
cache, not a second human-profile authority. Room membership, role, mute state, and
permissions remain room authority and are never overwritten by a profile update.
Each Agent Session owns its own Agent display, avatar, and configuration and never
inherits or merges the owner's human profile.

Room roles use one strict typed contract with the reachable wire values `human`,
`director`, `implementer`, `reviewer`, and `agent`. The room participant is the
only current-state owner. `participant.role.update` reauthorizes `room.manage`
inside its SQLite transaction and commits the participant, sequenced
`participant_updated` event, and idempotent command result together. The frontend
does not maintain a role-override map or force human/Agent Session presentation
over canonical room state. Role likewise never classifies participant kind or
ownership: `participant_type` separates people from Agent Sessions, and a created
Agent Session's `owner_id` is the authenticated owning participant ID. Authorization
continues to resolve the independent principal and participant before mutation.
Historical aliases and compatibility normalization are rejected. Clean schema 23
rejects schema 22 because the serialized participant role changed from an
unconstrained string to this typed contract.

A committed human-profile revision atomically writes every affected projection in
an Active room where that human membership is still Joined, plus its durable event.
Ended memberships retain their historical identity instead of receiving later
profile disclosures. `room_events` plus one durable per-room publication cursor
form the live outbox for every event producer; only the room actor drains that
history in sequence order and updates the cursor. Startup, the committing room input,
and external HTTP/reconciliation commit wakes are the normal drain triggers. A successful
drain owns no timer. Only an observed drain failure arms one room-owned retry deadline;
repeated failure backs off from 250 milliseconds to a five-second cap, and the first
successful drain removes the deadline and resets the delay. One failure epoch ends after eight
consecutive failed drains: no further timer is armed, the durable cursor backlog remains intact,
and only a later real room input or external commit wake attempts it again. HTTP success therefore means the
canonical profile, projections, and publication work are durable, not that every
receiver consumed them. Handler cancellation, queue pressure, and restart leave a
cursor backlog that the room owner retries within that finite failure epoch without an idle
database poll or fallback broadcaster; no profile-only
broadcast path exists.
WebSocket delivery suppresses duplicate sequence numbers and requires the next
exact sequence, otherwise it closes with resynchronization. Retry of the same
profile mutation reuses the same revision and cannot create duplicate revisions.
No local profile file, cached browser name, or older profile record is imported or
used as read fallback.

Profile-avatar uploads are capabilities with two durable states rather than public
blobs on receipt. The authenticated upload path verifies declared type against
bytes, decodes under bounded resources, and re-encodes one static PNG. A pending
opaque ID is hidden and expires after 15 minutes; the profile swap transaction
promotes the new ID, removes the prior bound object, and publishes the new URL.
Only bound PNG bytes are public, and responses are non-cacheable so removing an
avatar also removes the server-readable capability. The current schema creates
this representation directly; older attachment records are rejected with their
schema instead of being converted or exposed.

### Runtime lifecycle

Tauri owns the local sidecar it starts. The package carries the built frontend once as a Tauri resource for the sidecar and passes that fixed resource directory as `--frontend`; the server remains the sole canonical-path and `index.html` validator and fails startup if the resource is absent. The sidecar binds loopback, reports one structured startup record containing the selected address and readiness, and is cancelled and reaped by its owner. The server accepts its host secret only from a private anonymous stdin control pipe; argv and environment credentials do not exist. Tauri keeps that write end open, and EOF makes a running sidecar shut down. A second anonymous control pipe is owned only by Tauri and watched by a separate minimal process; parent death closes it and the watchdog force-kills the sidecar process tree even if the sidecar is stopped and cannot cooperate. Transport failure or an invalid ticket response retires the unhealthy owned child so the next request starts a fresh runtime, while valid application rejection does not restart it. Reusing an existing runtime requires a data-root-scoped ownership record plus a live readiness proof. The lifecycle control plane is separate from room application messages.

On macOS, the desktop runtime supervisor is also the lifecycle owner for its private
executable copies. The running desktop-image re-exec and server-sidecar binding share
one desktop-only `0700` staging root. A per-directory exclusive lease protects every
active `BoundSidecar`; root-serialized creation and owner-drop cleanup with nonblocking
root-lock acquisition reclaim only unlocked crash directories. Unknown entries, unsafe
ownership or modes,
and scans beyond the absolute bound fail closed. This root is intentionally distinct
from provider staging because the desktop and provider supervise different process
lifetimes.

### Persistence

SQLite is the local durable authority. The Rust schema owns its version, stable
server identity, and cutover marker, while an adjacent process-lifetime exclusive
writer lease prevents two Rust runtimes from becoming concurrent room authorities.
A nonempty database without the Rust owner marker is rejected before any schema
write, and any non-current schema version is rejected before product state is read.
A command result and its
event commit in one transaction, as do canonical room creation and its initial
membership/event boundary. Persistence failure is an error, never an in-memory
success.

Room snapshots are read in one SQLite transaction. Their `oldest_seq` and `last_seq` describe the returned event range, not an independently sampled global range. Initial connections receive the newest bounded tail; resumes receive every event after the cursor when it fits, an explicit gap tail otherwise, and the empty `(oldest_seq=0, last_seq=cursor)` boundary when already current.

A resume cursor ahead of durable state produces `resync_required` with the durable latest sequence. The browser transport then clears its local cursor and reconnects for an authoritative initial snapshot; it does not retry the impossible cursor indefinitely.

PostgreSQL central hosting is a later vertical slice, not a reason to introduce
`sqlx::Any`, speculative repository frameworks, or mock backends now. SQLite SQL,
row types, pragmas, and connection types stay inside persistence. Server code calls
meaningful persistence operations such as command commit and lifecycle preparation
rather than assembling raw rows or issuing SQL. Sequence allocation, idempotency,
lifecycle reservation, and commit-before-fanout are application/persistence
contracts so a later concrete PostgreSQL implementation can supply a shared
application transaction, per-room advisory transaction lock, exact schema
authority marker, and fail-closed startup without a SQLite fallback.

### Provider catalog and Agent Sessions

The provider crate owns installed-provider discovery, catalog normalization, catalog revisioning, and selection validation. Provider probes run in dedicated owned process trees, inherit only an explicit credential-free environment allowlist, and have bounded time and output. Cancellation kills and reaps the full tree. Windows creates the probe suspended, assigns its Job Object, then resumes it; Unix starts it as a new process-group leader. OpenCode subscription models are restricted to the original managed namespaces. Public catalogs are bounded before publication; the server cannot turn a missing, malformed, oversized, or provenance-invalid catalog into a startable provider. The provider crate does not own rooms or persistence.

Future provider cutovers use one explicit compile-time registry rather than adding independent catalog, selection, factory, and frontend branches. A registration owns one stable descriptor, discovery function, exact supported provider/runtime/transport combinations, launch constructor, and capability set. The catalog descriptor is also the only behavioral provider input to the copied frontend. Branding may have one presentation-only resolver, but provider identity must not create frontend-owned transport, credential, capability, or validation branches. Provider modules own native differences; common registry, selection, and supervisor modules contain no growing provider-name conditionals. Persisted provider kind, runtime kind, transport, execution harness, model, and profile key remain exact server authority. An existing session changes that composition only while fully stopped and only after fresh catalog and filesystem revalidation; there is no live hot-swap or fallback.

Claude is not inserted ahead of its ordered provider slice. When that slice begins, its reachable runtime is implemented against the official Claude Agent SDK rather than a transcript scraper, print mode, or compatibility shim. Experiments that expose subscription-backed model sources to alternate harnesses, including a CLIProxyAPI-style model-source gateway, remain deferred until the product reimplementation is complete and are not part of the current runtime architecture or completion evidence. This is also a repository-wide file-boundary rule: independently changing owners are split before the source ceiling, while code that must change atomically to preserve one invariant remains together. The current first-bundle runtime still has fixed Codex, Antigravity, and OpenCode discovery/launch branches and accepts only the builtin execution harness; that fixed branching is current truth, not the target shape.

An Agent Session's configured and desired state is durable room state. Its public projection deliberately excludes workspace paths, executable paths, filesystem identities, runtime handles, provider conversation identities, lifecycle intents, and the runtime profile key/version; those fields exist only in the private durable record. Exact workspace input is canonicalized without text cleanup. The workspace identity and the executable identity—bound to both its opened filesystem object and complete bytes—are revalidated between a short replay transaction and the final write transaction. The final transaction reauthorizes the room and rechecks command replay before committing, while slow filesystem work never holds the single SQLite writer. Filesystem validation uses a fixed-capacity set of detached standard threads with deadlines; a stalled operation retains its permit until it actually exits but cannot make Tokio runtime shutdown join a blocked filesystem worker. Rooms admit at most 64 sessions so non-event snapshot metadata remains bounded. Live provider processes and their task handles are observed resources owned by one server supervisor. Lifecycle effects begin only from committed intent and report completion through the room mutation owner. Stop confirmation is durably marked before finalization so a retry cannot repeat an already-applied external effect. Replayed commands reuse their durable result before consulting a newer catalog or launching an effect.

`agent.configure` follows the same two-phase authority rule as creation without changing Agent Session identity. The room owner first authorizes and loads one exact stopped private profile, then the provider catalog merges only the client-selectable runtime controls with that stored provider/workspace authority. The final transaction reauthorizes, rechecks replay and the original profile key, revalidates the selected filesystem identities, and atomically replaces both the private profile and public projection. The Agent Session owns only a selected persona-library ID and its matching safe summary; creation and stopped configuration resolve both inside that final transaction, while private normalized card content remains library-owned. Running, active-turn, owned-handle, and lifecycle-intent states fail closed; a successful profile save writes the current durable profile version and emits a canonical `agent_session_state` event. Empty string values are accepted for the copied React controls whose current contract makes those controls optional, while provider/runtime/transport identity stays server-owned.

Provider diagnostics are untrusted process output. Before an error enters a durable Agent Session, room event, command result, snapshot, or public projection, the shared domain boundary removes local paths, credentials, authorization headers, secret-shaped options and assignments, URL user information, JWTs, and private keys, then applies the field's size limit. The common provider adapter exposes only stable public error codes and messages; protocol payloads and stderr never cross directly into room authority.

Lifecycle command payloads carry exactly one unchanged Agent Session identifier alias and no unknown keys. The external-effect operation identity binds the exact room, principal, request ID, and action; only that operation may finalize its work, and an opposite lifecycle command cannot replace it. Before an incomplete lifecycle command leaves its write transaction, a room/principal/request reservation also binds its action, payload hash, Agent Session, operation ID, phase, and the immutable random generation of the `SqliteStore` runtime that created it. Schema 23 retains that generation with the exact private authenticated principal and payload needed for a server-owned recovery commit, and makes the effect boundary explicit: `prepared` authorizes no provider I/O, `effect_inflight` durably binds the reserved handle, supervisor owner, and custody lease generation before provider I/O, `unconfirmed` records an uncertain return, and `effect_applied` records a confirmed stop awaiting its result checkpoint. None of those private fields enters the public projection, and the provider adapter exposes no production start path that can bypass the durable authorization. Every non-lifecycle command admission checks the same request namespace, so the reservation is the in-flight or terminal-failure phase of one global command authority rather than a second authority beside completed results. It survives recoverable failures and restart; a completed command replaces it with the durable command result, while a safe terminal launch failure retains a bounded public rejected outcome for exact replay. A provider-proven safe start failure also retains its exact pre-effect reservation or confirmed-stop tombstone until persistence commits either the terminal database result or the same request's exact live-`Gone` transition back to `prepared` empty authority. Only after one of those commits may the adapter release the captured handle/owner/lease-token triple; a failed database checkpoint leaves the proof owned for restart reconciliation rather than opening a crash window with neither a terminal result nor absence evidence, while successful live recovery cannot leave a tombstone blocking the newly authorized generation. Schema 22 and every other non-current schema are rejected without migration or compatibility code. Process reuse and provider-conversation reuse are independent observations; an app-server process is not proof that a Codex thread is active, and a reused conversation must retain its exact durable identity. Every runtime handle is paired with its private supervisor-instance owner and lease-generation token. Ambiguous start or shutdown retains its exact operation and complete runtime identity rather than turning uncertainty into success or a new generation; exact replay remains unresolved and cannot repeat the provider effect until authoritative reconciliation changes the intent. A confirmed stop is held as an in-memory tombstone until persistence checkpoints `effect_applied`, so a checkpoint retry cannot repeat the external stop. Persistence never emits a stop effect with a missing handle, owner, or lease generation and never accepts a DB-only reused start.

Runtime reconciliation is a three-owner protocol: persistence loads a complete private candidate outside process I/O, the common provider supervisor reports `Adopted`, `Gone`, `LeaseUncertain`, or `Ambiguous`, and persistence validates and commits the lifecycle-specific transition with an exact candidate CAS. Drivers report facts and never choose room state. A provider `Gone` observation never consumes its launch lease or tombstone; the live-request, watcher, startup, or shutdown persistence owner first commits its exact captured candidate, then releases only that captured runtime generation. The common post-commit release owner dispatches the captured authority explicitly: start releases proven-absent launch authority; stop releases its exact confirmed-stop tombstone; and a normal active runtime with no lifecycle intent releases the exact confirmed runtime absence used by startup or dynamic cleanup. Any other stored action is rejected as corrupt before observation. A failed or stale write and every unresolved observation release neither. In the same live `SqliteStore` generation, replay of the exact owning request in either `effect_inflight` or `unconfirmed` performs one bounded observation; both phases already crossed durable effect authorization and therefore remain live recovery authority. Proven `Gone` reopens only that original effect path; an exact adopted start resumes through the already-owned runtime only when the driver still has attachment retry authority; uncertainty remains unresolved and does not mutate or reissue the effect. A different request remains blocked throughout. After process restart, the reservation generation differs and the old browser command can never enter live effect reentry: proven-gone start/create-start becomes a durable terminal rejection while retaining the created Agent Session for a new lifecycle request, and proven-gone or already-confirmed stop commits its original durable success.

Unix runtime handles and activated lease markers bind the exact current OS boot identity and the same launch-generation token. Linux and Android use the kernel boot UUID; macOS reads `kern.bootsessionuuid` through the maintained safe `sysctl` crate, parses it as a UUID, canonicalizes its representation, and never uses mutable wall-clock or boot-epoch arithmetic. The private boot value is platform-domain-separated and hashed before storage, cached once per server process, and never enters public state. `runtime_handle.rs` is the one strict runtime-v5 decoder for Unix and Windows, while `runtime_absence.rs` is the one owner that may turn a lease observation into `Gone`. Cold Unix recovery requires the handle, durable token, and available marker witnesses to agree on boot and launch generation; a missing lease additionally requires an old-boot handle. Cold Windows recovery requires the Windows handle, durable token, and generation-gone marker to carry the same launch token. A live in-memory slot never accepts `PreviousBoot`: that observation contradicts the current process's launch generation and remains fail-closed. `Unknown`, a current-boot handle paired with an old marker, a substituted token, a malformed value, a source error, a cross-platform handle, or any other mismatch remains `Ambiguous` and retains the exact request, intent, handle, owner, and token. The runtime-handle format and required durable representation changed together, so clean schema 23 rejects schema 22 without conversion, compatibility, or fallback behavior.

Initial reconciliation runs exactly once before network admission. One cancellation-owned reconciler then scans fixed-size cursor pages for every `prepared`, `effect_inflight`, `unconfirmed`, or `effect_applied` reservation created before or after admission, observes with fixed concurrency and timeout bounds, and applies only its captured candidate. The browser command owner and reconciler compete for the same exact in-memory request claim and retain it across their complete asynchronous operation; a loser returns unresolved, closing the former check-then-act race. A changed candidate CAS is discarded. Abandoned pre-effect work is durably rejected without provider I/O, and already-applied stops are checkpointed without another stop. `Gone` terminalizes the captured lifecycle request. An exact same-sidecar `Adopted` or `LeaseUncertain` runtime is first committed under CAS, then reloaded as that recovery operation's own current candidate, stopped only by its exact durable handle/owner/lease generation, committed as `Gone`, and finally released from the confirmed-stop tombstone. This cleanup never retries the original provider effect, including OpenCode session creation. Stop failure, `Ambiguous`, or observation timeout remains fail-closed; cancellation is checked before observation and application but cannot cancel an exact stop after it starts. Runtime adoption never asserts provider-conversation activity.

The watcher pages at most 64 pending lifecycle reservations through one schema-owned partial `(room_id, session_id)` index, excludes blocking provider-turn owners in the same candidate query, observes at most eight candidates concurrently, and bounds each observation to two seconds. Its first periodic tick is delayed by the scan interval because startup already performs the mandatory pre-admission pass. The one-second watcher remains an explicit owner-loss contract: a room or provider task can end after committing external-effect authority but before returning a typed result, and no browser retry is required to recover that durable request. It is not used to validate every ordinary Agent Session repeatedly. Before schema 50, one empty lifecycle page selected up to 64 ordinary sessions and then issued a blocking-turn query, session/room read, and reservation read for each—193 SQL statements for a full inactive page. The current empty page is one covering pending-index scan; exact session and reservation reads occur only for selected unresolved candidates. The partial index contains no completed or rejected history, and `EXPLAIN QUERY PLAN` confirms the pending index, Agent Session primary key, and blocking-turn partial index without a DISTINCT temporary B-tree. Startup still performs the complete pre-admission integrity scan, normal writes preserve session and reservation authority in one transaction, the database permits only its exact process writer, and every live candidate still enters complete authority validation and exact CAS. The accepted trade-off is one small clean-schema index and no continuous out-of-band corruption scan of unrelated sessions; schema 49 is rejected rather than converted or served through compatibility code.

The common provider adapter owns live runtime slots, one supervisor identity, and the provider-neutral room-observation lifecycle independently of room persistence. A driver may know Codex JSONL, Antigravity PTY/ConPTY, or OpenCode HTTP/SSE, but it does not decide room lifecycle, replay, publication, handoff, decline, or recovery semantics. One common outcome is either a bounded public message with an optional exact Agent Session handoff or an explicit supported decline. Executable binding follows the selected runtime shape. Single-file providers on Linux and Android copy verified bytes into a sealed executable `memfd`; other Unix targets execute a byte-verified `0500` copy held inside an explicitly verified private `0700` staging directory; Windows holds the verified image without write/delete sharing. The native Codex executable and its required sibling `codex-code-mode-host` instead form one byte-identified multi-file bundle, so every Unix target stages both files together and the resident Codex driver retains that staging lease through its full lifetime. Staged bytes are hashed directly with the already-open source object's stable identity. Filesystem-staged provider images and Unix private companion helpers share one provider-owned lease root: active runtimes hold their directory lease, while the next creation or owner drop reclaims only unlocked crash directories under the root lock. Linux and Android single-file provider images remain sealed `memfd` objects outside that filesystem owner. The scan is absolutely bounded and unsafe or unknown entries fail closed; this correction adds no timer, sweeper, compatibility cleanup, fallback, or alternate launch path. Linux/Android bind the running server through `/proc/self/exe`; on macOS the desktop supervisor opens the current executable, verifies its device/inode against the process's mapped text vnodes, and launches the server from a private staged copy of those open bytes. The server refuses provider custody without that launch proof, and the guardian then binds the exact running server object before either helper re-executes. Codex uses the provider environment allowlist plus one process-private RoomPortal bearer, `app-server --stdio`, process-local model/effort/tier/sandbox/approval, an exact runtime `untrusted` workspace entry, and private RoomPortal MCP configuration, one bounded JSONL reader, a 256-message/2 MiB aggregate pre-turn notification queue, and default-denied server requests. The runtime trust entry disables workspace `.codex/config.toml`, hooks, and exec policies while leaving the session-flag RoomPortal MCP active; ordinary `AGENTS.md` discovery remains part of the thread contract. On Unix the complete provider launch manifest, including that bearer, crosses an anonymous inherited descriptor rather than argv or the guardian environment; Windows adds it only to the exact owned provider environment. Each RoomPortal generates a fresh unpredictable environment-variable name containing `TOKEN`, so pre-existing user configuration cannot name the bearer as another MCP credential. Codex's built-in sensitive-name exclusions are forced on for model-reachable tool children without replacing either pre-existing or current user filter fields, and this owned app-server disables shell snapshots so the bearer cannot be copied into snapshot state and replayed around the filter. The RoomPortal itself remains in server memory and is exposed through an unguessable process-lifetime path plus independently authenticated bearer on an ephemeral loopback HTTP listener. The locked rmcp streamable-HTTP implementation bounds request bodies. A hard eight-connection semaphore bounds pre-authentication tasks and file descriptors; when full, the accept owner aborts the oldest registry-locked unauthenticated connection and waits for its permit to return before admitting a replacement. Exact constant-time bearer validation and the authenticated transition are one operation under that same registry mutex, so a successfully authenticated connection cannot be evicted between those steps. Unauthenticated requests never consume the separate eight authenticated request permits, every connection has an absolute deadline and disables keep-alive, incomplete headers and bodies expire, stateless JSON responses validate the exact Host and capability path, and cancellation closes every accepted connection with the portal. The bearer never appears in provider argv. Codex explicitly approves only the private server that exposes the exact eight room tools—discussion read, attachment read, lobby-message search, lobby-message context, publish, decline, roll, and choose—and the app-server pump accepts only the exact `agentsassemble_room` MCP-tool elicitation shape. The installed app-server protocol does not provide a trustworthy tool-name field on that elicitation, so the private server boundary and its fixed route set are the approval scope, leaving every other provider request denied. Codex therefore creates no portal sidecar, receives no portal filesystem path, and cannot turn ordinary output into room authority. Process reuse also requires live guardian custody and the exact provider anchor group; Linux/Android then perform a final bounded `/proc/<pid>/stat` check and reject zombie or dead leaders, which retain a PID and PGID until their guardian parent reaps them.

After Codex process initialization, the driver starts or resumes one bounded exact provider thread before reporting the provider session active. A cancelled request remains bound to its method, parameters, and JSON-RPC ID, so retry reads the original response rather than repeating the external effect. A definitive initialize failure is poisoned on that process. The complete attachment response is retained until all original-compatible identity and model locations are normalized; missing, malformed, changed, or conflicting thread identities and any reported model different from the exact configured model fail closed instead of opening or committing another conversation. A poisoned driver can never satisfy runtime reuse: fatal turn poison is stopped under the exact runtime owner and held as a confirmed-stop tombstone until persistence checkpoints it, while other poisoned attachment state returns an explicit restart-required failure.

Antigravity uses one persistent native PTY or managed system-ConPTY session and never invokes print mode. Its workspace hook file is an exclusive managed boundary: any pre-existing project hook refuses launch, the installed document contains only the AgentsAssemble policy hook, and the workspace reference count retains its first quoted absolute guardian-staged or Windows byte-verified locked helper until the last same-workspace session stops. Every provider process receives its own canonical absolute room-helper prefix independently of that shared hook command; its prompt, terminal permission policy, and hook policy require that exact per-session prefix. Portal authority therefore remains per-session while neither a bare basename nor a workspace-shadowed executable receives automatic sandbox bypass. A fresh launch nonce and exact durable turn ID enter every terminal prompt, so concurrent sessions cannot claim the same newly created transcript from equal room text. New attachment remains unbound until exactly one transcript contains that input; multiple candidates fail closed. Cache files, per-poll transcript tails, line sizes, and event counts are all bounded before JSON allocation, while resume reads only new rows from the exact durable conversation path.

Antigravity exposes no correlated native acknowledgement that proves a Ctrl-C has
ended one exact turn while keeping its PTY reusable. The driver therefore never
promotes terminal silence to retained-runtime quiescence: after the exact Ctrl-C
write it poisons that driver, and the common supervisor must stop and reap the
exact H/O/T generation. Only that provider-neutral `RuntimeGone` proof may reach
the room finalizer, which atomically terminalizes the interrupt, requeues once,
clears runtime custody, and leaves the Agent Session cleanly stopped. A future
Antigravity transport can add a genuine correlated retained-runtime proof without
changing persistence, room, protocol, or frontend ownership.

OpenCode uses one persistent loopback `serve --pure` process and native HTTP/SSE session identity. The process gets a private empty configuration root, disables project configuration, default and external plugins, and external skill discovery, while retaining the installed native data store needed for authentication and durable provider sessions. Each runtime receives a fresh high-entropy Basic-auth password in its exact child environment; the strict loopback client requires those credentials and applies them to every JSON and SSE request. On Unix the credential remains inside the anonymous inherited launch manifest and never enters provider argv or the guardian environment. Reserving a port is not readiness authority: before transmitting any HTTP credential, the driver must observe the exact bounded `opencode server listening` line for that selected IPv4 loopback endpoint from the byte-bound child stdout. Every initial or later request then opens a credential-free TCP connection, revalidates exact guardian/child custody after the connection exists, and only sends HTTP through that already connected socket using Hyper without redirects, proxies, or transparent reconnection. If the child died before verification, a replacement listener receives EOF and no request bytes; if it dies after verification, the established socket cannot move to a newly bound listener. Only an authenticated health check over that custody-bound transport permits RoomPortal registration. Because the stable OpenCode HTTP contract does not accept a caller-chosen idempotent session ID, the driver marks session creation uncertain immediately before the first authenticated `POST /session` and clears that authority only after a successful response yields a valid identity. Response loss leaves the runtime observable only as `LeaseUncertain`; neither `Adopted` recovery nor a direct runtime reuse may send a second session-creation request. Assistant responses and SSE events share one provider-specific model parser: every supplied direct or nested alias must be a bounded nonempty string, all aliases must agree, both provider and model components must exist, and both channels must exactly match the configured model before a turn can complete.

Provider turns enter through the common adapter only when durable active-turn, provider-session, runtime-handle, owner, profile, and filesystem authority all match. Ordered turns additionally carry a bounded 20,000-character canonical RoomPortal view, its exact input sequence, and at most 64 unique Agent Session handles. If the Agent Session selected a persona, the persistence owner renders its private card against that same canonical message prefix and freezes the bounded provider-neutral result in the existing assignment envelope; recovery never rereads changed library content. The adapter prepares the portal before provider I/O and accepts completion only after an exact read receipt plus exactly one turn-scoped message publication or supported decline; ordinary assistant final text is ignored as room authority. The portal creates an unguessable turn generation at `begin_observation`. A message or decline may be staged before or after `read_discussion`, matching the original tolerant order, but finalization requires the receipt generation and staged outcome generation to equal that exact active turn generation. Retrying a cancelled caller reuses the exact portal state, including an already staged terminal action. The adapter releases the outer runtime-slot mutex before waiting on a long turn, clones the runtime's inner serialized driver owner, and races driver work against an exact-runtime cancellation token; stop and shutdown can therefore cancel the wait, acquire the driver, and reap the owned process within their existing bound. A Codex turn uses `turn/start` with the original app-server workspace/model/effort/approval/sandbox-policy settings, room-observation orientation, and source metadata. Its response must expose one bounded exact provider-turn identity across original-compatible aliases; a bounded process-lifetime history prevents that identity from being rebound to another logical turn. Every reported model, including a `model/rerouted` destination, must still match the selected model. Output-bearing notifications require both exact thread and turn identities, while malformed unscoped output poisons the turn instead of entering its result. Official `hook/*` control notifications are thread-scoped: their thread identity remains mandatory and their nullable turn identity is compared only when present. Valid unmatched notifications remain under the aggregate queue budget, that budget is decremented when a match is consumed, and completion accepts either an explicit signal or the original final-message-plus-thread-idle grace signal. A cancelled caller continues the same pending request or active turn; a different logical turn cannot replace it.

The room mutation task owns scheduling, while SQLite owns its durable authority.
Each private queue item binds an event ID to its provider delivery semantic—ordered
or ambient observation. Pending and inflight vectors are the only queue record; no
parallel delivery map exists. One assignment moves only the oldest contiguous
prefix with one delivery kind that fits the existing message/view limits, so the
active source is unambiguous and no omitted event can fall behind the cursor.
Combined queue capacity remains 256 unique IDs. Invalid, duplicate, oversized,
missing, wrong-room, or self-origin authority fails rather than being repaired or
truncated.

Ordered routing treats a structured handoff as the earliest direct target; a later
explicit body mention wins, and the final nonactor direct target resolves before
idle eligibility. Undirected work uses director, prior-speaker, sample, and
least-recent policy. Ambient queues every
eligible or runtime-busy nonactor independently. Mode changes never delete
active/inflight work. Multiple active turns are valid after an ordered/ambient
transition, while ordered mode simply blocks a new assignment whenever any active
turn exists. The compatibility-only original continuous relay is not accepted or
executed.

The current clean schema creates typed queue items directly. Older Rust/Python
schema records are not converted or repaired by a compatibility migration.
Lifecycle preparation and startup candidate loading validate current authority
before provider effects. Failure, stop, and restart recovery merge queues in
bounded linear time. An adopted runtime with an active turn requeues inflight
input, clears active authority, disables and detaches
the session, and requires explicit recovery because the provider task was lost.

Provider I/O runs in an owned child task rather than holding the room mutation
loop. Completion re-enters that loop and atomically commits the server-owned
RoomPortal publication or explicit decline, terminal turn event, provider-sync
cursor, idle state, and subsequent assignments. Failure restores inflight work and
clears active authority before exposing a bounded redacted error. Successful or
replayed lifecycle ACKs survive later progression failure. A stale provider result
after stop or authority replacement cannot publish. The browser validates public
session state carried by live events and renders only room-visible final messages.

Human and provider tabletop operations share one bounded parser and server RNG.
Provider calls use a turn-scoped sender that stamps room/session/turn IDs; only the
room actor owns the receiver, persistence, and publication. A same-generation
RoomPortal read receipt is required. Random calls and terminal publication share a
short-held witness mutex with opaque `Queued` to `Committing` reservations.
Terminal publication cannot overtake live reservations, and a reservation cannot
start after terminal publication. The actor revalidates the durable active tuple,
inflight source, current tabletop mode, room write budget, and a durable maximum of
32 successful tool results per turn before commit. Caller cancellation releases no
queued ownership already transferred to the actor, and committing work is removed
only after SQLite commit or rollback. Turn close invalidates queued reservations
but retains a closing tombstone until committing work resolves. No provider-driver
mutex or witness lock is held while waiting on provider I/O, the room actor, or
SQLite.

Every current-profile runtime acquires a generation-tokened exact room/session lease before the provider can spawn. The common adapter installs that lease and its handle/owner identity in a `Launching` slot, then atomically changes Unix `pending:<generation>` to `launching:<generation>` before the driver future can first execute, so cancellation cannot discard custody or admit a replacement generation. Before any anchor or provider creation, the server and guardian overlap shared locks on the exact token-bound lifetime inode: the guardian publishes a bounded readiness record and is blocked on an exact continue record until the server releases its copy. The guardian retains its copy while spawning an exact generation-tagged anchor; inherited pre-exec descriptors and the post-exec tag bridge that spawn until the anchor locks and writes the `unix` marker. It also retains the copy until the stopped provider launcher has inherited its own shared descriptor. Thus process death at any parent-to-guardian-to-anchor/provider handoff cannot leave an unlocked launch marker while an untracked process may still begin. For `pending` or pre-anchor `launching`, absence of both the lifetime lock and exact runtime tag is a bounded `Gone` proof; observation errors remain unknown. A typed launch result distinguishes failures that are still pre-effect from failures after provider creation. Windows keeps its process lock in the supervisor. Unix uses an internal guardian and a stable process-group anchor plus the separate lifetime lease. The guardian runs in a process group independent of the server, becomes the actual parent of a stopped provider launcher, and continues it only after the anchor has atomically changed the locked launch marker to its group identity. The provider inherits both an unguessable generation tag and a shared lock descriptor for the exact lifetime-file inode. A server or desktop loss closes the guardian control pipe and enters the same cleanup path as a requested stop. Guardian shutdown freezes the anchored group before inspecting escaped ownership. Linux and Android mark the guardian as a child subreaper so ordinary `setsid`, double-fork, cleared-environment, and closed-descriptor descendants return to its observable lineage. Linux acquires start-time-revalidated pidfds for group-external descendants, accepts lifetime ownership only when the matching descriptor's bounded `/proc/<pid>/fdinfo` contains a shared `FLOCK` read record, signals only through pidfds, reaps exited children, and waits for the exact group and every captured `(pid,start_time)` to disappear. Android detects escaped lineage but fails closed rather than signaling a numeric PID. macOS registers a kernel `kqueue` fork/exit watcher while the launcher is still stopped; any observed fork or a provider exit before shutdown proof prevents a cleanup receipt. After bounded cleanup the guardian alone changes the exact locked `unix:<generation>:<group>` marker to `gone:<generation>`. A `Launching` slot is released by that exact receipt or by the narrower pre-anchor `Gone` proof; timeout, observation error, unlocked `unix` marker, missing state after anchor activation, or guardian failure remains effect-uncertain with its original handle/owner and blocks replacement. An unlocked `unix` marker is never absence proof, even when its group, tag, and lifetime lock have vanished, so guardian death and ordinary daemon signal stripping cannot create false `Gone`. Confirmed normal-shutdown observations retain the exact lease generation until their absence is durably checkpointed, then remove only that generation. Initialization failure likewise retains exact uncertain authority until shutdown is confirmed. Process start and provider thread attachment remain separate checkpoints.

Executable binding protects selection against path, symlink, updater, in-place-write, and atomic-replacement races within the application's trusted local account. Normal provider daemonization that changes sessions, clears the environment, or closes inherited descriptors remains inside the custody model. A hostile process already executing as the same OS account, or a deliberately hostile provider that delegates work through an unrelated pre-existing same-account process, remains outside this boundary because Unix peers can interfere with account-private processes and files. macOS exposes neither a sealed descriptor-execution path equivalent to Linux `memfd` nor a pidfd-style stable signaling handle, so any provider fork makes cleanup recovery-required instead of claiming success. Network peers, room inputs, and provider output remain untrusted.

## Unix provider health custody

Unix liveness never relies on PID or process-group presence alone. Before any
provider can be reused—and after OpenCode connects a raw socket but before it
constructs or sends authenticated HTTP—the common asynchronous health contract
sends one bounded request over the private guardian control pipe. The guardian
synchronously calls `try_wait` on its exact provider `Child` handle and returns
the expected provider PID, a strictly increasing nonzero request identity, and
`alive` or `exited`. The caller accepts only the one fully correlated response.
A malformed, mismatched, timed-out, cancelled, or unavailable exchange
permanently poisons that observation channel, so buffered output can never become
authority for a later request. An exited macOS zombie is therefore rejected even
while it retains its PID and anchor PGID. The existing group check and
Linux/Android `/proc/<pid>/stat` state check remain additional evidence after
this exact-child proof.

Reporting an exited child or poisoning health observation does not abandon
custody. Normal stop does not require another health probe: it closes the guardian
input and performs the exact-owner stop directly. The guardian retains the handle
and descendants until control-pipe EOF drives the existing cleanup and receipt
path. Consequently, a replacement OpenCode listener reached after the real child
exits sees EOF and zero HTTP bytes, while the existing normal-stop and macOS
fork-history failure semantics remain unchanged.

## Enforced source structure

Cargo crate dependencies enforce direction:

```text
domain <- protocol
domain <- persistence
domain <- provider
domain + protocol + persistence + provider <- server
```

`domain` cannot depend on Tokio, Axum, SQLx, Tower, or provider/network mechanisms. `scripts/check_architecture.py` rejects unowned workspace crates, dependency-direction violations, and every local path dependency outside the explicit owned-crate allowlist. `scripts/check_source_growth.py` signals a structure review at 500 lines, a strong split candidate at 800, and rejects files over 1,000 lines by default. LOC never substitutes for state and invariant ownership: a small file still splits when domain, authority, lifecycle, or change-reason owners differ, while a large file remains intact when one cohesive flow owns its invariant. Reconsider a proposed split when it increases state transfer, public interfaces, inter-module dependency count, or obscuring glue. A concrete generated-code, fixture, or declarative-data file may receive a narrowly reviewed exception only when that need actually exists.
