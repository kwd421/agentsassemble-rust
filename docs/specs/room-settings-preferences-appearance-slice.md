# Room Settings, Scheduling, Tabletop, Preferences, and Appearance Slice

Status: implementation and flow evidence retained; remote-session HTTP authorization
reopened by repository audit D-03. Stage A, local desktop authority, appearance
lifecycle, copied-frontend activation, and packaged restart evidence remain.

## Definition

This slice completes the reachable current settings controls without creating
storage-only success. Stage A owns canonical room settings, the ordered and ambient
conversation schedulers that those settings change, typed durable provider input,
and human/provider tabletop randomness. Stage B owns the authenticated user's room
preferences and the room appearance asset lifecycle. Human invite/admission and
its durable room-session owner now authorize remote-user preferences and remain a
prerequisite for bound appearance reads. Custom channels, voice/text streams, activity-plugin
hosting, and the original legacy relay mode remain outside this slice.

The comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust commit `5aaa04b`.
The earlier reviewed draft included the original compatibility-only continuous
relay and a v9-to-v10 data conversion. Real-client and source verification later
proved that continuous is shown only for a room already carrying that legacy value.
The user explicitly requires a Rust product with no legacy or compatibility
migration. That part of the earlier approval is superseded; the corrected design
below must be reviewed with the resulting implementation.

Audit correction at Rust baseline `8a5f75a`: the remote browser must present its
durable session once at the target preferences or appearance route, which performs
the exact room/permission/lifecycle revalidation. The detailed remote exchange and
derived-grant descriptions below are historical implementation evidence until
Phase 0B removes that extra credential hop. Desktop private-control grants remain.

## Original reachable contract

- `settings_json` is the room-global SSoT for label, topic, appearance,
  `conversation_mode`, `tool_mode`, ordered-speaker exclusion, channels, and
  activity plugin. The original record can also contain a legacy relay limit, but
  no normal current-room control reaches it. Changing the label updates the room
  summary in the same transaction.
- `room.settings.update` requires current `room.manage`, a nonempty strict partial
  update, and the exact current `expected_revision`. Settings, room metadata, one
  `room_settings_updated` event, and the replay result commit before publication.
- The revision is `room-settings-v1-` plus SHA-256 of sorted-key compact UTF-8 JSON.
  A stale revision is `settings_conflict`; it is never partially merged.
- Settings commit precedes scheduler reconciliation. A post-commit progression
  failure cannot turn the committed ACK into a NACK; it is exposed as durable
  public floor-progression failure and retried from later lifecycle triggers.
  Exact settings replay returns the stored result without running progression
  again.
- Current conversation modes are `ordered` and `ambient`. The original also parses
  `continuous` only to display an existing legacy relay room; the normal settings
  UI never offers it. Rust does not accept, create, migrate, or execute that mode.
  Provider input orientation is an independent delivery semantic: ordered or
  ambient observation.
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

`RoomSettings` understands and validates the current record. Fresh construction,
snapshot/directory projection, revision generation, and mutation all use the same
domain validator. The record is not equivalent to the set of mutations currently
available.

Before Stage B asset activation, Stage A permitted mutations only for behavior
completed in that stage:

- label and topic;
- `conversation_mode`, `tool_mode`, and ordered previous-speaker exclusion;
- visual `banner_preset` and `icon_label`, which the copied UI already renders;
- `invite_scope`, now that human invite creation and admission own the complete
  read/write and read-only contract. A settings change supplies only future invite
  creation; an already issued invite retains its immutable durable scope.

The current validator still rejects, with stable explicit unsupported errors and
no write:

- nonempty `channels`, until custom registry plus text/voice behavior exists;
- nonempty `activity_plugin`, until plugin hosting exists.

Stage B activates `banner_image_url` and `icon_image_url` only through the owning
settings transaction. No route keeps a second allowlist, and no unsupported field
is silently preserved as a successful mutation.

The active reimplementation does not import or convert an older Rust/Python room,
settings, queue, participant, or Agent Session record. A database that does not
match the current clean schema fails visibly. It is not repaired, reinterpreted,
or copied through a compatibility migration.

## Stage A: typed durable provider input

The current private Agent Session record stores provider input only as:

```text
QueuedRoomInput {
    event_id,
    delivery_kind: OrderedObservation | AmbientObservation,
}
```

No parallel metadata map is allowed. One assignment contains only the oldest
contiguous items with the same delivery kind, subject to the existing event,
message, and rendered-view bounds. The current schema creates this representation
directly; no older queue representation is accepted or converted.

## Stage A: scheduling and transitions

Mode transitions never delete or cancel active/inflight work.

- `ordered` to or from `ambient` preserves every queued delivery kind.
- Multiple active turns are valid after a transition. Each session tuple remains
  strict; ordered scheduling merely refrains from a new assignment while
  `active_count >= 1` and does not call `active_count > 1` corruption.

Routing preserves the original distinction between addressed and unaddressed work.

- Ordered direct targeting treats a structured handoff as the earliest target and
  lets a later body mention own the floor, then resolves configured sessions
  before idle eligibility. Busy/stopped/detached addressed sessions may retain
  queued work unless their participant is kicked or muted. Undirected work uses the original director,
  previous-speaker, sample, and least-recent-speaker policy.
- Ambient queues every nonactor participant that is eligible or runtime-busy, then
  assigns independently under per-session capacity.

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

A complete canonical preference row is owned by `(user_id, room_id)`. One
`RoomUserIdentity` resolver proves, in one transaction, an active room, a joined
human participant, and the exact `user_profiles.user_id -> participant_id`
binding. It does not infer manager authority from `owner_id`, participant role, or
a cached ticket capability. The current HTTP appearance manager is derived by a
separate full `require_complete_bootstrap_in_transaction` integrity check and the
exact local user, participant, and active-membership binding.

Local desktop preference reads and writes use separate one-use room HTTP purposes.
A remote browser presents its durable session bearer in the bounded Authorization
header directly to the exact target.
A write authenticates before reading the bounded body, then revalidates the same
room/user/participant/permission authority in the write transaction. Notification
values, defaults, exact fields, the 54-entry
cap, and builtin-or-`c[0-9a-f]{12}` channel grammar match the original.
`last_read_at` is not normalized: it is at most 64 Unicode scalar values and only
CR, LF, and TAB are forbidden. A top-level partial update replaces the complete
`channel_settings` map when that field is present.

The local operator obtains those purposes only from the desktop private-control
owner. An admitted remote human sends its session credential only to
`/api/room-settings`; that route does not interpret a desktop device credential or
retry a target rejection through another authority. Read-only sessions may read but
receive the canonical `session_read_only` rejection from a write.

The durable invite/admission session stores only the bearer fingerprint and binds
the exact room, user, participant, client, scope, expiry, and revocation state.
Writes authorize once before the bounded body and again in the mutation transaction,
so replacement, leave, expiry, or revocation between those points cannot commit.
Local ticket and remote session authentication converge only after each boundary has
resolved a typed principal; failure never falls through to another authority.

Per-room `GET /api/room-settings` preserves the combined `{room_id, settings}`
wire response while the frontend projects only the caller preference fields.
The reachable no-room local-operator branch has a distinct server-wide one-use
purpose and returns archived rooms too. HTTP global writes conflict; the canonical
WebSocket command remains the only room-global writer.

Room HTTP grants are a closed set: preference read, preference write, appearance
upload, exact pending preview read, and exact bound appearance read. A separate
closed grant owns the no-room settings directory read. Grants bind the exact room,
principal, participant, purpose, and read asset where applicable. Consumption
removes a grant before checking its variant so wrong-purpose, wrong-room,
wrong-asset, and replay attempts are consumed and rejected. Implemented issuers use
typed operations; no path or payload string selects authority. Preference,
directory, and private-control appearance issuance are exposed through exact Tauri
commands and a strict frontend bridge. The copied settings UI does not consume those
grants through the preference owner; its separate appearance API consumes only the
exact upload, pending-preview, or bound-read grant for the requested operation.

Appearance upload and pending-preview grants retain the resolved
`LocalRoomManagerAuthority`, including server, authority lineage, and room UID,
instead of reducing it to room/user/participant strings. Their persistence targets
revalidate that exact authority in the asset transaction, so a ticket cannot cross
a delete/recreate generation. Reusing the existing local-manager grant owner also
removes one redundant standalone manager-authorization transaction from each local
upload or pending read; the bounded ticket stores one existing authority value and
adds no query, cache, task, or durable state. Bound reads remain active-room-member
authority because applied appearance belongs to the room rather than its uploader.

Room appearance uses a separate room-owned table and the exact asset grammar
`^ra_[0-9a-f]{32}$`, generated as a UUID v4 from the operating-system RNG with
122 random bits after the fixed version and variant bits. Its only
stored URL is `/api/attachments/<ra-id>?view=1`; missing, download, or extra query
forms are rejected. The `ra_` prefix is reserved and malformed reserved IDs never
fall through to public profile attachment lookup. Existing attachment POST and GET
paths have one route owner that dispatches authority or ID namespace once; there
is no handler fallback or duplicate route.

Pending custody consists of `room_id`, `pending_owner_user_id`, and expiry. The
schema requires owner and expiry for pending rows and requires both to be null for
bound rows. Promotion clears both in the settings transaction, transferring
lifetime ownership to the room. The settings event owns mutation audit; a separate
creator quota key is not retained after transfer. Deleting an uploader may remove
pending assets but cannot remove bound room assets.

Profile and room images call the one safe-raster owner defined by
[`asset-custody-lifecycle-slice.md`](asset-custody-lifecycle-slice.md). That owner
contains only shared hard safety limits and checked replacement arithmetic. Room
appearance SQL owns room custody and evaluates `current - exact replaced + new`;
bound banner/icon bytes count to the room, never to an uploader. This slice adds no
generic uploader quota or speculative configuration layer.

Pending preview requires the exact same room, uploader, unexpired asset, and
current local manager. Bound read requires the exact same room, a bound and
integral asset, a current canonical banner or icon reference, and an active human
member. Both return static PNG with `Cache-Control: private, no-store` and inherit
the global `nosniff` and `no-referrer` policy. Once activated, the frontend must
perform an authenticated ticketed fetch, render only an object URL, revoke it on
reference, room, authority, or component-lifetime change, and reject late results
by fetch generation.

The settings transaction evaluates banner and icon together, deduplicates new
references, promotes only an unexpired pending asset uploaded by the current local
manager or retains a bound same-room asset, and deletes an old bound asset only
when neither next field references it. Promotion, cleanup, settings, event, and
command replay result commit or roll back together.

Global mutation and realtime projection remain WebSocket-owned. Preferences and
binary upload/read remain authenticated HTTP request/response controls. Neither
transport is a fallback for the other.

The persistence owner, atomic settings transition, existing attachment route,
private control pipe, and typed desktop bridge are active and test-verified. The
audited remote human-session exchange is historical evidence and remains reopened
until D-03 is implemented. The control-pipe boundary performs a real HTTP upload and
pending preview after issuance, and rejects changed server, lineage, room UID, and
malformed asset authority. A remote bound read
must revalidate session, membership, profile binding, room reference, metadata, and
bytes in one SQLite snapshot. Read-only room members may read. Local desktop issuance, the
frontend's strict grant request functions, and the copied settings UI are active.
The controller derives current local manager authority for each upload; remote humans
present their live session only to the exact bound-read target. Canonical private
references are never placed directly in an image element. The frontend fetches the
PNG under resolved local-ticket or remote-session authority, requires the exact
private/no-store metadata and a
nonempty domain-bounded PNG-signature body, publishes only an object URL, rejects late
generations, and revokes the old URL after replacement or on room, directory-authority
currentness, reference, or component-lifetime change.

### Historical remote exchange evidence (superseded by D-03)

The audited remote cutover pays one additional same-origin exchange request
per preference read/write or bound-appearance read, plus bounded session
revalidation. That implementation kept the longer-lived session credential out of
the target route and preserved one-use purpose separation. D-03 rejects that
justification because the second credential does not close a distinct in-scope
threat; the direct target authorization above is the approved contract. Bound
appearance reads first load only metadata, stored byte
length, and room settings; the up-to-10-MiB BLOB is fetched only after current
authority and reference checks succeed. This deliberately adds one short SQLite
query on successful reads to avoid the code-path cost of copying a large BLOB for
rejected requests. Local issuance pays one bounded private-pipe exchange and reuses
the existing manager-resolution query; it adds no cache, lock, persistent frontend
state, trait, configuration layer, or background task. Shared ticket decoding, HTTP exchange,
asset-ID grammar, and transactional asset helpers remain the single owners of
their respective policies; repository-wide duplicate-policy review found no
competing route, SQL, validation, or state-transition owner. The exact typed-
issuance commit passed `make verify`: persistence 178/178, protocol 6/6, provider
120/120, server 85 unit tests plus every integration/TCP suite, desktop 20/20, and
84 frontend files with 518 tests. The corrected frontend batch passed 87 frontend
files with 538 tests and the same complete Rust/TCP/integration gates. Focused request
and lifecycle tests prove exact response parsing, distinct local/remote issuance,
same-reference deduplication, inactive-banner suppression, pending-to-bound
replacement, latest-only upload binding, abort/late-result rejection, explicit retry,
directory-currentness cleanup, and URL revocation. A fresh isolated release package
also uploaded repository PNGs through native file selection after the strict response
checks landed. The copied modal and the main room view rendered the authenticated
banner and icon through object URLs; normal quit stopped the exact app and sidecar,
and only that run's isolated data and regenerable build outputs were moved to
recoverable Trash.

The installed SQLite schema now admits exactly the domain-owned `ra_` plus 32-byte
ASCII lowercase-hex asset-ID language. Its explicit `NOT NULL`, byte-length, and
fixed-position GLOB checks are verified behaviorally against the Rust parser for
canonical, uppercase, nonhex, short, wrong-prefix, embedded-NUL, and null inputs. This
stricter durable contract creates clean schema 43; schema 42 is rejected without
conversion or a compatibility path. The directory owner also exposes one current
manager-authority projection derived from the same live snapshot, epoch, host, sync,
and bound-authority checks used by its exact room resolver. Local appearance projection hides URLs in the
false render, layout-owned cleanup aborts and removes them before publication, and a
currentness fence prevents deferred reads from creating a late object URL. Restoration
re-enters the unchanged exact room resolver and performs a fresh grant/read.

## Failure, acceptance, and review gates

- Invalid input, stale revision, unsupported mutation, missing authority, queue
  corruption, tool race, budget exhaustion, or persistence failure changes no
  authoritative state unless the contract explicitly committed before a later
  reconciliation failure.
- Replay of the same request/action/payload returns its committed result without a
  second event or another settings-progression attempt; conflicting reuse fails.
- Deterministic tests cover ordered/ambient transition timing, multiple active
  turns, delivery batching, read receipt, terminal-first and reservation-first
  barriers, cancellation phases, close tombstones, and parallel/durable 32-call
  budget enforcement. The room-owned transport window charges every decoded raw
  command frame, including replay, before envelope validation and cannot be reset
  by another WebSocket. The separate principal mutation window and durable
  room-wide window are consumed by fresh human commands, lifecycle intents, and
  provider random results; replay and rollback do not consume another mutation or
  durable slot. Only an `agent.stop` that owns real cleanup bypasses those two
  mutation budgets; a fresh already-stopped no-op stop is limited normally. Tests
  also prove that legacy `continuous` values, unknown nested settings fields, and
  older schema records fail instead of being migrated or executed.
- Stage B tests cover two-user isolation, the no-room operator directory branch,
  wrong-purpose/room/asset and replayed tickets, auth-before-body, identity and
  bootstrap races, exact preference merge and cursor semantics, remote-session
  expiry/revocation after ticket issuance, cross-room and read-only rejection,
  shared raster admission, combined cross-writer quotas, pending preview/expiry, reserved-prefix
  rejection, bind/replacement/clear, restart, reference cleanup, response caching,
  and late object-URL revocation.
- Mandatory architecture/source-growth/800-line gates, `make verify`, native and
  Windows warning-denied checks, generated bindings, and packaged copied-UI flows
  must pass. No gate exception is added.
- Before every commit, `git diff --stat` is inspected. Structure, schema,
  persistence, credential contracts, activation, frontend, integration tests, and
  documentation remain independently buildable, verifiable, and rollbackable
  changes. A change of 1,000 lines or more is split unless one mandatory invariant
  or structure gate makes that genuinely impossible. Each implementation change
  carries its focused invariant test; cross-layer, restart, race, packaged-client,
  and verification-record work remain separate.
- Batch timing is owned only by the active `Standing project workflow` in `AGENTS.md`. The exact pushed
  batch is cross-reviewed by the critical web session and Daybreaker Blue High.
  Provider-dependent verification uses persistent Codex Terra, Antigravity Flash,
  and OpenCode Muse Spark sessions, never print mode, and removes every
  verification-owned process, window, server, and temporary resource afterward.

Room close/archive/delete, operator pairing, external-agent admission, custom
channel registry and streams, voice, activity-plugin hosting, general message
attachments, PostgreSQL, RimWorld, and new console/local-profile UI remain
separate contracts. Their controls stay visibly unsupported until their complete
authority boundary is implemented.
