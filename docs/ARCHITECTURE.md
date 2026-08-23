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
optimistic ordering assumption into state authority.

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
- server: `snapshot`, `event`, `ack`, `nack`, `resync_required`, `pong`.

Action payloads are added only by the slice that implements them.

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

### Authentication and authorization

A credential resolves once to an `AuthenticatedPrincipal` containing stable identity, room scope, client kind, and server-derived capabilities. Client-supplied roles, operator flags, participant type, or capabilities are never authority.

Opaque, short-lived, one-use WebSocket tickets remain the connection credential. Browser-compatible HTTP ticket issuance stays an adapter while it is a reachable flow and always requires a high-entropy host secret or an authenticated session. Desktop mode cannot start with an empty host secret; Tauri generates it per owned runtime and returns only a one-use ticket plus the validated loopback WebSocket origin to React over IPC.

The local HTTP/WebSocket adapter has explicit resource budgets: admission is bounded immediately after TCP accept, incomplete HTTP headers and request bodies have real deadlines, WebSocket admission is independently bounded, frames/messages stop at 256 KiB, the first subscription has a ten-second deadline, and authenticated ingress has message, byte, and control-frame windows. Binary frames are rejected. Room queue admission never waits and returns `room_busy` when saturated.

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

A committed human-profile revision durably schedules every affected room
projection update. Per-room application state and durable events make partial
progress restartable; an in-memory broadcast loop is not completion. The HTTP
result semantics—whether success means the canonical profile commit or completion
of all current room checkpoints—must be fixed by the active identity slice before
implementation. In either case, retry of the same mutation reuses the same profile
revision and cannot create duplicate revisions. A legacy local profile may be
imported only by an explicit one-time migration with a completion marker, never by
an ongoing read fallback.

### Runtime lifecycle

Tauri owns the local sidecar it starts. The sidecar binds loopback, reports one structured startup record containing the selected address and readiness, and is cancelled and reaped by its owner. The server accepts its host secret only from a private anonymous stdin control pipe; argv and environment credentials do not exist. Tauri keeps that write end open, and EOF makes a running sidecar shut down. A second anonymous control pipe is owned only by Tauri and watched by a separate minimal process; parent death closes it and the watchdog force-kills the sidecar process tree even if the sidecar is stopped and cannot cooperate. Transport failure or an invalid ticket response retires the unhealthy owned child so the next request starts a fresh runtime, while valid application rejection does not restart it. Reusing an existing runtime requires a data-root-scoped ownership record plus a live readiness proof. The lifecycle control plane is separate from room application messages.

### Persistence

SQLite is the local durable authority. The Rust schema owns its version and cutover marker, while an adjacent process-lifetime exclusive writer lease prevents two Rust runtimes from becoming concurrent room authorities. A nonempty database without the Rust owner marker is rejected before any schema write; ownership changes require explicit migration. A command result and its event commit in one transaction. Persistence failure is an error, never an in-memory success.

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

An Agent Session's configured and desired state is durable room state. Its public projection deliberately excludes workspace paths, executable paths, filesystem identities, runtime handles, provider conversation identities, lifecycle intents, and the runtime profile key/version; those fields exist only in the private durable record. Exact workspace input is canonicalized without text cleanup. The workspace identity and the executable identity—bound to both its opened filesystem object and complete bytes—are revalidated between a short replay transaction and the final write transaction. The final transaction reauthorizes the room and rechecks command replay before committing, while slow filesystem work never holds the single SQLite writer. Filesystem validation uses a fixed-capacity set of detached standard threads with deadlines; a stalled operation retains its permit until it actually exits but cannot make Tokio runtime shutdown join a blocked filesystem worker. Rooms admit at most 64 sessions so non-event snapshot metadata remains bounded. Live provider processes and their task handles are observed resources owned by one server supervisor. Lifecycle effects begin only from committed intent and report completion through the room mutation owner. Stop confirmation is durably marked before finalization so a retry cannot repeat an already-applied external effect. Replayed commands reuse their durable result before consulting a newer catalog or launching an effect.

`agent.configure` follows the same two-phase authority rule as creation without changing Agent Session identity. The room owner first authorizes and loads one exact stopped private profile, then the provider catalog merges only the client-selectable runtime controls with that stored provider/workspace authority. The final transaction reauthorizes, rechecks replay and the original profile key, revalidates the selected filesystem identities, and atomically replaces both the private profile and public projection. Running, active-turn, owned-handle, and lifecycle-intent states fail closed; a successful profile save upgrades the durable profile version and emits a canonical `agent_session_state` event. Empty string values from the copied React controls remain compatible for optional controls and `max_output_tokens` while provider/runtime/transport identity stays server-owned.

Provider diagnostics are untrusted process output. Before an error enters a durable Agent Session, room event, command result, snapshot, or public projection, the shared domain boundary removes local paths, credentials, authorization headers, secret-shaped options and assignments, URL user information, JWTs, and private keys, then applies the field's size limit. The common provider adapter exposes only stable public error codes and messages; protocol payloads and stderr never cross directly into room authority.

Lifecycle command payloads carry exactly one unchanged Agent Session identifier alias and no unknown keys. The external-effect operation identity binds the exact room, principal, request ID, and action; only that operation may resume or finalize its prepared work, and an opposite lifecycle command cannot replace it. Before an incomplete lifecycle command leaves its write transaction, a room/principal/request reservation also binds its action, payload hash, Agent Session, operation ID, and phase. Every non-lifecycle command admission checks that same request namespace, so the reservation is the in-flight or terminal-failure phase of one global command authority rather than a second authority beside completed results. It survives recoverable failures and restart, and a completed command replaces it with the durable command result in the same transaction. A pre-reservation schema containing an incomplete lifecycle intent cannot reconstruct that binding and fails migration closed. Process reuse and provider-conversation reuse are independent observations; an app-server process is not proof that a Codex thread is active, and a reused conversation must retain its exact durable identity. Every runtime handle is paired with its private supervisor-instance owner. Ambiguous start or shutdown retains its exact operation and any observed handle rather than turning uncertainty into success or a new generation. A confirmed stop is held as an in-memory tombstone until persistence checkpoints `effect_applied`, so a checkpoint retry cannot repeat the external stop. Persistence never emits a stop effect with a missing handle or owner and never accepts a DB-only reused start.

Startup reconciliation is a three-owner protocol: persistence loads a complete private candidate outside process I/O, the common provider supervisor reports `Adopted`, `Gone`, `LeaseUncertain`, or `Ambiguous`, and persistence validates and commits the lifecycle-specific transition with an exact candidate CAS. Drivers report facts and never choose `owner_lost`. A gone pending stop becomes `effect_applied`; a confirmed checkpoint never repeats an effect; an adoptable runtime is rebound to the current supervisor only after filesystem authority revalidation; an exact uncertain lease stays recovery-locked with its handle/owner; ambiguous start intent remains locked against duplicate spawn; and ambiguous or foreign stop ownership becomes terminal `owner_lost`. Runtime adoption never asserts provider-conversation activity. Network admission occurs only after every loaded candidate has committed a current observation.

The common provider adapter owns live runtime slots, one supervisor identity, and the provider-neutral room-observation lifecycle independently of room persistence. A driver may know Codex JSONL, Antigravity PTY/ConPTY, or OpenCode HTTP/SSE, but it does not decide room lifecycle, replay, publication, handoff, decline, or recovery semantics. One common outcome is either a bounded public message with an optional exact Agent Session handoff or an explicit supported decline. The Codex driver binds the verified executable object through process creation: Linux and Android copy the verified bytes into a sealed executable `memfd`, other Unix targets execute a byte-verified `0500` copy held inside an explicitly verified private `0700` staging directory, and Windows holds the verified image without write/delete sharing. Staged bytes are hashed directly with the already-open source object's stable identity. Linux/Android bind the running server through `/proc/self/exe`; on macOS the desktop supervisor opens the current executable, verifies its device/inode against the process's mapped text vnodes, and launches the server from a private staged copy of those open bytes. The server refuses provider custody without that launch proof, and the guardian then binds the exact running server object before either helper re-executes. Codex uses the provider environment allowlist plus one process-private RoomPortal bearer, `app-server --stdio`, process-local model/effort/tier/sandbox/approval, an exact runtime `untrusted` workspace entry, and private RoomPortal MCP configuration, one bounded JSONL reader, a 256-message/2 MiB aggregate pre-turn notification queue, and default-denied server requests. The runtime trust entry disables workspace `.codex/config.toml`, hooks, and exec policies while leaving the session-flag RoomPortal MCP active; ordinary `AGENTS.md` discovery remains part of the thread contract. On Unix the complete provider launch manifest, including that bearer, crosses an anonymous inherited descriptor rather than argv or the guardian environment; Windows adds it only to the exact owned provider environment. Each RoomPortal generates a fresh unpredictable environment-variable name containing `TOKEN`, so pre-existing user configuration cannot name the bearer as another MCP credential. Codex's built-in sensitive-name exclusions are forced on for model-reachable tool children without replacing either legacy or canonical user filter fields, and this owned app-server disables shell snapshots so the bearer cannot be copied into snapshot state and replayed around the filter. The RoomPortal itself remains in server memory and is exposed through an unguessable process-lifetime path plus independently authenticated bearer on an ephemeral loopback HTTP listener. The locked rmcp streamable-HTTP implementation bounds request bodies. A hard eight-connection semaphore bounds pre-authentication tasks and file descriptors; when full, the accept owner aborts the oldest registry-locked unauthenticated connection and waits for its permit to return before admitting a replacement. Exact constant-time bearer validation and the authenticated transition are one operation under that same registry mutex, so a successfully authenticated connection cannot be evicted between those steps. Unauthenticated requests never consume the separate eight authenticated request permits, every connection has an absolute deadline and disables keep-alive, incomplete headers and bodies expire, stateless JSON responses validate the exact Host and capability path, and cancellation closes every accepted connection with the portal. The bearer never appears in provider argv. Codex explicitly approves only this server's three room tools and the app-server pump accepts only the exact `agentsassemble_room` MCP-tool elicitation shape, leaving every other provider request denied. Codex therefore creates no portal sidecar, receives no portal filesystem path, and cannot turn ordinary output into room authority. Process reuse also requires live guardian custody and the exact provider anchor group; Linux/Android then perform a final bounded `/proc/<pid>/stat` check and reject zombie or dead leaders, which retain a PID and PGID until their guardian parent reaps them.

After Codex process initialization, the driver starts or resumes one bounded exact provider thread before reporting the provider session active. A cancelled request remains bound to its method, parameters, and JSON-RPC ID, so retry reads the original response rather than repeating the external effect. A definitive initialize failure is poisoned on that process. The complete attachment response is retained until all original-compatible identity and model locations are normalized; missing, malformed, changed, or conflicting thread identities and any reported model different from the exact configured model fail closed instead of opening or committing another conversation. A poisoned driver can never satisfy runtime reuse: fatal turn poison is stopped under the exact runtime owner and held as a confirmed-stop tombstone until persistence checkpoints it, while other poisoned attachment state returns an explicit restart-required failure.

Antigravity uses one persistent native PTY or managed system-ConPTY session and never invokes print mode. Its workspace hook file is an exclusive managed boundary: any pre-existing project hook refuses launch, the installed document contains only the AgentsAssemble policy hook, and the workspace reference count retains its first quoted absolute guardian-staged or Windows byte-verified locked helper until the last same-workspace session stops. Every provider process receives its own canonical absolute room-helper prefix independently of that shared hook command; its prompt, terminal permission policy, and hook policy require that exact per-session prefix. Portal authority therefore remains per-session while neither a bare basename nor a workspace-shadowed executable receives automatic sandbox bypass. A fresh launch nonce and exact durable turn ID enter every terminal prompt, so concurrent sessions cannot claim the same newly created transcript from equal room text. New attachment remains unbound until exactly one transcript contains that input; multiple candidates fail closed. Cache files, per-poll transcript tails, line sizes, and event counts are all bounded before JSON allocation, while resume reads only new rows from the exact durable conversation path.

OpenCode uses one persistent loopback `serve --pure` process and native HTTP/SSE session identity. The process gets a private empty configuration root, disables project configuration, default and external plugins, and external skill discovery, while retaining the installed native data store needed for authentication and durable provider sessions. Each runtime receives a fresh high-entropy Basic-auth password in its exact child environment; the strict loopback client requires those credentials and applies them to every JSON and SSE request. On Unix the credential remains inside the anonymous inherited launch manifest and never enters provider argv or the guardian environment. Reserving a port is not readiness authority: before transmitting any HTTP credential, the driver must observe the exact bounded `opencode server listening` line for that selected IPv4 loopback endpoint from the byte-bound child stdout. Every initial or later request then opens a credential-free TCP connection, revalidates exact guardian/child custody after the connection exists, and only sends HTTP through that already connected socket using Hyper without redirects, proxies, or transparent reconnection. If the child died before verification, a replacement listener receives EOF and no request bytes; if it dies after verification, the established socket cannot move to a newly bound listener. Only an authenticated health check over that custody-bound transport permits RoomPortal registration. Assistant responses and SSE events share one provider-specific model parser: every supplied direct or nested alias must be a bounded nonempty string, all aliases must agree, both provider and model components must exist, and both channels must exactly match the configured model before a turn can complete.

Provider turns enter through the common adapter only when durable active-turn, provider-session, runtime-handle, owner, profile, and filesystem authority all match. Ordered turns additionally carry a bounded 20,000-character canonical RoomPortal view, its exact input sequence, and at most 64 unique Agent Session handles. The adapter prepares the portal before provider I/O and accepts completion only after an exact read receipt plus exactly one turn-scoped message publication or supported decline; ordinary assistant final text is ignored as room authority. The portal creates an unguessable turn generation at `begin_observation`. A message or decline may be staged before or after `read_discussion`, matching the original tolerant order, but finalization requires the receipt generation and staged outcome generation to equal that exact active turn generation. Retrying a cancelled caller reuses the exact portal state, including an already staged terminal action. The adapter releases the outer runtime-slot mutex before waiting on a long turn, clones the runtime's inner serialized driver owner, and races driver work against an exact-runtime cancellation token; stop and shutdown can therefore cancel the wait, acquire the driver, and reap the owned process within their existing bound. A Codex turn uses `turn/start` with the original app-server workspace/model/effort/approval/sandbox-policy settings, room-observation orientation, and source metadata. Its response must expose one bounded exact provider-turn identity across original-compatible aliases; a bounded process-lifetime history prevents that identity from being rebound to another logical turn. Every reported model, including a `model/rerouted` destination, must still match the selected model. Output-bearing notifications require both exact thread and turn identities, while malformed unscoped output poisons the turn instead of entering its result. Official `hook/*` control notifications are thread-scoped: their thread identity remains mandatory and their nullable turn identity is compared only when present. Valid unmatched notifications remain under the aggregate queue budget, that budget is decremented when a match is consumed, and completion accepts either an explicit signal or the original final-message-plus-thread-idle grace signal. A cancelled caller continues the same pending request or active turn; a different logical turn cannot replace it.

The room mutation task owns ordered-floor serialization, while SQLite owns its durable authority. A source message, unique ordered target queueing, and an immediately available assignment commit together. Direct target resolution considers every configured session before eligibility, so stopped or detached targets retain their queued floor unless kicked or muted; an undirected agent-origin update prefers an eligible director before sampled least-recent-speaker selection. At most one session in a room may carry an exact busy active-turn tuple; `busy` with empty or partial active authority is invalid rather than clear. Its private record binds pending and inflight event IDs, the exact source event, and the canonical provider-input cursor; the combined queue is capped at 256 unique IDs and overflow rejects the source transaction. Assignment moves only the oldest complete prefix that fits 50 messages and the canonical view bound, so no omitted queued event can fall behind an advanced cursor. Lifecycle preparation and startup candidate loading validate this authority before any provider effect. Failure, stop, and restart recovery merge it with one bounded `HashSet` pass; oversized, empty, or duplicate state returns an error instead of truncating queued messages. An adopted runtime with an active turn requeues inflight input, clears active authority, disables and detaches the session, and requires explicit recovery because the provider task was lost. Public `turn_started`, `turn_state`, and `agent_session_state` events are derived from the private record. Provider I/O runs in an owned child task rather than holding the room mutation loop. Completion re-enters that loop and atomically commits the server-owned RoomPortal publication or explicit decline, terminal turn event, provider-sync cursor, idle state, and next queued assignment. Failure restores inflight IDs to pending and clears active authority before exposing a bounded redacted error. Successful or replayed start and stop paths preserve their already committed ACK if later floor progression fails, report that progression failure only through stable diagnostics, and attempt recovered pending work. A stale provider result after stop or authority replacement cannot publish. The browser validates public session state carried by live events, upserts it independently, and renders only room-visible final messages rather than internal coordination events.

Every current-profile runtime acquires a generation-tokened exact room/session lease before the provider can spawn. The common adapter installs that lease and its handle/owner identity in a `Launching` slot, then atomically changes Unix `pending:<generation>` to `launching:<generation>` before the driver future can first execute, so cancellation cannot discard custody or admit a replacement generation. Unlocked `launching` is never absence proof; `pending` remains reserved for work that provably has not entered a driver. A typed launch result distinguishes failures that are still pre-effect from failures after provider creation. Windows keeps its process lock in the supervisor. Unix uses an internal guardian and a stable process-group anchor plus a separate lifetime lease. The guardian runs in a process group independent of the server, becomes the actual parent of a stopped provider launcher, and continues it only after the anchor has atomically changed the locked launch marker to its group identity. The provider inherits both an unguessable generation tag and a shared lock descriptor for the exact lifetime-file inode. A server or desktop loss closes the guardian control pipe and enters the same cleanup path as a requested stop. Guardian shutdown freezes the anchored group before inspecting escaped ownership. Linux and Android mark the guardian as a child subreaper so ordinary `setsid`, double-fork, cleared-environment, and closed-descriptor descendants return to its observable lineage. Linux acquires start-time-revalidated pidfds for group-external descendants, accepts lifetime ownership only when the matching descriptor's bounded `/proc/<pid>/fdinfo` contains a shared `FLOCK` record, signals only through pidfds, reaps exited children, and waits for the exact group and every captured `(pid,start_time)` to disappear. Android detects escaped lineage but fails closed rather than signaling a numeric PID. macOS registers a kernel `kqueue` fork/exit watcher while the launcher is still stopped; any observed fork or a provider exit before shutdown proof prevents a cleanup receipt. After bounded cleanup the guardian alone changes the exact locked `unix:<generation>:<group>` marker to `gone:<generation>`. A `Launching` slot is released only by that exact receipt, never by the broader lease observation used for truly pre-effect recovery. A failure after guardian spawn is safe only when the receipt is observed; timeout, non-receipt, missing state, or guardian failure remains effect-uncertain with its original handle/owner and blocks replacement. An unlocked `unix` marker is never absence proof, even when its group, tag, and lifetime lock have vanished, so guardian death and ordinary daemon signal stripping cannot create false `Gone`. Confirmed normal-shutdown observations retain the exact lease generation until their absence is durably checkpointed, then remove only that generation. Initialization failure likewise retains exact uncertain authority until shutdown is confirmed. Process start and provider thread attachment remain separate checkpoints.

Executable binding protects selection against path, symlink, updater, in-place-write, and atomic-replacement races within the application's trusted local account. Normal provider daemonization that changes sessions, clears the environment, or closes inherited descriptors remains inside the custody model. A hostile process already executing as the same OS account, or a deliberately hostile provider that delegates work through an unrelated pre-existing same-account process, remains outside this boundary because Unix peers can interfere with account-private processes and files. macOS exposes neither a sealed descriptor-execution path equivalent to Linux `memfd` nor a pidfd-style stable signaling handle, so any provider fork makes cleanup recovery-required instead of claiming success. Network peers, room inputs, and provider output remain untrusted.

## Enforced source structure

Cargo crate dependencies enforce direction:

```text
domain <- protocol
domain <- persistence
domain <- provider
domain + protocol + persistence + provider <- server
```

`domain` cannot depend on Tokio, Axum, SQLx, Tower, or provider/network mechanisms. `scripts/check_architecture.py` rejects unowned workspace crates, dependency-direction violations, and every local path dependency outside the explicit owned-crate allowlist. `scripts/check_source_growth.py` applies the inherited 800-line source ceiling to handwritten and generated source alike, so a filename cannot bypass the gate. An exception requires an explicit path, ceiling, and cohesive reason showing why splitting would break an essential single owner.
