# Frontend/backend exposure map

Status: source-derived migration inventory, 2026-08-23

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
currently completes `message.send`, `agent.create`, `agent.start`, `agent.resume`,
and `agent.stop`.
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
| Games and plugins | Mafia HTTP operations and plugin WebSocket frames. |
| Canonical room commands | History, vote summary, edit/delete, settings, random operations, re-add/configure, pause/interrupt, participant controls, room lifecycle, and provider request resolution. Stopped-session `agent.resume` is connected. |
| Canonical room events | The React projector recognizes the broader original event vocabulary; only Rust-emitted snapshot/events are currently verified. |

## Connected Rust slice and live evidence

The frontend source, styles, assets, and component hierarchy were copied from the
original React frontend rather than recreated. Rust-specific changes stay at the
desktop and transport boundaries:

- Tauri obtains a short-lived ticket, WebSocket base URL, and proof key through
  its existing `runtime_ticket` command.
- The WebSocket client verifies the Rust runtime's initial snapshot proof before
  accepting events or sending queued commands.
- Rust snapshots drive the original room timeline, provider catalog, participant
  list, Agent Session list, and the original create/start/resume/stop controls.
- The original create dialog uses a native Tauri directory picker in the packaged
  desktop build. Desktop creation preserves the original atomic intent by issuing
  the Rust backend's durable create and start commands in order, then resyncing.
- Unsupported original controls fail with the Rust command error. They are not
  hidden by a substitute result and do not fall back to Python.

Packaged-release UI verification on 2026-08-23 exercised the original controls
against real native sessions, not mocked adapters:

| Provider | Verified path |
| --- | --- |
| Codex `gpt-5.6-terra` | Native app-server session created from the copied UI, one real room turn completed, then the exact owned process tree was stopped. |
| Antigravity `gemini-3.6-flash` | Persistent native session created from the copied UI, one real room turn completed, then the exact owned process tree was stopped. |
| OpenCode `opencode/hy3-free` | Persistent `opencode serve` session created from the copied UI, first turn completed, UI stop completed, then UI resume reused provider session `ses_fd204f66cffekIX7o7bed3VgnA`; a second turn completed with durable `provider_session_reused=true` and turn count `2`, followed by confirmed UI stop. |

The OpenCode resume check also verifies that `agent.resume` keeps its own durable
request/result identity while sharing the provider launch effect with `agent.start`.
It does not silently rewrite a resume request into a start request.

This file must be updated when either side gains or removes a reachable surface.
