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

Opaque, short-lived, one-use WebSocket tickets remain the connection credential. Browser-compatible HTTP ticket issuance stays an adapter while it is a reachable flow and always requires a high-entropy host secret or an authenticated session. Desktop mode cannot start with an empty host secret; Tauri generates it per runtime and should return only the one-use ticket to React.

The local WebSocket adapter has explicit resource budgets: bounded connection admission, 256 KiB frames/messages, a ten-second first-subscription deadline, an idle deadline, bounded ingress messages/bytes, and a non-waiting room queue admission that returns `room_busy` when saturated.

### Runtime lifecycle

Tauri owns the local sidecar it starts. The sidecar binds loopback, reports one structured startup record containing the selected address and readiness, and is cancelled and reaped by its owner. Reusing an existing runtime requires a data-root-scoped ownership record plus a live readiness proof. The lifecycle control plane is separate from room application messages.

### Persistence

SQLite is the local durable authority. The Rust schema owns its version and cutover marker, while an adjacent process-lifetime exclusive writer lease prevents two Rust runtimes from becoming concurrent room authorities. A nonempty database without the Rust owner marker is rejected before any schema write; ownership changes require explicit migration. A command result and its event commit in one transaction. Persistence failure is an error, never an in-memory success.

Room snapshots are read in one SQLite transaction. Their `oldest_seq` and `last_seq` describe the returned event range, not an independently sampled global range. Initial connections receive the newest bounded tail; resumes receive every event after the cursor when it fits, an explicit gap tail otherwise, and the empty `(oldest_seq=0, last_seq=cursor)` boundary when already current.

A resume cursor ahead of durable state produces `resync_required` with the durable latest sequence. The browser transport then clears its local cursor and reconnects for an authoritative initial snapshot; it does not retry the impossible cursor indefinitely.

## Enforced source structure

Cargo crate dependencies enforce direction:

```text
domain <- protocol
domain <- persistence
domain + protocol + persistence <- server
```

`domain` cannot depend on Tokio, Axum, SQLx, Tower, or provider/network mechanisms. `scripts/check_architecture.py` rejects unowned workspace crates and dependency-direction violations. `scripts/check_source_growth.py` applies the inherited 800-line source ceiling. An exception requires an explicit path, ceiling, and cohesive reason showing why splitting would break an essential single owner.
