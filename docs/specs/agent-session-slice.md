# Agent Session vertical slice

Status: active implementation owner

## Definition

A host selects an installed provider/model from the authoritative live catalog, creates a durable Agent Session, and can ultimately start that same session so its canonical room-context reply is published back into the room.

## Current contract

- Provider options come from bounded probes of the installed provider CLIs. Every probe runs in its own owned process tree with a credential-free environment allowlist, a ten-second deadline, and bounded output; cancellation and shutdown kill and reap the whole tree. A session can be created only from a `ready` catalog entry and the exact current `catalog_revision`; a stale, unavailable, unlisted, oversized, or internally inconsistent selection fails visibly.
- OpenCode subscription discovery accepts only syntactically valid model IDs in the original managed `opencode` and `opencode-go` namespaces. Other namespaces never become startable subscription authority.
- `agent.create` requires the server-derived `agent.control` capability. Client-supplied ownership, participant role, provider command, executable, runtime kind, transport, and process identity are ignored as authority.
- `(room_id, principal_id, request_id)` remains the command identity. A new Agent Session ID is deterministically derived from that full identity and action, and the participant, session, creation event, and ACK commit in one room mutation transaction. A same-payload retry returns the original result and never creates or starts a second runtime.
- The durable Agent Session owns desired/configured state. A provider supervisor owns live subprocesses and reports observed transitions through the room authority; process presence, caches, and task handles are never parallel session authority.
- A stopped server-owned session is restorable from its complete private durable runtime profile. Public snapshots, ACKs, events, replay results, and generated TypeScript never expose its workspace, executable, filesystem identities, or runtime profile key. Restart never silently substitutes a provider, model, workspace, transport, new provider conversation, or Python implementation.
- Workspace input is an exact path, not an identifier: it is never trimmed or cleaned before canonicalization. Selection records stable workspace and executable identities, and the persistence transaction reopens and revalidates both immediately before commit.
- Public provider catalogs are capped at 48 KiB, individual providers at 16 KiB and 256 options, and rooms at 64 Agent Sessions. Oversized authority fails closed before it can make the fixed 256 KiB WebSocket snapshot impossible.
- Provider output becomes a canonical durable `message_final` event attributed to the Agent Session. No ACK or room event is published before its transaction commits.
- Runtime and provider processes have explicit cancellation and reaping owners. Desktop/server shutdown and verification cleanup stop only processes created by that owner.

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
