# Frontend/backend exposure map

Status: source-derived migration inventory, 2026-08-24

Comparison baseline: original
`d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust
`11e9b8547580c3da8b2f32ed40ff5034d7683ec2`. Local uncommitted code and
local verification are not promoted to public implementation status in this file.

## Scope and method

This inventory compares the reachable source registrations in the Python product at
`../AgentsAssemble` with that product's canonical React client, then records the
remaining Rust cutover gap. It is not a feature specification and does not make an
unreferenced route public by default.

Evidence owners:

- Python HTTP routes: `../AgentsAssemble/agentsassemble/web/routes/`,
  `../AgentsAssemble/agentsassemble/features/*/routes.py`, and
  `../AgentsAssemble/agentsassemble/web/websocket.py`.
- Python room commands: `../AgentsAssemble/agentsassemble/room/commands.py` and
  `../AgentsAssemble/agentsassemble/room/realtime.py`.
- React calls: `frontend/src/api/`, `frontend/src/api.ts`,
  `frontend/src/roomSocketClient.ts`, `frontend/src/useCanonicalRoom.ts`, and
  `frontend/src/App.tsx`.
- Rust HTTP and room commands: `crates/agentsassemble-server/src/web.rs` and
  `crates/agentsassemble-server/src/room_runtime.rs`.

The HTTP comparison is method-aware. A route used only through another backend
flow is marked indirect rather than incorrectly reported as a missing screen.
Test fixtures and example asset URLs are not counted as frontend integration.

## Python backend behavior with no direct React exposure

### User-facing operations without a canonical frontend control

| Backend operation | Backend evidence | Frontend status |
| --- | --- | --- |
| Revoke an operator pairing | `POST /api/operator-pairing/revoke` in `web/routes/room_invite.py` | Create and redeem are called; revoke has no React call or control. |
| Inspect issued room invites and sessions | `GET /api/room-invite/invites`, `GET /api/room-invite/sessions` in `web/routes/room_invite.py` | The invite UI creates, joins, admits, and leaves; it does not list these backend records. |
| Revoke a room invite | `POST /api/room-invite/revoke` in `web/routes/room_invite.py` | No React call or control. |
| Direct Agent-Bridge invite join | `POST /api/room-invite/agent-join` in `web/routes/room_invite.py` | The React invite UI generates/copies an agent invite, but does not execute this bridge-side join route. |
| Export a participant | `POST /api/room-participants/export` and `participant.export` | Kick, mute, role changes, and leave are exposed; export is not. |
| Close a room without archiving/deleting it | `POST /api/rooms/close` and `room.close` | Archive is exposed through the moderation client and delete through the room socket; close has no React control. |
| Run or inspect a rolling restart | `GET/POST /api/runtime/rolling-restart` in `web/routes/runtime.py` | The frontend shows version/update status but has no rolling-restart call. |
| Submit the Mafia night/action operation | `POST /api/play/mafia/action` in `features/mafia/routes.py` | Start, chat, vote, and resolve are called from `api.ts`; action is not. |
| Server-side random roll/choice commands | `room.random.roll`, `room.random.choose` in `room/commands.py` | The composer sends messages and votes; it has no command or control for these two actions. |

### Alternative, compatibility, or service-facing surfaces

These are implemented and unreferenced by React, but the absence of a button is
not by itself a product gap.

| Backend surface | Why it is not a direct React call |
| --- | --- |
| `POST /api/agent-sessions` | Compatibility HTTP creation; the canonical room UI uses the atomic `agent.create` WebSocket command. |
| `GET /api/providers`, `GET /api/model-catalog`, `GET /api/provider-sessions/local` | Compatibility/discovery reads; the canonical catalog and local sessions arrive in the room snapshot, while login and forced refresh use dedicated HTTP routes. |
| `GET /api/room-events/stream`, `GET /api/rooms/state` | Alternate SSE/state reads; the canonical room projection uses the WebSocket snapshot/event stream. |
| `POST /api/room-members/mute`, `POST /api/room-participants/kick`, `POST /api/room-participants/leave` | HTTP compatibility controls; React uses `participant.mute`, `participant.kick`, and `participant.leave` WebSocket commands. |
| `GET /api/central-login/callback`, `GET /central-login-complete` | OAuth return pages reached by browser navigation after the frontend starts the handoff, not by `fetch`. |
| `POST /api/server-info/challenge` | Server-to-directory proof flow; the local room UI does not own this challenge. |
| `bridge.*`, `room.observed`, `room.check`, `room.result.publish`, `room.attachment.read`, `turn.*`, `activity.update`, `message.delta`, `message.final`, `provider.request.open`, `provider.request.closed` | Agent-Bridge/provider command surface. Browser authority intentionally exposes only `provider.request.resolve` from this family. |

## Canonical React behavior not yet provided by the Rust backend

The Rust server currently exposes `GET /healthz`, `GET /api/host-challenge`,
`POST /api/ws-ticket`, static `/app`, and `/ws`. Its room command implementation
at the public comparison commit completes `message.send`, atomic
`agent.create(start=false|true)`, `agent.start`, `agent.resume`, `agent.stop`, and
stopped-session `agent.configure`.
Everything below must remain visibly unavailable or failed until its Rust owner is
implemented; the frontend must not silently substitute Python or local fake data.

| React feature group | Missing Rust surface |
| --- | --- |
| Startup identity and accounts | `/api/account`, Google account challenge/connect/delete, central-login callback start/poll, guest recovery-code create/redeem. |
| Room directory and lifecycle | `GET/POST /api/rooms`, archive/close/delete compatibility routes, room settings, public server info, and central-directory registration proof. |
| Admission and invites | Host claim, room invite create/join/admission/companion/leave, operator pairing create/redeem, and public-invite status/URL/tunnel controls. |
| Roster, profile, friends, and channels | Room members, role/mute HTTP compatibility, user profile, room friends, room channels, voice presence, and side chat. |
| Attachments, personas, pins, and search | Attachment upload/read, persona list/import/thumbnail, message pins, room search/context. |
| Provider settings and diagnostics | Login, catalog refresh HTTP response, credential CRUD, provider usage, local resources, release health, and runtime version. The original `/api/local/workspace-picker` HTTP route is absent, but packaged desktop creation uses the native Tauri directory picker instead. |
| Games and plugins | Mafia HTTP operations and generic plugin WebSocket hosting remain unimplemented. The copied RimWorld view is an external plugin consumer; its Python plugin package/runtime is intentionally outside the current Rust core-migration scope and is not a core parity exit condition. |
| Canonical room commands | History, vote summary, edit/delete, settings, random operations, re-add, pause/interrupt, participant controls, room lifecycle, and provider request resolution. Stopped-session `agent.resume` and `agent.configure` are connected at the public comparison commit. |
| Canonical room events | The React projector recognizes the broader original event vocabulary; only Rust-emitted snapshot/events are currently verified. |

## Public Rust slice, active gap, and provenance gate

The frontend source, styles, assets, and component hierarchy were copied from the
original React frontend rather than recreated, but that statement is not parity
evidence by itself. The frontend provenance is original commit `d504647…`.
Every Rust-only frontend change must be allowlisted with its file and reason. The
allowlist is limited to runtime bootstrap, ticket/transport, Tauri native boundary,
and behavior-preserving structure-gate splits. Product-controller orchestration,
client-owned authority, changed DOM order, or changed CSS cascade is not justified
by the allowlist.

At the public Rust comparison commit:

- Tauri obtains a short-lived ticket, WebSocket base URL, and proof key through
  its existing `runtime_ticket` command.
- The WebSocket client verifies the Rust runtime's initial snapshot proof before
  accepting events or sending queued commands.
- Rust snapshots drive the original room timeline, provider catalog, participant
  list, Agent Session list, and create/start/resume/stop controls.
- The original create dialog uses a native Tauri directory picker in the packaged
  desktop build.
- One durable `agent.create(start=false|true)` reservation owns creation and its
  optional start intent. The copied desktop controller sends one command, consumes
  the original nested result shape, and does not issue a success-path start or
  resynchronization command.
- Snapshot, catch-up, resynchronization, and live fanout use the same
  authenticated-viewer projection. Invisible durable events become bounded
  `event_hidden` envelopes so every viewer retains a contiguous cursor, while
  public ACK/result/error shaping excludes private runtime authority.
- Stopped-session `agent.configure` preserves the Agent Session identity and
  revalidates provider controls plus stored filesystem authority before commit.
- Unsupported original controls fail with the Rust command error. They are not
  hidden by a substitute result and do not fall back to Python.

Frontend parity for each vertical slice compares assets, selectors/classes,
component and DOM hierarchy, responsive breakpoints, and rendered geometry at fixed
viewports. Geometry evidence includes left/right panel widths, central chat bounds,
composer, and left-bottom profile-card overlap/clipping. The same gate exercises
create stopped, create-and-start, re-add, stop, resume/restart, reconnect, and the
provider reply. Unsupported controls remain explicit unavailable/error states, not
fake data or no-ops.

The RimWorld package under the original repository's `plugins/rimworld/` tree is
tracked as a separately migratable plugin, not as part of the core Rust product
cutover. Its copied frontend consumer may remain present for source provenance,
but it must not be backed by placeholder snapshots, a fake plugin host, or a Python
fallback. Until a later plugin slice is explicitly opened, attempts to use it stay
visibly unsupported.

Published packaged-release UI verification for `99165dd` exercised native
sessions, not mocked adapters:

| Provider | Verified path |
| --- | --- |
| Codex `gpt-5.6-terra` | Persistent native app-server session created and started by one copied-UI command; one real room turn completed. |
| Antigravity `gemini-3.6-flash` | Persistent native PTY session created and started by one copied-UI command; one real room turn completed without print mode. |
| OpenCode `opencode/hy3-free` | Persistent `opencode serve` session completed turn one, application restart, UI resume with the same private provider-session identity, turn two with durable `provider_session_reused=true`, and confirmed UI stop. |

The OpenCode resume check also verifies that `agent.resume` keeps its own durable
request/result identity while sharing the provider launch effect with `agent.start`.
It does not silently rewrite a resume request into a start request.

The complete command flow, exact public commit, cleanup evidence, overlay geometry,
and the explicit Codex zero-turn resume limitation are recorded in
`docs/VERIFICATION.md`. Provider-private session identifiers remain local evidence.

This file must be updated when either side gains or removes a reachable surface.
