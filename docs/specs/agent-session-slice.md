# Agent Session vertical slice

Status: published implementation owner; idle pause/resume packaged verification and manual review complete

## Definition

A host selects an installed provider/model from the authoritative live catalog, creates a durable Agent Session, and can ultimately start that same session so its canonical room-context reply is published back into the room.

## Public baseline and implementation order

The comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust commit
`5c31ccf1cf33146a4e91431df7400b8508aca82d`. Local uncommitted work is
not completion evidence. At that Rust baseline one durable server-owned
`agent.create(start=false|true)` reservation covers creation and optional start,
the copied desktop sends only that command, and snapshot, catch-up, resync, and
live fanout use the same authenticated-viewer projector. The exact published
real-provider and copied-UI evidence is recorded in `docs/VERIFICATION.md`; web
review and the separate Daybreak Blue high manual-security re-review are complete.

Implementation order is mandatory:

1. correct the existing owner documents and freeze frontend provenance/parity gates;
2. establish the minimum authenticated-principal, viewer-projection, public-result,
   and common application-command boundary in this document;
3. implement the server-owned Agent Session command and lifecycle contract;
4. verify the copied UI, crash/replay cases, exact provider matrix, restart, and
   owned-resource cleanup before declaring the slice complete.

## Idle pause/resume extension

The original reachable client exposes `agent.pause` only for an idle connected Agent Session and
`agent.resume` for a paused session. Pause disables scheduling while preserving the exact provider
process and provider conversation; resume re-enables that same resident runtime and advances queued
room work. This extension owns only that state-preserving pair. Busy-turn `agent.interrupt`, stopped
runtime resume through provider launch, re-add, kick, and provider-request resolution retain their
separate existing or future owners.

- Both actions require the current server-derived `agent.control` capability, an active-room
  principal, and a payload containing exactly one canonical Agent Session identifier. A browser
  display condition never creates authority, and admitted non-operator or Agent Bridge principals
  cannot use either action.
- `agent.pause` accepts only a complete idle/enabled resident session with empty active and inflight
  turn authority, no lifecycle intent, and the exact durable runtime handle/owner/lease and active
  provider-session identity. It atomically changes only `enabled=false` and
  `runtime_status=paused`, appends the canonical complete `agent_session_state`, and stores the exact
  replay result. It sends no provider request and does not stop, detach, replace, or reattach the
  runtime.
- A paused session continues to own newly queued canonical inputs but is ineligible for assignment,
  so it consumes no new provider turn while paused. The existing combined 256-input ceiling remains
  the sole queue bound.
- `agent.resume` takes the state-only path only for that exact paused resident state. It preserves
  every runtime and provider identity, atomically sets `enabled=true` and `runtime_status=idle`,
  appends the canonical state event/result, and lets the existing ordered-floor owner assign queued
  work after commit. Every non-paused resume retains the current stopped-runtime launch contract and
  its durable external-effect reservation; the client does not choose between the paths.
- Same request/action/payload replay returns the committed result without another event or state
  transition; changed request reuse conflicts. Invalid, active-turn, stopping, recovery-required,
  incomplete-runtime, or lifecycle-owned state fails before mutation. No schema, migration,
  compatibility branch, provider-specific path, background task, polling, heartbeat, timer, retry,
  or fallback is added.
- A fresh pause or state-only resume performs one command-triggered proof against the adapter's
  exact live slot. Handle, supervisor owner, lease generation, runtime profile, driver liveness,
  and safe provider-session attachment must match the durable preflight, and the mutation
  transaction compares that same identity again before committing. A replay returns its existing
  result before live proof. The proof does not revalidate filesystem selection authority, poll,
  retry, or create a second runtime owner; unavailable or uncertain residency rejects the fresh
  command without claiming process preservation or reuse.
- Verification must prove exact replay/conflict and invalid-state rejection, no assignment while
  paused, queued-work assignment after resume, unchanged runtime/provider identities across reload
  and restart, copied-control surface gating, and real packaged Codex Terra, Antigravity Flash, and
  OpenCode Muse Spark process-preserving flows. Resource evidence compares the resident process set
  and idle CPU/RSS before pause, during pause, and after resume without claiming an improvement from
  point samples.

The local pause/resume candidate passed the complete repository gate and the copied packaged UI for
all three exact providers. Each provider completed one addressed turn, retained the same guardian,
anchor, provider process, runtime handle, and provider-conversation identity across idle pause, left
one paused direct mention pending with no inflight turn, and completed that queued turn after resume.
The original Codex verification then exposed its official internally spawned code-mode host outside
the guardian group, so the fail-closed stop correctly withheld a cleanup receipt. The correction
binds and stages that official companion with the selected Codex executable, starts it as an explicit
same-group child before `app-server`, and supplies only its validated loopback endpoint to the
official `--code-mode-host` option. A fresh packaged Terra turn and UI Stop now produce confirmed
cleanup with no recovery state. Exact packaged evidence and cleanup are recorded in
`docs/VERIFICATION.md`. Critical-web and Daybreaker manual review approved pushed HEAD `0821b0a`
and cumulative `a340a31..0821b0a` at C0/H0/M0/L0; no automated scan was run.

## Required slice contract

- Codex lifecycle start binds its official executable and code-mode companion as one byte-identified bundle. The guardian's stopped launcher starts the companion as a same-anchor-group child on one validated canonical IPv4-loopback WebSocket endpoint, passes only that endpoint through the official `--code-mode-host` option, and then execs the persistent `app-server --stdio`. The companion receives a sanitized environment and runtime custody token but no RoomPortal bearer; unexpected inherited protocol descriptors are replaced with `/dev/null`. Missing or changed companion authority, invalid readiness, early exit, or custody change fails the launch without a fallback.
- Provider options come from bounded probes of the installed provider CLIs. Every probe runs in its own owned process tree with a credential-free environment allowlist, a ten-second deadline, and bounded output; cancellation and shutdown kill and reap the whole tree. Windows probes are created suspended, assigned to their Job Object, and only then resumed, so no descendant can escape before ownership attaches. A session can be created only from a `ready` catalog entry and the exact current `catalog_revision`; a stale, unavailable, unlisted, oversized, or internally inconsistent selection fails visibly.
- OpenCode subscription discovery accepts only syntactically valid model IDs in the original managed `opencode` and `opencode-go` namespaces. Other namespaces never become startable subscription authority.
- `agent.create` requires the server-derived `agent.control` capability evaluated from the current principal and durable room state when the command runs. Client-supplied ownership, participant role, provider command, executable, runtime kind, transport, process identity, capability, or operator status is ignored as authority.
- `agent.create(start=false)` creates one stopped Agent Session. The creation transaction appends one complete canonical `agent_session_created` projection containing that exact detached room Participant and public stopped Agent Session. Every live viewer upserts both authorities from the event; the issuer ACK is not a private UI authority, and reconnect is not required to reveal the session. `agent.create(start=true)` is one server-owned client command whose identity covers durable creation and the optional lifecycle start intent; its same creation event contains the exact public `starting/enabled` session committed with that intent before provider launch. Later success or provider failure transitions the same already-visible session. Snapshot participant/session arrays are the sole current-state authority during initial, resume, and resync delivery; accompanying events supply timeline/history only and are never replayed over those arrays. The client never implements creation as create, start, and resync commands. Explicit `agent.start` remains available for an already-existing stopped session.
- `(room_id, principal_id, request_id)` remains the command identity. Its first durable commit binds the action, canonical payload hash, derived Agent Session ID, `start` intent, and current phase before a crash can leave creation without its owning reservation. A same-payload retry returns or resumes the original result and never creates a second session, lifecycle intent, or runtime; changed payload reuse conflicts. Internally derived create/start operation IDs are domain-separated details, not client orchestration.
- That first durable commit is also an independent publication boundary. Its newly appended creation event is offered to current room subscribers before an optional provider launch begins; a long or failed launch cannot leave existing live viewers behind a concurrent snapshot viewer. Success, safe failure, or uncertainty events are published after their own commits. Resume and replay may reconstruct older events for the command result, but only events newly appended by the current transaction are eligible for live publication.
- The public create result preserves the original observable created-session, participant, and optional-start result fields actually consumed by the frontend. The contract and tests distinguish creation success with stopped state, creation plus confirmed start, explicit start failure after creation, and an uncertain start requiring recovery; an internal state-machine improvement cannot silently narrow that result shape.
- The durable Agent Session owns desired/configured state. A provider supervisor owns live subprocesses and reports observed transitions through the room authority; process presence, caches, and task handles are never parallel session authority.
- A stopped server-owned session is restorable from its complete private durable runtime profile. Public snapshots, ACKs, events, replay results, and generated TypeScript never expose its workspace, executable, filesystem identities, or runtime profile key. Restart never silently substitutes a provider, model, workspace, transport, new provider conversation, or Python implementation.
- A stopped session accepts the copied frontend's `agent.configure` runtime-settings payload only after exact catalog revision, provider identity, option relation, workspace, and executable revalidation. The command preserves the Agent Session ID, rejects every live/owned/turn-active/lifecycle-active state, upgrades an older private profile version, commits its public projection and canonical state event atomically, and replays without consulting a newer catalog. Empty optional control strings from the original React form are normalized without granting a new server-owned field.
- Workspace input is an exact path, not an identifier: it is never trimmed or cleaned before canonicalization. Selection records a stable workspace identity and an executable identity bound to both the opened filesystem object and all executable bytes. Persistence first performs a short replay check, reopens and revalidates both authorities without holding the SQLite writer, then opens the final transaction and rechecks room authority plus command replay before commit. Potentially stalled filesystem work runs in capacity-bounded detached workers with a ten-second deadline, so a stalled mount fails closed without joining Tokio runtime shutdown, and each stalled worker continues consuming capacity until it really exits.
- Public provider catalogs are capped at 48 KiB, individual providers at 16 KiB and 256 options, and rooms at 64 Agent Sessions. Oversized authority fails closed before it can make the fixed 256 KiB WebSocket snapshot impossible.
- Provider output becomes a canonical durable `message_final` event attributed to the Agent Session. No ACK or room event is published before its transaction commits.
- Runtime transport preserves the original resident boundaries: Codex owns one persistent `app-server` stdio JSONL process and thread identity; Antigravity owns one persistent PTY session on Unix or managed system-ConPTY session on Windows; OpenCode owns one local HTTP/SSE runtime and session identity. Every OpenCode JSON and SSE request uses one fresh runtime-private Basic-auth capability. No HTTP credential or RoomPortal bearer is transmitted until the exact byte-bound child stdout reports the selected IPv4 loopback endpoint. For every initial or later request, the driver connects without transmitting bytes, revalidates exact guardian/child liveness after that socket exists, and sends authenticated Hyper HTTP only through that verified connection with transparent reconnection disabled. A process that replaces a dead child on the reserved port receives EOF rather than credentials. The Windows ConPTY child is created suspended, assigned to a kill-on-close Job Object before it resumes, and remains one bidirectional terminal for every turn. Antigravity and the later Claude cutover never use print/one-shot mode.
- The common `ProviderAdapter` owns per-session runtime slots, one private supervisor identity, exact handle/profile correlation, confirmed-stop tombstones, generation-tokened cross-platform room/session leases, and bounded owned-process shutdown. It places lease plus handle/owner in a `Launching` slot and changes Unix `pending:<generation>` to fail-closed `launching:<generation>` before first polling the driver, then requires typed safe-versus-uncertain launch failures. Parent and guardian overlap token-bound lifetime locks and complete an exact ready/continue handshake before the guardian may create the anchor; an exact generation tag bridges anchor spawn to marker activation, and guardian/stopped provider launcher overlap the lifetime proof. Cancellation or crash therefore cannot leave a launch gap that admits a replacement. A pre-anchor `Launching` slot may be released only when both lifetime and tagged-runtime evidence are absent; after anchor activation it requires the guardian's exact receipt. Drivers own transport and protocol only. Linux/Android launch single-file providers from sealed executable `memfd` bytes, other Unix single-file providers hold a byte-verified private staged copy in an explicitly verified `0700` directory, and Windows holds the verified image without write/delete sharing. The native Codex executable and required sibling code-mode host are one multi-file bundle staged together on every Unix target and retained by the resident driver until that runtime drops. Linux/Android bind helper re-exec through `/proc/self/exe`; macOS opens the desktop executable, verifies its device/inode against the process's mapped text vnodes, and launches the server from a private staged copy of those open bytes. On Unix the independent guardian is the provider launcher's actual parent, establishes the anchor group before continuing the stopped launcher, and alone may publish an exact generation-bound `gone` cleanup receipt. A post-anchor failure becomes safe only after bounded guardian cleanup and that exact receipt; every other outcome retains uncertain authority. Linux/Android make the guardian a child subreaper; Linux freezes and captures group-external descendants through start-time-revalidated pidfds, treats an inherited lifetime inode as ownership only when that exact descriptor's `/proc` metadata records a shared `flock` read, then waits for the group and every captured identity to disappear. Process reuse first confirms live guardian custody and the exact provider anchor group; Linux/Android finally reject bounded `/proc` status `Z/X` so an unreaped dead leader cannot be reused. Android detects escaped lineage but fails closed without stable signaling. macOS registers `kqueue` fork/exit history before provider code runs and fails closed if the provider ever forks or exits before custody is established. Confirmed shutdown is checkpointed durably before its exact lease generation is released. Codex lifecycle start launches the selected executable as persistent `app-server --stdio`, initializes JSONL with `AgentsAssemble` client identity, drains stderr, bounds queued pre-turn notifications by both count and 2 MiB encoded bytes, and rejects provider-initiated requests by default. It then attaches exactly one provider thread with `thread/start` or resumes the durable identity with `thread/resume`; cancellation resumes the same pending JSON-RPC request without retransmission, and only a bounded exact thread identity can activate the provider session. Antigravity launches one interactive native session, refuses pre-existing workspace hooks, and installs only its managed room hook. A workspace registration retains the first quoted absolute, byte-verified hook executable until the last same-workspace session drops. Each provider process separately receives its own canonical absolute room-helper prefix, which is the exact value used by its prompt, terminal policy, and hook policy; portal authority remains per-session, while a bare basename or workspace-shadowed executable is never auto-approved. It binds a new conversation through a per-launch nonce and parses only bounded transcript tails. OpenCode launches one persistent `serve --pure` runtime with project, global, default-plugin, and external-skill configuration isolated while retaining its native data/session store; every response and SSE model alias must be present, non-conflicting, and exactly equal to the configured model. Neither driver falls back to print, exec, Python, or another provider.
- Codex keeps the complete attachment response until every original-compatible thread-ID and observed-model location is normalized. Conflicting aliases, a reported model different from the exact configured model, or a definitive initialization/attachment failure poison that process state; only a still-pending timeout/cancellation may continue reading the same request. A poisoned process cannot be reported as a successful reused runtime. A fatal turn poison is stopped under its exact handle/owner/lease generation before persistence clears that authority; an attachment poison remains explicitly restart-required and cannot be reused.
- A provider turn crosses one common adapter contract only after its exact durable turn ID, active phase, provider-session identity, runtime handle, and supervisor owner match. The adapter also owns the provider-neutral room-observation lifecycle: a bounded canonical view, exact read receipt, and exactly one server-owned `message` publication or explicit supported decline. Ordinary provider final text never becomes a room publication for this mode. Codex receives the common portal as process-local MCP configuration on its persistent `app-server --stdio`; the MCP endpoint is an in-process, independently bearer-authenticated capability-addressed loopback service with bounded bodies and absolute connection/request deadlines, not a provider-spawned executable or filesystem outbox. A hard connection-task bound evicts only the oldest unauthenticated entry and requires its permit to return before replacement, preserving separately bounded authenticated request admission without an unbounded pre-authentication task/FD surface. Its bearer crosses Unix custody through an anonymous manifest descriptor and never appears in argv. The managed process marks the exact workspace untrusted so project-local Codex configuration, hooks, and exec policies cannot consume the bearer while the runtime RoomPortal MCP remains active. Every portal uses a fresh unpredictable `TOKEN`-shaped environment-variable name; Codex's built-in sensitive-name exclusion is forced on for model tool children and its shell snapshot feature is disabled, preventing config prebinding and snapshot replay without replacing legacy or canonical user filters. Exact server-specific MCP approvals are accepted while all other provider requests remain denied. Publication may be staged before the read as in the original, but finalization requires both outcome and receipt to match the active turn generation. Antigravity and OpenCode expose the same common outcome through their native transports rather than defining room semantics themselves. Exact known Agent Session IDs can hand off the floor; unknown aliases resolve to no handoff. Cancellation resumes the same in-memory observation generation and provider request without deleting a staged terminal action.
- The common adapter releases its outer runtime-slot lock before the long provider wait, while an inner driver lock serializes protocol effects and an owner cancellation token lets exact stop/shutdown interrupt the wait. Codex sends `turn/start` with the room-observation orientation, room-source metadata, workspace, exact model/effort, approval policy, and structured sandbox policy. It requires one bounded exact provider-turn identity across all original response aliases, rejects reuse of that identity for another logical turn within a bounded process-lifetime history, and validates every reported model including `model/rerouted` before accepting output. Output-bearing notifications require both exact thread and turn identities; malformed unscoped output fails closed rather than becoming the active turn. Official thread-scoped hooks require the exact thread but accept their nullable turn identity, without contributing output. Valid unmatched notifications remain bounded and the original one-second final-message-plus-idle completion inference remains supported. Cancellation continues the same pending request or active provider turn without retransmission.
- Provider attachment uses that same exact-runtime cancellation owner. Dropping the
  requesting caller leaves the shielded attachment task and driver custody intact;
  exact stop or shutdown cancels the attachment wait, receives the driver back, and
  performs the existing process cleanup. Cancellation never retransmits a pending
  initialize or thread request and never becomes runtime-absence authority.
- Ordered-room routing commits the source `message_final`, its one selected Agent Session queue entry, and the first available durable turn assignment in the same SQLite mutation. Direct mentions first resolve across every configured session, so a stopped or detached exact target keeps its queued floor unless kicked or muted. Without a direct target, an agent-origin message prefers an eligible director before the original sampled least-recent-speaker rule and configured previous-speaker exclusion. Exactly one room turn may be active. Each session's combined pending/inflight authority is hard-capped at 256 unique event IDs; overflow rejects the source message in the same transaction. An assignment moves only the oldest complete prefix that fits 50 messages and the 20,000-character canonical RoomPortal view, leaving every later message pending and advancing the provider cursor only through the visible prefix. The view lists exact visible Agent Session handles for deliberate handoff. Lifecycle preparation and startup candidate loading reject oversized, empty, duplicate, or partial busy-turn authority before any provider effect.
- Provider completion accepts only the RoomPortal outcome, validates its exact active turn, receipt, publication target, and canonical input cursor, then atomically appends a portal-attributed `message_final` or explicit decline, `turn_finished`, idle session state, provider-sync advancement, and—when queued input exists—the next assignment. A provider failure appends a redacted bounded error and terminal turn state, restores inflight input to pending, clears active authority, and marks recovery required. Confirmed fatal runtime shutdown clears only the matching durable handle/owner/lease generation before the adapter releases its tombstone. Stop and restart recovery likewise preserve inflight input as pending; adoption of a runtime with an interrupted active turn disables and detaches that session in explicit recovery rather than leaving a busy turn with no task. Successful or replayed start and stop commands preserve their committed ACK even if later floor progression fails, log the stable progression error, and attempt the next pending floor without requiring a new message. The browser projects complete `agent_session_created` and `agent_session_state` authority separately from the visible message timeline and rejects private, partial, cross-room, or identity-inconsistent creation projections in snapshots and live events. History pagination never replays participant or Agent Session state over the current canonical snapshot.
- The browser admits only coherent `stopped/disabled` or `starting/enabled` creation projections. It projects complete live `agent_session_created` and `agent_session_state` authority separately from the visible timeline, while initial, resume, resync, and history snapshot events never overwrite the snapshot Participant or Agent Session arrays.
- `agent.start` and `agent.stop` accept exactly one unmodified Agent Session identifier alias and no unknown fields. Their durable external-effect identity is domain-separated over the exact room, principal, request ID, and action, so neither whitespace normalization nor cross-room reuse can alias a supervisor operation.
- Start and stop effects begin only after a `prepared` intent and a room/principal/request reservation commit together. The reservation binds action, payload hash, Agent Session, operation ID, and phase until the exact command completes or records a safe terminal rejection, including across recoverable failures and restart, so one request ID cannot authorize effects for different sessions; all non-lifecycle command admission checks the same namespace, and every non-current schema fails closed before product state is read. A safe launch failure atomically retains its bounded redacted rejection for exact replay without a new durable room-budget reservation, event, or provider call. Its definitive outcome closes the in-memory process-principal retry exemption, so every later exact replay receives a new process debit while retaining the same stored rejection authority. The provider's exact pre-effect lease or confirmed-stop tombstone remains owned until either that terminal result commits or an exact same-request live-`Gone` reconciliation commits the original intent to its next durable phase. An exact handle/owner/lease-token release occurs only after the successful database transition and is action-specific: start releases proven-absent launch authority, while stop releases its confirmed tombstone before provider-free finalization. Checkpoint failure leaves restart evidence intact, start retry receives a fresh generation, and finalized stop cannot leave an old tombstone blocking a later start. Only the exact originating operation can finalize its intent; opposite and unrelated lifecycle commands fail while it is outstanding. A successful start reports process reuse and provider-conversation reuse separately, and `provider_session_active` comes from the observed provider thread rather than process presence; claimed reuse must preserve the prior durable provider-session identity. Every runtime handle carries a private supervisor-instance owner and lease-generation token, and persistence emits no stop effect when any member is missing. Confirmed stop is checkpointed as `effect_applied` before finalization and that checkpoint survives server restart; an ambiguous start or stop instead commits a redacted `disconnected` state with recovery required and never claims success. Its `unconfirmed` intent retains the exact operation/handle/owner/token binding, blocks replacement by a newer lifecycle generation, and performs no provider effect on replay until authoritative reconciliation changes the state.
- Before the server admits HTTP or WebSocket traffic, persistence loads private reconciliation candidates and closes its read transaction; the common supervisor concurrently obtains an exact provider-neutral `Adopted`, `Gone`, `LeaseUncertain`, or `Ambiguous` observation, and persistence applies each only after reloading the complete session plus pending reservation set and matching its CAS token. A normal active runtime with empty lifecycle fields is a valid startup candidate, not an unknown action; after its exact cold `Gone` commit, the same post-commit owner releases its confirmed runtime absence without inventing a lifecycle command. Every nonempty stored lifecycle action must be exactly start or stop with its matching reservation and phase before observation. The server-lifetime watcher retains ordered 64-session pages so orphan lifecycle authority remains detectable; it excludes terminal reservation rows from candidate allocation and delays its first tick because startup already ran the same required pass. Current runtimes create their exact lease generation before any provider process effect and bind the same hashed OS boot identity and launch token into the private runtime-v5 handle, durable Agent Session, and activated Unix marker. Linux/Android use the kernel boot UUID; macOS uses immutable `kern.bootsessionuuid`, never wall-clock-derived boot time. One strict cross-platform runtime-v5 decoder and one absence-proof owner classify every cold and live observation. Cold Unix `Gone` requires the exact boot/handle/durable/marker generation witnesses; cold Windows `Gone` requires the exact handle/durable/marker launch triple. A live slot never treats `PreviousBoot` as proof. A pre-effect empty durable identity remains the only separately safe empty-authority case. An observation error is never upgraded from the handle alone, and any one-sided boot, platform, format, or token mismatch remains `Ambiguous`. Within the same boot, an unlocked `unix` marker remains ambiguous even when its group, tag, and lifetime lock have vanished, because a normal daemon may clear all three signals. `stop/effect_applied` never repeats its external effect, `Gone` stop proves the desired external state and remains finalizable, and exact adoption rebinds only the owner while retaining the lease generation. Every generic `Ambiguous` result retains the pending request, operation ID, handle, owner, and token in a recovery-required state, so neither start nor stop can release authority or admit a replacement without proof. Provider conversation identity is retained for explicit recovery. Clean schema 23 has only `pending` and `rejected` lifecycle reservation states; schema 22 is rejected without conversion, compatibility, or fallback code.
- Untrusted lifecycle diagnostics cross one shared redaction boundary and are capped at 512 characters before entering public session state or room events. The browser accepts every defined public lifecycle status but rejects private runtime handles, provider conversation IDs, profile markers, and lifecycle intents.
- Runtime and provider processes have explicit cancellation and reaping owners. Desktop/server shutdown and verification cleanup stop only processes created by that owner.
- The local executable-race boundary assumes the OS account itself is trusted. Same-account hostile processes are outside scope; paths, network peers, room inputs, and provider output are not trusted.

## Projection and delivery matrix

Credential resolution produces one `AuthenticatedPrincipal` per HTTP request or
WebSocket connection. Capabilities are server-derived and rechecked against current
room state for each command. Snapshot, catch-up/history, live fanout, reconnect, and
resynchronization apply the same viewer visibility policy. Each durable sequence is
delivered to each viewer as exactly one public event or `event_hidden` envelope;
hidden events retain `id`, `seq`, `room_id`, and `created_at` so the cursor stays
contiguous. Public ACKs, results, errors, and resync payloads use a separate
redaction projection and never reveal private runtime profiles, provider session
identities, workspace/executable authority, or internal diagnostics.

HTTP and WebSocket adapters only authenticate, decode, and encode before calling
the same application command owner. No event or requester result is published
before its commit. Post-commit event and ACK ordering is not assumed: an event may
arrive before the live ACK, and reconnect may recover that event before replay
returns the command result. The frontend must correlate the command and project
durable state without using either ordering as temporary authority. Normal success
does not call resync; resync is reserved for a cursor gap, lag, proof mismatch, or
another explicit synchronization failure.

## Lifecycle fault and replay matrix

- A crash before the first command reservation proves no provider effect exists.
- A crash after the create reservation but before lifecycle preparation resumes the
  same session and start intent; concurrent retry cannot prepare a second intent.
- Launch timeout or cancellation does not prove absence and cannot be reported as
  stopped or successful merely because a handle was not yet checkpointed.
- If a process exists before runtime handle/owner/lease-generation checkpoint, recovery observes and
  classifies that exact attempt as `Adopted`, `Gone`, `LeaseUncertain`, or
  `Ambiguous`; it does not launch a replacement first.
- A crash after observed running state but before command-result commit cannot
  repeat the external start.
- ACK loss after result commit replays the exact committed public result.
- Stop may affect only the exact runtime handle, supervisor owner, and generation.
  Confirmed stop is durably checkpointed before handle deletion or generation
  release.
- Uncertain start or stop remains explicit recovery-required state;
  it never impersonates success, absence, or a new generation.
- Uncommitted events and ACKs are never broadcast.

## Provider ownership

The common adapter may own runtime slots and generations, supervisor identity and
cancellation, process custody and cleanup confirmation, provider session/thread
evidence, provider-neutral observation lifecycle, and a normalized outcome or
explicit decline/failure. Drivers encapsulate their native transport objects and
protocols. Final slot replacement, generation, stop authorization, cleanup
confirmation, and reuse eligibility remain common-supervisor decisions.

Neither adapter nor driver may own user authorization, mutate durable Agent Session
desired/configured state directly, allocate room sequence, decide command
replay/conflict, create the final ACK, project for a viewer, or append/broadcast a
canonical room message or event. The adapter consumes committed room context and
returns an outcome. The room application owner revalidates it and commits message,
event, turn completion, and command result through persistence.

## Non-goals for the first implementation bundle

- Continuous/free-mode room attention, streaming activity, provider permission requests, personas, alternate execution harnesses, and external Agent Bridges.
- Providers beyond installed Codex, Antigravity, and OpenCode discovery.
- Replacing room directory, invites, identity recovery, attachments, votes, moderation, channels, voice, side chat, friends, pins, search, or plugin flows.

These are sequencing boundaries, not reductions of the repository reimplementation objective.

## Acceptance criteria

### First bundle: catalog and durable stopped session

1. Owner and non-owner snapshot, live, catch-up, reconnect, and resync paths apply the same projection; hidden events preserve contiguous sequence, and public ACK/result/error payloads pass their redaction boundary.
2. The React room shows Codex, Antigravity, and OpenCode from live CLI discovery, including their discovered model values and a nonempty catalog revision.
3. A host can submit one original-shaped create command with `start=false` for a stopped Agent Session or `start=true` for server-owned creation plus lifecycle intent. The client sends neither a second start command nor a normal-path resync.
4. The participant, public session, complete creation event, first command reservation, and correlated result follow the required commit phases. A connected viewer sees a stopped creation immediately with no HTTP roster request or reconnect, and a later provider-start failure leaves that created authority visible. Same-request replay is deduplicated; changed payload reuse conflicts; stale catalog, unsupported controls, missing capability, and invalid workspace fail without unauthorized rows or events.
5. Reconnect and a full Rust runtime restart recover the same Agent Session, command reservation/result, runtime profile, viewer cursor, and any explicit recovery state without Python.

### Slice exit: real provider conversation

1. The same visible session can start, consume canonical room context, publish a durable reply, stop, and restart without changing its provider conversation identity when the provider supports resume.
2. The exact real-client matrix in `docs/VERIFICATION.md` passes: Codex Terra, Antigravity Flash, and OpenCode Muse Spark contributor free. Missing availability remains failed or unknown and never substitutes a model.
3. Every Computer Use window, test runtime, Agent Session, and provider process created for verification is shut down and its cleanup result recorded.
4. The final public commit reproduces ACK-loss replay, launch ambiguity or adoption, exact stop, hidden-sequence reconnect, provider conversation reuse, and restart cleanup evidence; a create-and-start success screen alone is not slice completion.

## Verification

The RoomPortal MCP owner previously kept two independently changing flows in one
655-line module: loopback HTTP server lifetime, bearer authentication, connection
admission and deadlines; and provider-visible tool routing plus terminal-action
staging. The transport flow now lives in a sibling module while the tool handler
retains the shared `PortalState` and every room-action invariant. The split reuses
the existing `Arc<Mutex<PortalState>>` boundary and exposes only the handler type and
constructor within the provider crate; it adds no trait, forwarding wrapper, state,
task, timer, retry, fallback, or dependency. Capability-path secrecy, constant-time
bearer validation, the atomic authenticated/eviction transition, request and
connection bounds, exact read receipt, and one terminal outcome remain unchanged.
The accepted cost is one direct sibling-module dependency instead of one mixed
source file. No CPU, memory, disk, or latency improvement is claimed. Focused
RoomPortal MCP tests, including the authentication/eviction race and raw loopback
admission/body-limit checks, passed after the split.

The warning-denied workspace check then measured three server async boundaries above
the 16-KiB `large_futures` threshold: provider-result commit at 16,448 bytes,
interrupt recovery at 17,088 bytes, and its startup candidate at 17,360 bytes. The
completed-result owner now boxes the exact commit future and removes the former outer
box around the whole result handler, retaining one allocation per completed provider
result rather than adding a second one. Startup and live reconciliation box only the
exact interrupt-resume future when a durable interrupt effect exists; ordinary
candidates allocate nothing new. Durable ordering, cancellation points, retained-turn
release, publication, exact interrupt authority, and failure classification keep the
same owners and await points. The trade-off is one heap allocation for each recovered
interrupt effect to bound its caller's stack state. No latency or throughput gain is
claimed. Warning-denied server Clippy and the complete repository `make verify`
gate passed with no lint exception after the change.

The cancelled-initialization regression had twice failed before this correction,
and successful repetitions spent 6.7–7.1 seconds in the test because shutdown could
wait its complete five-second driver-ownership deadline. The fixture also published
its descendant PID before reading `initialize`, so it did not prove which lifecycle
phase it cancelled. The common attachment owner now observes the existing runtime
cancellation token both before driver acquisition and during attachment, and the
fixture publishes only after consuming `initialize`. This adds no task, state,
provider branch, fallback, or shorter safety deadline. It preserves cancellation
shielding, exact driver/process custody, pending-request replay, and fail-closed
macOS fork handling. Ten consecutive repetitions completed in 1.71–1.76 seconds,
then all 120 provider tests and warning-denied provider Clippy passed. No CPU,
memory, or disk improvement is claimed. The complete repository `make verify`
gate then passed.

- `make verify`
- Explicit Windows GNU cross-checks for the workspace and Tauri shell as recorded in `docs/VERIFICATION.md`.
- WebSocket boundary test: create/replay/conflict/reconnect/restart against a real SQLite file.
- Browser/Tauri real flow: select discovered provider/model, add stopped session, and observe it after runtime restart.
- Slice-exit provider run: the exact three-provider matrix above, with owned-process identity and cleanup evidence.
