# Rust Runtime Architecture

Status: current architecture owner

## Definition

AgentsAssemble is one asynchronous Rust runtime serving the existing React and Tauri clients while preserving every actually reachable product flow. It reimplements product semantics, not the Python module tree.

## Authority and cutover

The cutover unit is an authoritative contract owner, not a feature label. Routing selects the owner before a request is admitted. An owner has exactly one authoritative writer: Python before cutover or Rust after cutover.

- Runtime failure never falls through to the other implementation.
- Shadow comparison may read copied input but cannot write durable state, publish events, or affect a response.
- A completed cutover disables the replaced writer.
- Data cutover is atomic: validate the input version, acquire exclusive ownership, migrate transactionally, write the completion marker, then permit Rust writes.
- Rust writes are never followed by automatic Python rollback. Reversal requires an explicit, separately verified migration.

## Stable boundaries

### Room authority

Each room has one bounded command queue and one mutation task. That task serializes validation, domain transition, durable event sequence allocation, command-result persistence, and publication. Different rooms may execute concurrently. A cache, timer, queue, or projection is derived state and never a second room authority.

### Durable commands

`(room_id, principal_id, request_id)` identifies a command attempt. Repeating the same action and canonical payload returns its committed result. Reusing the key with a different action or payload is a conflict.

The mutation transaction orders work as follows:

1. authenticate and authorize the principal;
2. validate the command and current room state;
3. apply the state transition;
4. append any canonical event and allocate its room sequence;
5. persist the correlated command result;
6. commit;
7. publish the event and return the ACK.

No ACK or event is externally visible before commit.

### Room event cursor

Room event `seq` is positive, durable, monotonic, and scoped to one room. A client advances only over contiguous sequences. Initial, resume, gap, and resynchronization responses preserve the existing frontend meaning. Missing or inconsistent ranges fail visibly and trigger authoritative resynchronization.

### Protocol

The Rust protocol crate owns WebSocket envelopes, request correlation, error shape, snapshots, ACKs, and canonical public event types. TypeScript bindings are generated from that owner; handwritten transport copies must not become a second authority.

The existing outer envelope remains compatible:

- client: `subscribe`, `command`, `ping`;
- server: `snapshot`, `event`, `ack`, `nack`, `resync_required`, `pong`.

Action payloads are added only by the slice that implements them.

### Authentication and authorization

A credential resolves once to an `AuthenticatedPrincipal` containing stable identity, room scope, client kind, and server-derived capabilities. Client-supplied roles, operator flags, participant type, or capabilities are never authority.

Opaque, short-lived, one-use WebSocket tickets remain the connection credential. Browser-compatible HTTP ticket issuance stays an adapter while it is a reachable flow and always requires a high-entropy host secret or an authenticated session. Desktop mode cannot start with an empty host secret; Tauri generates it per owned runtime and returns only a one-use ticket plus the validated loopback WebSocket origin to React over IPC.

The local HTTP/WebSocket adapter has explicit resource budgets: admission is bounded immediately after TCP accept, incomplete HTTP headers and request bodies have real deadlines, WebSocket admission is independently bounded, frames/messages stop at 256 KiB, the first subscription has a ten-second deadline, and authenticated ingress has message, byte, and control-frame windows. Binary frames are rejected. Room queue admission never waits and returns `room_busy` when saturated.

### Runtime lifecycle

Tauri owns the local sidecar it starts. The sidecar binds loopback, reports one structured startup record containing the selected address and readiness, and is cancelled and reaped by its owner. The server accepts its host secret only from a private anonymous stdin control pipe; argv and environment credentials do not exist. Tauri keeps that write end open, and EOF makes a running sidecar shut down. A second anonymous control pipe is owned only by Tauri and watched by a separate minimal process; parent death closes it and the watchdog force-kills the sidecar process tree even if the sidecar is stopped and cannot cooperate. Transport failure or an invalid ticket response retires the unhealthy owned child so the next request starts a fresh runtime, while valid application rejection does not restart it. Reusing an existing runtime requires a data-root-scoped ownership record plus a live readiness proof. The lifecycle control plane is separate from room application messages.

### Persistence

SQLite is the local durable authority. The Rust schema owns its version and cutover marker, while an adjacent process-lifetime exclusive writer lease prevents two Rust runtimes from becoming concurrent room authorities. A nonempty database without the Rust owner marker is rejected before any schema write; ownership changes require explicit migration. A command result and its event commit in one transaction. Persistence failure is an error, never an in-memory success.

Room snapshots are read in one SQLite transaction. Their `oldest_seq` and `last_seq` describe the returned event range, not an independently sampled global range. Initial connections receive the newest bounded tail; resumes receive every event after the cursor when it fits, an explicit gap tail otherwise, and the empty `(oldest_seq=0, last_seq=cursor)` boundary when already current.

A resume cursor ahead of durable state produces `resync_required` with the durable latest sequence. The browser transport then clears its local cursor and reconnects for an authoritative initial snapshot; it does not retry the impossible cursor indefinitely.

### Provider catalog and Agent Sessions

The provider crate owns installed-provider discovery, catalog normalization, catalog revisioning, and selection validation. Provider probes run in dedicated owned process trees, inherit only an explicit credential-free environment allowlist, and have bounded time and output. Cancellation kills and reaps the full tree. Windows creates the probe suspended, assigns its Job Object, then resumes it; Unix starts it as a new process-group leader. OpenCode subscription models are restricted to the original managed namespaces. Public catalogs are bounded before publication; the server cannot turn a missing, malformed, oversized, or provenance-invalid catalog into a startable provider. The provider crate does not own rooms or persistence.

An Agent Session's configured and desired state is durable room state. Its public projection deliberately excludes workspace paths, executable paths, filesystem identities, runtime handles, provider conversation identities, lifecycle intents, and the runtime profile key/version; those fields exist only in the private durable record. Exact workspace input is canonicalized without text cleanup. The workspace identity and the executable identity—bound to both its opened filesystem object and complete bytes—are revalidated between a short replay transaction and the final write transaction. The final transaction reauthorizes the room and rechecks command replay before committing, while slow filesystem work never holds the single SQLite writer. Filesystem validation uses a fixed-capacity set of detached standard threads with deadlines; a stalled operation retains its permit until it actually exits but cannot make Tokio runtime shutdown join a blocked filesystem worker. Rooms admit at most 64 sessions so non-event snapshot metadata remains bounded. Live provider processes and their task handles are observed resources owned by one server supervisor. Lifecycle effects begin only from committed intent and report completion through the room mutation owner. Stop confirmation is durably marked before finalization so a retry cannot repeat an already-applied external effect. Replayed commands reuse their durable result before consulting a newer catalog or launching an effect.

Provider diagnostics are untrusted process output. Before an error enters a durable Agent Session, room event, command result, snapshot, or public projection, the shared domain boundary removes local paths, credentials, authorization headers, secret-shaped options and assignments, URL user information, JWTs, and private keys, then applies the field's size limit.

Lifecycle command payloads carry exactly one unchanged Agent Session identifier alias and no unknown keys. The external-effect operation identity binds the exact room, principal, request ID, and action; only that operation may resume or finalize its prepared work, and an opposite lifecycle command cannot replace it. Before an incomplete lifecycle command leaves its write transaction, a room/principal/request reservation also binds its action, payload hash, Agent Session, and operation ID. That reservation survives recoverable failures and restart, so the request ID cannot be rebound to a second effect; a completed command replaces it with the durable command result in the same transaction. Process reuse and provider-conversation reuse are independent observations; an app-server process is not proof that a Codex thread is active, and a reused conversation must retain its exact durable identity. Ambiguous shutdown becomes a durable redacted `disconnected` error with recovery required, never a successful stop. Its private runtime handle remains only as exact retry correlation and is not accepted as proof of process liveness. Before network admission after server startup, stale live-looking session states are either adopted by an exact-handle owner or, while cross-process adoption is unavailable, synchronously disconnected with their provider conversation identity retained for explicit resume. A confirmed `stop/effect_applied` checkpoint is preserved across restart so its exact originating command can finalize without repeating the external stop.

## Enforced source structure

Cargo crate dependencies enforce direction:

```text
domain <- protocol
domain <- persistence
domain <- provider
domain + protocol + persistence + provider <- server
```

`domain` cannot depend on Tokio, Axum, SQLx, Tower, or provider/network mechanisms. `scripts/check_architecture.py` rejects unowned workspace crates, dependency-direction violations, and every local path dependency outside the explicit owned-crate allowlist. `scripts/check_source_growth.py` applies the inherited 800-line source ceiling to handwritten and generated source alike, so a filename cannot bypass the gate. An exception requires an explicit path, ceiling, and cohesive reason showing why splitting would break an essential single owner.
