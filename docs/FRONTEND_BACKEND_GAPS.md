# Frontend/backend exposure map

Status: source-derived reimplementation exposure inventory, 2026-08-28

Comparison baseline: original
`d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust
`fdb4e49`. The local-authority, surface, connection-admission, subscription, and moderation
boundary is complete. Central registration, canonical participant roles, participant
mute, exact provider interruption, and the required packaged provider matrix have
passed both manual reviews. Room-global settings, local-operator preferences, human
admission, the session-derived person-profile exchange, and the admitted-human
WebSocket are source-connected and integration-verified. Real production-browser
one-use/reusable normal and read-only admission, avatar, reload, posting/denial, and
restart recovery pass against the canonical Rust authority. Remote-session
preferences now pass their real write, read-only denial, reload, and restart flow.
Exact `participant.leave` now passes its real WebSocket UI, HTTP connector, session
revocation, reload, and restart flow.

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

## Completion reporting contract

This file is the single exposure inventory for final reimplementation reporting.
It must be updated whenever a Rust backend surface or copied frontend entry point
becomes reachable, becomes intentionally service-only, or is removed. Before the
reimplementation is called complete, the final handoff must explicitly enumerate:

- Rust backend behavior that has no reachable frontend entry point;
- partially connected frontend flows and the exact missing operation or state;
- intentionally indirect or service-only backend surfaces that do not need a UI;
- controls kept visibly unavailable because their complete authority boundary is
  not implemented.

A backend route, command, or event existing in source is not evidence that the
frontend exposes its real user flow. Tests, copied components, fixtures, and local
fake state likewise cannot close an exposure row.

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
| Use the release-health administration panel | `GET /api/release-health` and `GET /api/release-health/queue` | `AdminPanel.tsx` calls both routes, but production `App.tsx` never sets `adminOpen` to `true`; the component has no reachable opener. |
| Use Mafia from the canonical client | `GET /api/play/mafia` plus `POST /api/play/mafia/start`, `/chat`, `/vote`, `/resolve`, and `/action` | `api.ts` defines start/chat/vote/resolve calls and `useActiveMafiaGame.ts` defines a poller, but no production component imports or invokes them. `/action` has no React call at all. The canonical UI has no Mafia start or control entry point. |
| Server-side random roll/choice commands | `room.random.roll`, `room.random.choose` in `room/commands.py` | The composer sends messages and votes; it has no command or control for these two actions. |
| Configure a custom text or voice channel after creation | Canonical room settings accept the persisted `channels` list and per-channel settings | The React client can create a custom channel, but custom channel buttons have no context-menu handler and `RoomSettingsModal.tsx` hard-codes only `lobby` for channel notification settings. Rename, delete, and per-custom-channel notification controls are absent. |

### Original alternate or service-facing surfaces excluded from Rust parity

These original surfaces are implemented and unreferenced by React, but the absence
of a button is not a product gap. Alternate or compatibility-only entries in this
table are original-product evidence, not Rust requirements; Rust does not recreate
them unless a separately verified current user or service flow depends on them.

| Backend surface | Why it is not a direct React call |
| --- | --- |
| `POST /api/agent-sessions` | Alternate HTTP creation; the canonical room UI uses the atomic `agent.create` WebSocket command. |
| `GET /api/providers`, `GET /api/model-catalog`, `GET /api/provider-sessions/local` | Alternate discovery reads; the canonical catalog and local sessions arrive in the room snapshot, while login and forced refresh use dedicated HTTP routes. |
| `GET /api/room-events/stream`, `GET /api/rooms/state` | Alternate SSE/state reads; the canonical room projection uses the WebSocket snapshot/event stream. |
| `POST /api/room-members/mute`, `POST /api/room-participants/kick`, `POST /api/room-participants/leave` | Alternate HTTP controls; React uses `participant.mute`, `participant.kick`, and `participant.leave` WebSocket commands. |
| `GET /api/central-login/callback`, `GET /central-login-complete` | OAuth return pages reached by browser navigation after the frontend starts the handoff, not by `fetch`. |
| `POST /api/server-info/challenge` | Server-to-directory proof flow; the local room UI does not own this challenge. |
| `bridge.*`, `room.observed`, `room.check`, `room.result.publish`, `room.attachment.read`, `turn.*`, `activity.update`, `message.delta`, `message.final`, `provider.request.open`, `provider.request.closed` | Agent-Bridge/provider command surface. Browser authority intentionally exposes only `provider.request.resolve` from this family. |

## Original real-client findings

On 2026-08-24, original commit `d504647…` was run from a fresh isolated output
root after the tracked legacy Python files were removed. Five current-module
imports were disconnected only far enough to start the current GUI; this was a
diagnostic run, not a clean-source release claim. The Safari desktop client was
driven with Computer Use. The tested room used the canonical React entry points,
and the provider matrix used the installed real native sessions rather than mocks
or print/one-shot substitutes.

Verified reachable behavior:

- room creation, room name/topic, ordered and ambient modes, chat and tabletop
  tool modes, appearance, room/channel notifications, and invite scope;
- general and custom text chat, side chat, mention and emoji insertion, attachment
  stage/remove/send, image preview, search, read cursor, pin, edit, vote create/
  cast/withdraw/close, and the `/vote` command dialog including no-deadline mode;
- human profile/avatar/status, Agent Session profile/avatar/runtime/activity
  settings, member role and room mute authority, and provider lifecycle controls;
- custom voice-channel create, join, mute/unmute, and leave. The product explicitly
  labels this as presence-only; no audio transport is currently claimed;
- CCv3 persona-card import, automatic library selection, and the safe notice that
  stored scripts, regexes, and triggers are not executed;
- real Codex Terra, Antigravity Flash, and OpenCode Hy3-free room replies. Pause,
  resume, stop, and stopped-session resume controls were exercised where exposed.

Observed original-product defects and reachability limits:

- The left-bottom human profile card can paint above the Agent Add and User
  Settings modal backdrops. This reproduces the old stacking defect in the
  original client; a copied Rust client must retain the verified Rust overlay fix,
  not reintroduce the original geometry.
- Human profile projection is not one SSoT in the original flow. The saved name
  updated the main timeline and member list, while channel search retained the old
  name, a custom-channel message/pin used generic `호스트`, and voice presence also
  displayed generic `호스트`.
- A read-only local guest preview correctly disables all posting controls, but it
  remained at `불러오는 중...`, never projected room messages, and displayed the
  operator's human profile as guest `YOU`.
- `응답 중단` acknowledged the request but did not cancel or transition an active
  Antigravity response; only Stop settled the session.
- A manually saved friend retained the supplied provider identifier in the invite
  row, but its profile rendered a generic Claude/Codex/Cursor/Antigravity family
  string. Friend deletion has no confirmation step and immediately calls the
  delete route.
- The member context-menu label `내보내기` executes kick, not participant export.
  Agent-detail kick does have a confirmation dialog, whose Korean copy currently
  renders `에이전트을`.
- Custom channels have no React context menu or settings row after creation, as
  confirmed by both the real client and the source owners above.
- Reused stopped-session model controls can expose inconsistent accessible values
  versus their visible child label. The model drill-down is also incompletely
  exposed through Safari accessibility, so keyboard/search accessibility remains
  unverified rather than clean.
- `continuous` is not offered when creating or editing a normal current room.
  `RoomSettingsModal.tsx` renders it only when persisted room state is already
  `continuous` and labels it an old compatibility/legacy relay mode. It is not a
  current-mode parity requirement for the Rust reimplementation.

A supplemental Chrome Computer Use run on the same original commit exercised the
previously unknown credential, public-tunnel, and destructive paths against a
disposable output root:

- The built-in Cloudflare quick tunnel reached `running`, produced an HTTPS public
  origin, served the real React client, and stopped with the owning GUI process.
  The retired quick-tunnel origin returned HTTP 530 after cleanup. The stable
  Worker entry initially kept redirecting to that dead origin because GUI shutdown
  schedules its KV delete on a daemon thread and exits without waiting for it.
  An ownership-checked explicit clear completed and the Worker then reported a
  null target; the unawaited shutdown clear is an original lifecycle defect.
- A public human invite was generated and admitted `InviteGuest`; the one-time
  token was removed from browser history and the host roster projected the guest.
  However, that public guest client remained at `불러오는 중...`. Sending a
  message failed with `방 연결이 준비되지 않았습니다`, even after waiting, so
  public browser admission passes but its post-join realtime path does not.
- A one-use external-AI invite was consumed by the original `RoomConnector` over
  the public origin. Its WebSocket command received an event ACK, and the host
  React timeline rendered `EXTERNAL_AI_INVITE_OK`. The installed Room Connector
  test plugin's separate MCP endpoint was unavailable, so the original local
  connector runtime was used rather than claiming that plugin path passed.
- An operator-pairing link redeemed on the public origin as the canonical `SeiNel`
  operator, removed its token from browser history, and a second isolated-browser
  redemption was rejected as `pairing_already_used`.
- Host member `내보내기` displayed its confirmation and kicked the disposable
  guest, confirming again that the label maps to kick rather than export.
- Message deletion displayed the confirmation dialog and projected a
  `삭제된 메시지입니다` tombstone. Permanent server deletion remained disabled
  until the exact server name was entered, then removed the disposable room and
  immediately revoked the paired public session.

## Canonical React behavior not provided at Rust baseline `5aaa04b`

The Rust server currently exposes `GET /healthz`, `GET /api/host-challenge`,
`GET /api/server-info`, `POST /api/server-info/challenge`, `POST /api/ws-ticket`,
authenticated `GET/POST /api/rooms`, authenticated
`GET/POST /api/user-profile`, profile-avatar upload/read, static `/app`, and `/ws`.
Private no-store `GET /api/public-invite/status` and
`POST /api/public-invite/tunnel/start|stop` expose the implemented direct managed
ingress lifecycle to the local operator boundary. The copied modal and controller
call all three through fresh one-use server-operator tickets, strict status parsing,
and generation-owned polling; their legacy Host-token and mutable public-URL paths
are removed. Manager-invite create/revoke still awaits the separate C1 frontend
ticket cutover and packaged verification.
Its room command implementation
at the public comparison commit completes `message.send`, atomic
`agent.create(start=false|true)`, `agent.start`, `agent.resume`, `agent.stop`, and
stopped-session `agent.configure`.
Everything below must remain visibly unavailable or failed until its Rust owner is
implemented; the frontend must not silently substitute Python or local fake data.

| React feature group | Missing Rust surface |
| --- | --- |
| Startup identity and accounts | Fresh local desktop bootstrap, central guest creation/recovery, secure browser device identity, and proof-bound local-server registration are implemented. `/api/account`, Google account challenge/connect/delete, and the native Google handoff remain incomplete; the absent `open_central_google_login` host command keeps that button failed closed. |
| Room lifecycle and settings | Canonical archive/delete lifecycle and trusted remote activation of public server identity remain incomplete. Local server info and origin-bound identity challenge, room-global settings mutation, local-operator preferences, and admitted remote-human preferences are connected. |
| Admission and invites | Durable human invite/session create, join, verification, expiry, revoke, leave, restart recovery, the session-derived person-profile exchange, and the authenticated human WebSocket are implemented at their current persistence/server boundaries. Exact `/join`, `/join/`, `/pair`, and `/pair/` production entrances resolve their actual production assets; exact preflight/join response contracts bind request, room, client, server lineage, and product surface before bearer exposure. The browser's single bounded admission-intent owner now preserves pending retry custody and retires it only through a verified settled write or verified direct removal; a settled marker surviving best-effort deletion remains cleanup-only across reload, RoomGuestSession local-storage expiry/clearing, and later invite navigation. Direct external deletion of the admission-intent session-storage key is outside that guarantee. Real isolated browsers pass one-use/reusable normal and read-only admission, avatar transfer, profile edit, token removal, reload, posting/denial, consumed-link rejection, same-browser reusable recovery, server-restart reconnect, purpose-exchanged preference read/write or read-only denial, and exact leave with durable session revocation. Configured-manual trust, managed quick-tunnel/stable-entry custody, the private no-store status/start/stop controls, backend manager invite create/revoke with separate room-bound private-control tickets, and the native desktop create/revoke ticket bridge with its exact registered commands, capabilities, and permissions are implemented and tested. The copied modal/controller B2 ingress controls use fresh server-operator tickets, strict status parsing, and generation-owned polling, and their legacy mutable public-URL path is removed. Invite creation still uses the old moderator helper and therefore cannot satisfy the manager-ticket route; C1 frontend activation remains failed closed and unverified. Host claim, companion admission, operator pairing redeem, C2 retained invite custody, and complete packaged activation remain incomplete. |
| Roster, friends, and channels | The active-room roster, strict participant-role control, copied participant-mute control, canonical event projection, and exact Rust provider-interrupt owner are cut over and packaged-verified. Room friends, room channels, voice presence, and side chat remain incomplete. |
| Attachments, personas, pins, and search | General-message and room-appearance attachment purposes, persona list/import/thumbnail, message pins, room search/context. Profile-avatar upload/read is implemented. |
| Provider settings and diagnostics | Login, catalog refresh HTTP response, credential CRUD, provider usage, local resources, release health, and runtime version. The original `/api/local/workspace-picker` HTTP route is absent, but packaged desktop creation uses the native Tauri directory picker instead. |
| Games and plugins | Mafia HTTP operations and generic plugin WebSocket hosting remain unimplemented. The copied RimWorld view is an external plugin consumer; its Python plugin package/runtime is intentionally outside the current Rust core-migration scope and is not a core parity exit condition. |
| Canonical room commands | History, vote summary, edit/delete, re-add, general pause/interrupt, participant kick, room lifecycle, and provider request resolution remain incomplete. Settings and random operations are implemented at the backend boundary; direct human random controls are intentionally absent because the original React client has none. Strict `participant.role.update`, stopped-session `agent.resume`, `agent.configure`, verified `participant.mute`, and exact self `participant.leave` are connected to their existing copied controls. |
| Canonical room events | The React projector recognizes the broader original event vocabulary; only Rust-emitted snapshot/events are currently verified. |

### Active local-authority exposure delta: 2026-08-25

The current candidate replaces fixture bootstrap with an immutable-lineage local
authority. Fresh startup creates schema and the bootstrap marker only. The copied
desktop gate commits the real local human profile, admits a real zero-room
directory, and the room-rail plus control creates the first complete room through
the server-operator HTTP surface. The closed directory response is bound to the
native bootstrap server ID and lineage. Room creation durably binds a UUID request
and payload hash; exact replay is idempotent, while changed payloads or a separate
request for an existing room conflict without renaming it. Incomplete bootstrap
and inconsistent authority fail closed.

The server-wide local human profile is reachable before the first room through a
fresh one-use operator credential. The same profile remains the SSoT after room
creation and projects display-name/avatar revisions only into Active rooms where
the human membership is still Joined. Ended memberships keep their historical
identity projection. Room role, join state, room mute, and Agent Session profiles remain
owned by their separate authorities. Packaged Computer Use verified zero-room
startup, room create/join, one committed chat message, profile projection, modal
z-order, conditional Agent Add fields, and restart durability.

The production composition no longer mounts the original HTTP roster reader or
role refresh. Active-room and invite-modal participant lists use only the current
authenticated WebSocket projection; another room or disabled stream exposes no
cached members. No `/api/room-members` placeholder or failure-swallowing merge was
added.

The server directory now carries a closed `ServerProductSurface` whose HTTP
routes come from the same declarative registrations that construct the Axum
router, and whose WebSocket streams/actions come from the strict protocol enums
accepted by the socket. The desktop `HostProductSurface` is the intersection of
one shared Tauri command registry and the capability permission file; the build
manifest, invoke handler, and advertised command list consume that same registry.
The webview pins both surface digests for its lifetime before opening a room
socket. Production room composition requests only advertised streams, rejects
commands absent from the advertised action set, and sends canonical
`message.send` as content-only. Consequently copied side-chat/plugin socket code
and the RimWorld view remain source provenance but are not mounted or requested
by the current Rust product surface. The absent native Google-login command is
also rejected at the host-surface boundary rather than being attempted as an
unregistered Tauri invocation. Central guest creation, recovery, bootstrap, and
proof-bound local-server registration use their implemented owners and do not
borrow that absent Google command.

The same run confirmed these still-open frontend/backend rows rather than hiding
them:

- `/api/room-friends` is called by the copied Friends view but has no Rust route,
  so the view visibly reports `Load failed`.
- `/api/account` is called by Public Account settings but has no Rust route, so
  that section visibly reports 404.
- Desktop central guest creation/recovery and server-directory registration are
  cut over and packaged-verified. Native Google handoff and the Public Account
  settings routes remain incomplete; neither is inferred from guest success.

The earlier local-only packages used an explicitly empty central URL and remain
local-runtime evidence only. The separate 2026-08-26 production-central package
is the evidence for the guest and server-registration rows above; it is not Google
OAuth or Public Account settings evidence.

## Stage A feature-candidate delta

The active Stage A candidate removes the discarded conversion and relay prototype
before adding any new authority. It accepts only a fresh or exact-current Rust
schema, rejects `continuous` and older queue/profile/attachment shapes, and has no
Python, older-Rust, local-profile, provider-alias, or client-side compatibility
path.

The candidate adds one canonical `room.settings.update` WebSocket transaction for
the currently implemented settings controls, typed ordered/ambient queue items,
mode-transition-safe scheduling, and shared human/provider tabletop randomness.
Provider roll/choose uses the existing private `RoomPortal` on Codex and the exact
bound helper on Antigravity/OpenCode; it is not print mode or a client-side result.
The original React client still has no direct human roll/choose control, so the
human commands remain a reachable server contract without a fabricated button.
Local-operator and admitted remote-human preferences are connected through their
complete purpose-ticketed HTTP owners. Appearance assets, custom channels, invites,
and plugin hosting stay explicitly incomplete until their complete owners exist.

## Stage B local preference exposure delta

The copied room-settings UI now loads and saves the local operator's room and per-channel
notification preferences through separate one-use read and write grants.
Desktop grants are pinned to the advertised host surface and require exact string tickets,
positive safe-integer TTLs, and canonical IPv4-loopback HTTP bases.

The response parser rejects room mismatches, unknown fields, unsupported values, and
noncanonical cursors instead of projecting defaults. POST bodies contain only preference
fields; room-global settings remain WebSocket-owned. Both server responses and desktop
fetches disable caching so a browser cache cannot bypass ticket consumption or current
membership authorization. The copied settings control and restart flow are packaged-
verified. Room appearance upload, authenticated preview/read, binding, replacement, and
cleanup remain visibly incomplete and are not implied by preference completion.

The admitted-human owner now exchanges a raw session credential only at the exact
read or write purpose endpoint. The room-settings target receives only the derived
one-use grant and no desktop device credential. The write path revalidates the
durable session inside the mutation transaction; read-only sessions cannot obtain a
write grant. The copied channel menu uses this path after admission, while a tokenless
pre-admission remote remains failed closed. No local-operator authority, cached
default, compatibility bearer branch, or client-owned mutation substitutes for it.

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
- Tauri obtains a separate fresh one-use server-operator HTTP ticket for the
  canonical room directory. The copied room rail remains visibly unconfirmed
  until `GET /api/rooms` projects durable room/settings state, and its plus control
  creates the complete SQLite room boundary before inserting a UI entry.
- The copied left-bottom human profile reads and updates the authenticated Rust
  user profile, including bounded canonical profile-avatar publication, without
  overwriting room role/join/mute authority or Agent Session profiles.
- The WebSocket client requires the one-use ticket/proof-key object, validates a
  signed `Subscribed` receipt against the already pinned server surface and the
  expected room/participant, recomputes the exact Snapshot-byte and permissions
  digests, and withholds product readiness plus queued commands until the
  contiguous finite catch-up reaches authenticated high-water `H`. The prior
  proofless string-ticket/non-desktop path is removed; central and guest socket
  ticket authority stays explicitly incomplete until its real owner is cut over.
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

### Human-session profile exposure delta: 2026-08-26

The copied left-bottom profile and settings UI now exchange a live admitted human
session for a fresh one-use profile ticket on every read, patch, and avatar upload.
The profile target never accepts the raw room session. Display name, custom status,
and avatar use the server person-profile SSoT; room role, join state, mute, and
permissions remain room-owned, while every Agent Session keeps its own profile.

Before admission there is no server person-profile authority. The guest panel now
shows only the invite-submission profile without sending a profile request or exposing
settings; admission switches it to the server-owned profile path. Production-frontend
Computer Use verified preflight, admission, two save/re-read cycles, and file
selection/cropping/avatar re-read against a disposable canonical Rust fixture. The
same run confirmed two adjacent gaps rather than hiding them: the authenticated human
room socket is not connected, so the room remains unready, and Public Account settings
still expose the missing `/api/account` route as 404.

### Human-session browser connection delta: 2026-08-27

The original source registers `/join`, `/join/`, `/pair`, and `/pair/`, but the first
Rust static owner exposed only `/app`. The exact entrances and root asset directory are
now served from the same production bundle. A successful human admission returns the
existing server ID, immutable authority lineage, and product surface; the guest session
must pass the existing strict room-directory shape/digest/lifetime binding before its
raw session can be exchanged for a socket ticket. There is no directory fetch with
host authority, client-created surface, compatibility session, or failed-join restore.

Computer Use exercised isolated normal and read-only browsers against disposable
canonical Axum/SQLite state. Both removed their invite tokens and rendered the
authenticated snapshot/roster. The normal browser published
`HUMAN_SOCKET_NORMAL_UI_OK`; the read-only browser rendered disabled composer controls
and produced no message event. This closes only that browser connection delta. The
controlled socket races are recorded separately; trusted external ingress remains open.

The follow-up production-browser matrix exercised distinct one-use and reusable
normal/read-only invites. One-use links admitted once, survived tokenless reload, and
were rejected from a fresh browser identity after consumption. Reusable links
re-entered with the same browser identity without another use or participant and
retained history. Normal sessions published two durable messages. Read-only sessions
rendered both messages, kept their composer and attachment controls disabled, allowed
person-profile display-name edits, and rejected post-admission avatar upload without
creating an asset. The normal one-use pre-admission avatar became exactly one current
profile asset with no pending row. A live read-only reusable browser reported the
server interruption and then automatically restored its authenticated snapshot and
history after an actual process restart.

The follow-up remote-preference run used the copied channel menu. A writable guest
stored channel mute, re-read it after reload, and retained it after an actual server
restart. A distinct read-only guest loaded defaults, received the canonical write
denial, displayed the stale-state rollback, and created no preference row. Guest
leave still displays that `participant.leave` is absent from the bound signed product
surface and performs no mutation. That cutoff was closed by the later exact
participant-leave cutover recorded in `docs/VERIFICATION.md`. At that browser-matrix
cutoff manager invite create/revoke was not made reachable through fake host
authority. The backend controls, exact private-control tickets, and native desktop
manager-invite bridge are now implemented; frontend activation remains incomplete.
