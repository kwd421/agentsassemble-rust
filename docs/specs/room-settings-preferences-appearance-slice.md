# Room Settings, Scheduling, Tabletop, Preferences, and Appearance Slice

Status: approved implementation design; Stage A is routed and not yet implemented

## Definition

This slice completes the reachable settings controls without creating storage-only
success. Stage A owns canonical room settings, the conversation schedulers that
those settings change, typed durable provider input, and human/provider tabletop
randomness. Stage B owns the authenticated user's room preferences and the room
appearance asset lifecycle. Custom channels, voice/text streams, invites, and
activity-plugin hosting remain closed until their complete product slices exist.

The comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust commit `5c9035f`.
The design was critically reviewed in the continuing GPT-5 Pro, very-high-reasoning
web session. Its initial and follow-up `REVISE` findings produced the queue,
routing, migration, and random-tool reservation contracts below; the final design
returned `APPROVE` before implementation began.

## Original reachable contract

- `settings_json` is the room-global SSoT for label, topic, appearance,
  `conversation_mode`, `tool_mode`, ordered-speaker exclusion, relay limit,
  channels, and activity plugin. Changing the label updates the room summary in
  the same transaction.
- `room.settings.update` requires current `room.manage`, a nonempty strict partial
  update, and the exact current `expected_revision`. Settings, room metadata, one
  `room_settings_updated` event, and the replay result commit before publication.
- The revision is `room-settings-v1-` plus SHA-256 of sorted-key compact UTF-8 JSON.
  A stale revision is `settings_conflict`; it is never partially merged.
- Settings commit precedes scheduler reconciliation. A post-commit progression
  failure cannot turn the committed ACK into a NACK; it is exposed as durable
  public floor-progression failure and retried from later lifecycle triggers.
- Conversation modes are `ordered`, `ambient`, and `continuous`. Provider input
  orientation is an independent delivery semantic: ordered observation, ambient
  observation, or transcript.
- Human `room.random.roll` and `room.random.choose` validate bounded notation,
  options, and reason, generate server-owned randomness, and publish canonical
  `message_source=room_tool_result` system messages. Provider tools expose the
  same result semantics during a current tabletop-enabled turn.
- `GET /api/room-settings?room_id=...` composes canonical global settings with the
  authenticated user's own preferences. `POST /api/room-settings` accepts only
  those preferences; attempts to write global settings over HTTP conflict.
- Preference records are keyed by stable user and room identity. A channel key is
  supported when it is one of `lobby`, `live`, `board`, `records`, or matches
  `c[0-9a-f]{12}`. Registry membership is intentionally not required. At most 54
  exact per-channel records are accepted.
- A room appearance upload is not room authority. Only a settings transaction that
  binds its exact URL makes it reachable as banner or icon.

## Stage A: strict settings availability

`RoomSettings` understands and validates the complete original record. Fresh
defaults, migration, snapshot/directory projection, revision generation, and
mutation all use the same domain validator. The record is not equivalent to the
set of mutations currently available.

Stage A permits mutations only for behavior completed in the same stage:

- label and topic;
- `conversation_mode`, `tool_mode`, ordered previous-speaker exclusion, and
  `max_relay_turns`;
- visual `banner_preset` and `icon_label`, which the copied UI already renders.

Stage A rejects, with stable explicit unsupported errors and no write:

- nonempty `channels`, until custom registry plus text/voice behavior exists;
- `banner_image_url` or `icon_image_url`, until Stage B owns the asset lifecycle;
- nondefault `invite_scope`, until invite/admission behavior exists;
- nonempty `activity_plugin`, until plugin hosting exists.

The settings transaction contains an `AssetReferenceTransition` validation/plan
boundary from the start. Stage A's implementation rejects every URL transition;
Stage B activates this same boundary. No route keeps a second allowlist, and no
unsupported field is silently preserved as a successful mutation.

A v9 authority containing a value that no reachable v9 Rust path could create—
nonempty channels/activity plugin, a room-asset URL, or nondefault invite scope—
fails migration visibly. Migration does not repair, discard, or reinterpret it.

## Stage A: typed durable provider input

The v10 private Agent Session record replaces string `pending_event_ids` and
`inflight_event_ids` with only:

```text
QueuedRoomInput {
    event_id,
    delivery_kind: OrderedObservation | AmbientObservation | Transcript,
    relay_depth,
}
```

No parallel metadata map is allowed. One assignment contains only the oldest
contiguous items with the same delivery kind and relay depth, subject to the
existing event, message, and rendered-view bounds. That depth becomes the active
turn and resulting durable message relay depth.

The v9-to-v10 conversion runs inside one SQLite migration transaction. Each old
row and referenced room event is strictly checked for uniqueness, capacity,
room/actor identity, sequence, active source/cursor, and complete active-or-clear
turn authority. Existing pending and inflight order is retained and every item is
converted to `OrderedObservation` at relay depth zero, because the only v9 producer
was ordered and had no relay-depth metadata. Active turn ID, source, cursor,
sequence, runtime fields, and lifecycle fields are unchanged. Missing, wrong-room,
self-origin, duplicate, overflowing, or inconsistent authority aborts migration.
SQLite rollback makes interruption before schema-version commit a complete no-op;
after commit, normal startup reconciliation sees the same active authority.

## Stage A: scheduling and transitions

Mode transitions never delete or cancel active/inflight work.

- `ordered` to or from `ambient` preserves every queued delivery kind.
- `continuous` to `ordered` or `ambient` preserves queued transcript work.
- Switching to `continuous` does not eagerly delete queued observations. As in the
  original, each session lazily removes observation items only when its
  `assign_pending` path next runs while the current mode is continuous. Switching
  back before that point leaves them intact.
- Multiple active turns are valid after a transition. Each session tuple remains
  strict; ordered scheduling merely refrains from a new assignment while
  `active_count >= 1` and does not call `active_count > 1` corruption.

Routing preserves the original distinction between addressed and unaddressed work.

- Ordered direct targeting resolves configured sessions before idle eligibility,
  so busy/stopped/detached addressed sessions may retain queued work unless their
  participant is kicked or muted. Undirected work uses the original director,
  previous-speaker, sample, and least-recent-speaker policy.
- Ambient queues every nonactor participant that is eligible or runtime-busy, then
  assigns independently under per-session capacity.
- Continuous mention, structured target, and `@all` queue addressed nonactor
  sessions without an idle prefilter; kicked or muted participants are rejected.
  Assignment still requires current attach/enable/idle/bridge authority.
- Only unaddressed continuous work filters `default_responder=true`, applies strict
  floor eligibility, and selects one session in stable provider/session-ID circular
  order after the actor.
- An agent-origin continuous message at `max_relay_turns` is not routed. Otherwise
  the new transcript item's relay depth is incoming depth plus one.

Existing v9 sessions migrate with `default_responder=true`, preserving the current
Rust candidate behavior. New sessions project the explicit canonical provider
profile/adapter value into the Agent Session SSoT; this is not a permanent
redefinition that every provider must be a default responder.

Actor startup, provider-session activation, provider completion, and new-work
progression retry durable reconciliation. None may alter an already committed
settings command result.

## Stage A: tabletop and provider tool atomicity

One domain parser and one server RNG owner implement both human commands and
provider tools. Provider payloads contain only notation/options/reason; a
turn-scoped capability stamps immutable room, session, and turn authority. The
provider task, provider crate, and MCP handler never own persistence, publication,
or the room actor's ingress receiver.

The room actor owns one bounded tool ingress and handles it in its `select!` loop.
Before a result is committed it revalidates the durable active turn, inflight
source, room/session/turn tuple, current tabletop mode, normal room write budget,
and the durable count of successful random results for that turn. At most 32 may
commit. Commit and publication use the same SQLite writer and publication cursor
as every other room mutation.

Random tools and terminal publication share the existing `PortalState` mutex and
an explicit reservation state machine:

1. The MCP handler briefly locks the current generation and atomically verifies a
   same-generation read receipt, open terminal, nonclosing observation, and
   `successful + live < 32`. It inserts an opaque `Queued` reservation, unlocks,
   sends the stamped actor request, and awaits a oneshot.
2. The actor dequeues the guard and, under the same short witness lock, changes the
   exact live token from `Queued` to `Committing`. Only then may it begin durable
   validation and the SQLite transaction.
3. After commit or rollback is known, the actor resolves the guard, removes the
   reservation, updates the witness success count when appropriate, and only then
   sends the reply. The durable DB count remains budget authority.
4. `publish_message` and `decline_to_speak` stage a terminal outcome only while the
   same reservation set is empty. Terminal-first makes later reservations fail;
   reservation-first prevents terminal publication from overtaking its outcome.
5. Send failure or pre-commit request drop aborts only a `Queued` reservation.
   Caller cancellation after queue ownership transfers cannot release it.
   `Committing` is resolved only after transaction commit or rollback.
6. Turn close marks the observation closing, invalidates queued work, and denies new
   reservations/terminal actions. Committing work remains in a closing tombstone
   until resolved; only then is the observation removed. Normal turn finalization
   also rejects live reservations.

No witness or driver mutex is held over queue send, actor reply, provider I/O, or
SQLite await. The single room actor cannot process settings/cancel/provider-result
between a reservation's begin-commit and its DB outcome.

## Stage B: preferences and appearance

A complete canonical preference row is owned by `(user_id, room_id)`. Each request
uses a fresh one-use, purpose-scoped room HTTP ticket, authenticates before reading
the body, re-resolves active membership, and accesses only the caller's row. The
notification values, default record, exact keys, cursor bounds, 54-channel cap,
and builtin-or-canonical-ID grammar match the original. User preferences never
create channels or modify room, participant, or Agent Session authority.

Room appearance uses a separate room-owned attachment table and `ra_` capability
namespace. Profile and room images call one extracted safe-raster decoder guarded
by one shared global admission semaphore. Accepted PNG/JPEG/GIF/WebP is bounded by
the existing input, dimension, pixel, allocation, and concurrency limits and is
re-encoded to static PNG.

Pending preview is same-room/same-owner, high entropy, short lived, and served with
`image/png`, `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`, and
`Referrer-Policy: no-referrer`. Stage B implements `AssetReferenceTransition` so a
settings transaction may atomically promote only a nonexpired pending or already
bound asset of the same room and owner. Banner and icon references are evaluated
together; replacement or clearing deletes only an old asset with no remaining
reference. Unknown, expired, quarantined, corrupt, cross-room, or cross-owner
assets are not reachable.

Global mutation and realtime projection remain WebSocket-owned. Preferences and
binary upload/read remain authenticated HTTP request/response controls. Neither
transport is a fallback for the other.

## Failure, acceptance, and review gates

- Invalid input, stale revision, unsupported mutation, missing authority, queue
  corruption, tool race, budget exhaustion, or persistence failure changes no
  authoritative state unless the contract explicitly committed before a later
  reconciliation failure.
- Replay of the same request/action/payload returns its committed result without a
  second event; conflicting reuse fails.
- Deterministic tests cover mode-transition timing, multiple active turns,
  default-responder routing, relay batching, migration rollback/restart, read
  receipt, terminal-first and reservation-first barriers, cancellation phases,
  close tombstones, and parallel/durable 32-call budget enforcement.
- Stage B tests cover two-user isolation, wrong-room and replayed tickets,
  auth-before-body, custom preference key grammar, shared raster admission,
  pending preview/expiry, bind/replacement/clear, restart, and reference cleanup.
- Mandatory architecture/source-growth/800-line gates, `make verify`, native and
  Windows warning-denied checks, generated bindings, and packaged copied-UI flows
  must pass. No gate exception is added.
- Stage A and Stage B are separate public feature commits. Each is pushed before
  critical web and same Daybreak Blue manual-security review. Provider-dependent
  verification uses persistent Codex Terra, Antigravity Flash, and OpenCode Hy3
  free sessions, never print mode, and removes every verification-owned process,
  window, server, and temporary resource afterward.

Room close/archive/delete, invite/admission, custom channel registry and streams,
voice, activity-plugin hosting, general message attachments, PostgreSQL, RimWorld,
and new console/local-profile UI remain separate contracts. Their controls stay
visibly unsupported until their complete authority boundary is implemented.
