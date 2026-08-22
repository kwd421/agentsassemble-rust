# Agent Session vertical slice

Status: active implementation owner

## Definition

A host selects an installed provider/model from the authoritative live catalog, creates a durable Agent Session, and can ultimately start that same session so its canonical room-context reply is published back into the room.

## Current contract

- Provider options come from bounded probes of the installed provider CLIs. Every probe runs in its own owned process tree with a credential-free environment allowlist, a ten-second deadline, and bounded output; cancellation and shutdown kill and reap the whole tree. Windows probes are created suspended, assigned to their Job Object, and only then resumed, so no descendant can escape before ownership attaches. A session can be created only from a `ready` catalog entry and the exact current `catalog_revision`; a stale, unavailable, unlisted, oversized, or internally inconsistent selection fails visibly.
- OpenCode subscription discovery accepts only syntactically valid model IDs in the original managed `opencode` and `opencode-go` namespaces. Other namespaces never become startable subscription authority.
- `agent.create` requires the server-derived `agent.control` capability. Client-supplied ownership, participant role, provider command, executable, runtime kind, transport, and process identity are ignored as authority.
- `(room_id, principal_id, request_id)` remains the command identity. A new Agent Session ID is deterministically derived from that full identity and action, and the participant, session, creation event, and ACK commit in one room mutation transaction. A same-payload retry returns the original result and never creates or starts a second runtime.
- The durable Agent Session owns desired/configured state. A provider supervisor owns live subprocesses and reports observed transitions through the room authority; process presence, caches, and task handles are never parallel session authority.
- A stopped server-owned session is restorable from its complete private durable runtime profile. Public snapshots, ACKs, events, replay results, and generated TypeScript never expose its workspace, executable, filesystem identities, or runtime profile key. Restart never silently substitutes a provider, model, workspace, transport, new provider conversation, or Python implementation.
- Workspace input is an exact path, not an identifier: it is never trimmed or cleaned before canonicalization. Selection records a stable workspace identity and an executable identity bound to both the opened filesystem object and all executable bytes. Persistence first performs a short replay check, reopens and revalidates both authorities without holding the SQLite writer, then opens the final transaction and rechecks room authority plus command replay before commit. Potentially stalled filesystem work runs in capacity-bounded detached workers with a ten-second deadline, so a stalled mount fails closed without joining Tokio runtime shutdown, and each stalled worker continues consuming capacity until it really exits.
- Public provider catalogs are capped at 48 KiB, individual providers at 16 KiB and 256 options, and rooms at 64 Agent Sessions. Oversized authority fails closed before it can make the fixed 256 KiB WebSocket snapshot impossible.
- Provider output becomes a canonical durable `message_final` event attributed to the Agent Session. No ACK or room event is published before its transaction commits.
- Runtime transport preserves the original resident boundaries: Codex owns one persistent `app-server` stdio JSONL process and thread identity; Antigravity owns one persistent PTY session on Unix or ConPTY session on Windows; OpenCode owns one local HTTP/SSE runtime and session identity. Antigravity and the later Claude cutover never use print/one-shot mode.
- The common `ProviderAdapter` owns per-session runtime slots, one private supervisor identity, exact handle/profile correlation, confirmed-stop tombstones, cross-platform room/session leases, and bounded owned-process shutdown. Drivers own transport and protocol only. Linux/Android launch sealed executable `memfd` bytes, other Unix targets hold a verified private staged copy, and Windows holds the verified image without write/delete sharing. Unix provider groups are rooted at an internal anchor whose guardian and control pipes preserve exact custody across provider-leader exit, cancellation, or server death; fresh-process `Gone` proof comes only from the exact unlocked lease. Confirmed shutdown is checkpointed durably before its lease is released. Codex lifecycle start launches the selected executable as persistent `app-server --stdio`, initializes JSONL with `AgentsAssemble` client identity, drains stderr, bounds queued pre-turn notifications by both count and 2 MiB encoded bytes, rejects provider-initiated requests by default, and reports the provider conversation inactive until the later thread attach checkpoint. Antigravity and OpenCode lifecycle drivers remain visibly unavailable rather than falling back to print, exec, Python, or another provider.
- `agent.start` and `agent.stop` accept exactly one unmodified Agent Session identifier alias and no unknown fields. Their durable external-effect identity is domain-separated over the exact room, principal, request ID, and action, so neither whitespace normalization nor cross-room reuse can alias a supervisor operation.
- Start and stop effects begin only after a `prepared` intent and a room/principal/request reservation commit together. The reservation binds action, payload hash, Agent Session, operation ID, and phase until the exact command completes or reaches a terminal owner-loss failure, including across recoverable failures and restart, so one request ID cannot authorize effects for different sessions; all non-lifecycle command admission checks the same namespace, and an older schema with an unrecoverable incomplete intent fails migration closed. Only the exact originating operation can retry or finalize its intent; opposite and unrelated lifecycle commands fail while it is outstanding. A successful start reports process reuse and provider-conversation reuse separately, and `provider_session_active` comes from the observed provider thread rather than process presence; claimed reuse must preserve the prior durable provider-session identity. Every runtime handle carries a private supervisor-instance owner, and persistence emits no stop effect when either identity is missing. Confirmed stop is checkpointed as `effect_applied` before finalization and that checkpoint survives server restart; an ambiguous stop instead commits a redacted `disconnected` state with recovery required and never claims success. Its `unconfirmed` intent retains the exact operation/handle/owner binding, blocks replacement by a newer lifecycle generation, and may be retried only by the same supervisor.
- Before the server admits HTTP or WebSocket traffic, persistence loads private reconciliation candidates and closes its read transaction; the common supervisor concurrently obtains an exact provider-neutral `Adopted`, `Gone`, `LeaseUncertain`, or `Ambiguous` observation, and persistence applies each only after reloading the complete session plus reservation set and matching its CAS token. Current-profile runtimes create their exact lease before any provider process effect, so an absent or unlocked lease can prove `Gone`; legacy profiles without that contract remain ambiguous. `stop/effect_applied` never repeats its external effect, `Gone` stop proves the desired external state and remains finalizable, exact adoption rebinds the owner, and ambiguous or foreign lifecycle ownership becomes terminal `owner_lost`. Provider conversation identity is retained for explicit recovery. An original owner-lost request remains permanently reserved and fails with `runtime_owner_lost`, while a new request may create a later runtime generation.
- Untrusted lifecycle diagnostics cross one shared redaction boundary and are capped at 512 characters before entering public session state or room events. The browser accepts every defined public lifecycle status but rejects private runtime handles, provider conversation IDs, profile markers, and lifecycle intents.
- Runtime and provider processes have explicit cancellation and reaping owners. Desktop/server shutdown and verification cleanup stop only processes created by that owner.
- The local executable-race boundary assumes the OS account itself is trusted. Same-account hostile processes are outside scope; paths, network peers, room inputs, and provider output are not trusted.

## Non-goals for the first implementation bundle

- Starting a provider process, automatic room attention, streaming activity, provider permission requests, personas, alternate execution harnesses, and external Agent Bridges.
- Providers beyond installed Codex, Antigravity, and OpenCode discovery.
- Replacing room directory, invites, identity recovery, attachments, votes, moderation, channels, voice, side chat, friends, pins, search, or plugin flows.

These are sequencing boundaries, not reductions of the repository reimplementation objective.

## Acceptance criteria

### First bundle: catalog and durable stopped session

1. The React room shows Codex, Antigravity, and OpenCode from live CLI discovery, including their discovered model values and a nonempty catalog revision.
2. A host can add one stopped Agent Session with an allowed model and workspace. The participant, public session, `agent_session_created` event, and correlated ACK are committed atomically.
3. Same-request replay is deduplicated; changed payload reuse conflicts; stale catalog, unsupported controls, missing capability, `start_now`, and invalid workspace fail without partial rows or events.
4. Reconnect and a full Rust runtime restart recover the same Agent Session and runtime profile without Python.

### Slice exit: real provider conversation

1. The same visible session can start, consume canonical room context, publish a durable reply, stop, and restart without changing its provider conversation identity when the provider supports resume.
2. The exact real-client matrix in `docs/VERIFICATION.md` passes: Codex Terra, Antigravity Flash, and OpenCode Hy3 free. Missing availability remains failed or unknown and never substitutes a model.
3. Every Computer Use window, test runtime, Agent Session, and provider process created for verification is shut down and its cleanup result recorded.

## Verification

- `make verify`
- Explicit Windows GNU cross-checks for the workspace and Tauri shell as recorded in `docs/VERIFICATION.md`.
- WebSocket boundary test: create/replay/conflict/reconnect/restart against a real SQLite file.
- Browser/Tauri real flow: select discovered provider/model, add stopped session, and observe it after runtime restart.
- Slice-exit provider run: the exact three-provider matrix above, with owned-process identity and cleanup evidence.
