# Room Directory and Creation Slice

Status: public implementation `6624e51`; critical web and manual-security review pending

## Definition

The copied room rail is hydrated from the authenticated Rust room directory,
and its existing add-room action creates a canonical durable room through the
Rust runtime. A native/browser cache may accelerate first paint, but it is
never room authority and remains visibly unconfirmed until the server answers.

## Authority boundaries

- SQLite `rooms` plus each room's `settings_json` are the SSoT for room ID,
  stable server-assigned room UID, label, status, timestamps, and the settings
  projection shown in the directory.
- One stable opaque `server_id` belongs to the database authority rather than a
  port, process, room, browser cache, or Tauri window.
- The server-wide human profile supplies only display name and avatar when a
  newly created room receives its initial local-operator membership. Role,
  joined state, mute state, permissions, and later membership transitions stay
  room-owned.
- Room creation and directory reads are request/response control operations and
  remain HTTP. Live room state, ordered commands, snapshots, and events remain
  on the ticket-authenticated WebSocket. Neither transport is a fallback for
  the other.
- The private Tauri/runtime control channel may issue a fresh one-use
  server-operator HTTP ticket. Its scope is distinct from a room WebSocket
  ticket, and the host secret never reaches React, URLs, logs, or durable data.

## Reachable contract

- `GET /api/rooms` returns `server_id` and canonical room summaries sorted by
  last activity, excluding archived rooms unless `include_archived=true`.
  Every summary includes the complete public room-global settings projection,
  so an inactive room does not invent a separate label or appearance.
- `POST /api/rooms` accepts one bounded canonical `room_id` and label from the
  copied add-room flow. A new room transaction creates the room, default
  settings, publication cursor, local human membership projected from the
  current server profile, and exactly one durable `room_created` event.
- Repeating creation for an existing room preserves its stable UID and never
  emits a second `room_created` event. It may apply the original idempotent
  label/status update only when the existing room and active local-operator
  membership are valid; it never silently restores a left, kicked, exported,
  or detached membership.
- HTTP authentication is consumed before a request body is read. Only the
  packaged local operator's one-use server ticket can enumerate every local
  room or create one in this slice. Guest/session-scoped directory projection
  remains explicitly incomplete until admission owns those credentials.
- Exact Tauri origins, body limits, room/text normalization, error redaction,
  bounded ticket capacity, and one-use consumption fail closed.
- The copied desktop frontend refreshes the server directory instead of
  disabling hydration. Cached entries are marked unconfirmed while the request
  is pending or failed; a successful response removes stale local entries,
  preserves unrelated remote-server entries as disconnected, and becomes the
  local directory projection. A fabricated default `general` room is never
  shown as authority. On a fresh database, the server's existing bootstrap path
  creates the real durable `general` room before the directory response.
- A directory response that raced with a newer WebSocket metadata projection is
  discarded and fetched once more, as in the copied current client contract.

## Failure and retry semantics

- Database creation, settings, cursor, participant, event, and command-visible
  state commit together or do not change. A failed create cannot leave a room
  rail entry produced only by the client.
- A wrong-purpose, unknown, expired, or reused operator ticket fails before
  body decoding. An operator HTTP ticket cannot open a WebSocket.
- Stored room/settings/profile corruption fails visibly rather than producing
  an empty/default summary or participant.
- A directory refresh failure keeps only the bounded cached projection and a
  visible unconfirmed notice. It does not report synchronization success,
  create a local room, or redirect to Python or another transport.
- Retrying the same existing room create is idempotent at the product state
  boundary; a conflicting/invalid request remains a visible error.

## Verification

- persistence tests cover stable `server_id`/room UID across reopen, canonical
  sorting/filtering, atomic new-room creation, profile-derived human projection,
  preservation of room-owned membership state, idempotent retry, and rollback;
- server boundary tests cover purpose-separated one-use tickets, Tauri CORS,
  body-before-auth rejection, real `GET/POST /api/rooms`, and immediate WebSocket
  admission to the newly created room;
- copied frontend tests cover desktop authenticated routing, pending/failed
  unconfirmed state, stale-cache removal, remote entry preservation, hydration
  race retry, and no client-only success entry;
- the exact packaged app starts from the real server directory, creates and
  enters a room through the existing plus control, sends a room message,
  restarts with stable server/room identities, and shuts down all owned
  processes and browser/Computer Use resources.

Room-global settings mutation, per-user room preferences, room close/archive/
delete cleanup, invites/admission, remote/mobile server selection, custom
channel streams, PostgreSQL, RimWorld, and new console/profile-management UI
are outside this slice and remain explicitly incomplete rather than simulated.
