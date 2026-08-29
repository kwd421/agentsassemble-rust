# Lobby message attachments

## Definition

Reconnect the copied lobby composer and message attachment renderer to one durable
Rust-owned upload, message-binding, authorized-read, and provider-read lifecycle.

## Current contract

- This slice owns only attachments on the ordinary lobby `message_final` path. Custom
  channels, votes, message edit/delete, search, and history paging remain unavailable
  until their own message authorities exist. An attachment-only ordinary message is a
  valid reachable message; a message with neither visible text nor an attachment is not.
- `room_events` remains message authority. A separate message-attachment table owns
  pending bytes and, after send, the exact `(room_id, event sequence)` that retains
  them. Event projection carries only bounded public metadata: opaque ID, sanitized
  filename, normalized content type, byte size, safe-image classification, and the
  canonical view/download paths. It never embeds bytes or storage authority.
- Message attachments use the distinct `ma_` plus 32 lowercase hexadecimal ID
  namespace. The shared `/api/attachments/{id}` adapter dispatches that namespace only
  to the message-attachment owner; it does not probe other tables or treat a failed
  message read as a profile/pre-join/appearance fallback.
- One upload is at most 10 MiB and one message references at most eight distinct
  attachments. These are request and response safety bounds, not operating quotas.
  The original's reachable per-uploader `64 items / 128 MiB` and per-room `512 items /
  1 GiB` policies are intentionally not transplanted as fixed security constants. The
  product decision is to keep operating quotas configurable later while code owns only
  absolute safety ceilings; this slice adds no configuration system in advance.
- A pending upload is bound to the exact room and current human principal that created
  it and expires after a bounded hour. Upload and expiry never evict another principal's
  or a referenced attachment. Removing a staged item from the composer leaves it
  eligible only for exact expiry cleanup. The message-attachment owner removes expired
  pending rows on its bounded upload write path before accounting. A failed bind commits
  no cleanup, and the owner adds no background sweeper.
- `message.send` accepts exactly `content` and optional attachment IDs. The persistence
  transaction revalidates the active room, joined and unmuted participant, writable
  session, distinct count, exact pending owner, room, expiry, and unbound state before
  it inserts the event, binds every attachment, records the idempotent result, and
  routes turns. Any failure rolls back all of those changes. A replay of the same
  request returns the committed event; a different request cannot bind the same
  pending object.
- Uploads use fresh one-use operation-, room-, and principal-bound HTTP grants; the
  server creates the opaque attachment ID only inside the insertion transaction. Human
  reads additionally bind the grant to the exact asset. Local grants originate only at
  the typed desktop control boundary; admitted humans exchange their live session
  credential before the attachment route. Raw host secrets, raw reusable session
  credentials at the target, cross-purpose grants, read-only or muted uploaders,
  expired grants, and revoked/left/kicked sessions fail closed. A joined member with
  `room.history` may still read while muted or read-only. Grant authentication happens
  before the bounded body or attachment BLOB is read; after body processing, exact
  current room/session/participant authority is revalidated again in the same SQLite
  transaction as storage accounting and insertion. Bound reads perform their target
  revalidation and metadata check before loading the BLOB.
- A bound attachment read additionally proves that the requested ID is referenced by
  a current visible lobby message in that exact room. Provider reads use the current
  Agent Session's room portal and apply the same canonical-message reference check;
  a merely uploaded or same-room unreferenced object returns `not_found`. Provider
  wake/input includes only the referenced IDs from its canonical pending events.
- Arbitrary files retain their original bytes and are served download-only. Inline
  preview is limited to decoded, bounded PNG/JPEG/GIF/WebP whose declared and detected
  formats agree; active or ambiguous content is never classified inline. Every read is
  private, `no-store`, `nosniff`, and uses a safe content disposition. Provider base64
  output is bounded by the same item limit and is never logged or placed in events. The
  Rust decode boundary owns that classification and validates it again when loading
  stored metadata; the frontend consumes the canonical `is_image` projection and does
  not maintain a second MIME allowlist.
- The copied composer keeps drafts scoped to their room, retains text and staged
  attachments after a failed send, and clears them only after the committed ACK. The
  active upload is owned by one browser operation generation spanning the exact room,
  session, posting authority, role state, and component lifetime. Retiring any of those
  aborts grant exchange, file conversion, and target transfer; currentness is checked
  again after grant issuance and immediately before target dispatch, and a late result
  cannot enter another room's draft. The server has no attachment-grant revoke command,
  so the client does not invent one: an issued but undispatched purpose-bound one-use
  grant remains unusable to the retired client and expires at its existing short TTL.
  Authority-generation retirement runs in the commit layout phase, before any new
  layout observer can release or commit work under the replaced principal.
  The prior unowned path could continue converting one 10-MiB file into a roughly
  13.3-MiB base64 request and dispatch it after authority replacement. Deterministic
  delayed-grant and component-lifecycle tests now prove zero target dispatch and zero UI
  commit after retirement; no claim of measured heap reduction is made.
- The renderer does no transfer work for download-only files at mount and no transfer
  work for images outside the viewport. One LobbyView-lifetime scheduler owns the
  capacity used by every exact room-and-authority reader and admits at most four local
  or remote reads, below the server's eight concurrent grant ceiling;
  the fifth remains queued until an active transport actually settles, even when its
  caller has already aborted, and cancellation before deferred transport entry performs
  no transfer. Intersecting images retain only
  their own generation-owned object URL and revoke it on viewport exit, replacement,
  room or authority change, abort, or unmount. Authority replacement layout-aborts the
  old readers and revokes their object URLs before the new principal can paint, while
  any abort-ignoring transport retains its shared slot until actual settlement. The
  stable capacity owner is not retired by React StrictMode's effect reconnect. Arbitrary
  files read only after an
  explicit click, trigger one programmatic download, and revoke the temporary URL
  immediately. One item failure leaves other successful items intact and retry schedules
  only that item. The replaced `Promise.all` path could mount up to 1,600 simultaneous
  requests and theoretically demand 16 GiB of response bodies for 200 events with eight
  10-MiB files each. Deterministic mount, intersection, click, failure, queue-barrier,
  cancellation, cross-generation, and StrictMode tests now prove zero
  offscreen/download-only starts, a four-read peak, no fifth dispatch before actual
  release, and no retained URL after authority replacement; CPU and wall
  time are not assigned speculative benchmark numbers. Read-only clients expose neither
  upload nor send controls.
- The existing pin row remains only an event pointer. Once attachments are active, its
  target validation accepts a `message_final` with visible text or at least one bound
  attachment, and its existing `attachment_filenames` field is derived from that
  canonical event metadata instead of remaining an unconditional empty array. Pin
  storage never copies attachment IDs, names, bytes, or ownership.
- Profile avatars, pre-join avatars, room appearance, and message attachments retain
  separate SQL/state-transition owners. Their only shared owner contains absolute
  physically retained asset count/byte arithmetic and item-size constants. An expired
  pending row continues to occupy that ceiling until its exact lifecycle owner deletes
  it. Adding message storage to the existing 4,096-item/8-GiB absolute ceiling uses
  checked `current - exact predecessor + new` accounting; it does not create an asset
  trait, registry, repository framework, generic garbage collector, or configuration
  layer.
- Only expired pending objects or deletion of the owning room/event may remove bytes.
  A limit error never deletes current, bound, foreign, referenced, or merely old data.
  Room deletion cascades only that room's message attachments. The future
  message-delete owner must remove its exact bound attachments in the same transaction
  that tombstones the event; this slice does not expose that still-absent command.

Residual availability threat: a writable participant can retain bound message files
until the process-wide 4,096-item/8-GiB ceiling is reached. HTTP connection admission,
body-size, and deadline bounds limit concurrent and per-request work, but there is no
durable per-principal upload-rate or occupancy policy, so they do not prevent eventual
occupancy; at the ceiling, uploads fail closed for every room. That accepted limitation
is recorded rather than hidden behind hard-coded old operating quotas or eviction of
referenced data. A later user-selected configurable operating policy may add fairness at
the same accounting owner without changing attachment custody; this slice does not
speculate about it.

## Non-goals

- No compatibility schema, filesystem mirror, fallback transport, client-owned
  authority, placeholder metadata, derived search index, generic attachment service,
  speculative cache, background sweeper, configurable operating-quota layer, or old v0
  scripted-meeting behavior.
- No custom-channel, vote, message edit/delete, search, or history-paging attachment
  support in advance of those product owners.

## Acceptance criteria

1. Local and writable remote humans upload up to eight real files, remove staged items,
   send text-plus-attachment and attachment-only messages, render safe images, download
   other files, and retain the exact message and bytes after restart.
2. Message insertion, attachment binding, command replay, and turn routing are atomic.
   Duplicate, foreign-room, foreign-principal, expired, already-bound, missing, oversized,
   malformed, and ninth IDs fail without a message, binding, or partial durable change.
3. Read-only and muted authority cannot upload or bind; left/kicked, revoked,
   wrong-purpose, wrong-room, replayed, and expired authority cannot gain access. A
   joined read-only or muted member with `room.history` can read a referenced message
   attachment. The target authenticates before body admission, and unreferenced
   same-room attachments are unreadable to humans and agents.
4. Ordered and ambient Agent Sessions receive the exact attachment IDs with canonical
   room context. Codex Terra, Antigravity Flash, and OpenCode Hy3-free each exercise the
   real attachment path when that provider-visible boundary is complete; no transcript,
   print-mode, fake provider, or alternate attachment fallback is used.
5. Expiry and room deletion remove only their exact pending/bound rows. Absolute storage
   accounting spans all four asset owners once, counts every physically retained row
   until its owning lifecycle deletes it, uses checked replacement arithmetic, and
   never restores the removed generic per-subject or per-room quotas.
6. Existing pins accept attachment-only messages and project canonical attachment
   filenames without adding pin-owned attachment state. Other message, profile-avatar,
   pre-join-avatar, room-appearance, admission, reconnect, ordered/ambient, and pin
   contracts remain unchanged. Incomplete adjacent controls remain visibly unavailable.

## Verification path

- Schema and persistence tests cover ownership constraints, exact expiry, checked total
  accounting, a cross-owner expired-row regression proving retained bytes remain
  charged, transactional binding/replay/races, attachment-only routing, and room
  cascade cleanup.
- Real TCP HTTP tests cover purpose separation, auth-before-body, request bounds,
  current-session revalidation, safe disposition, content-type mismatch, private reads,
  and writable/read-only behavior. WebSocket tests cover exact payload validation,
  atomic ACK/event projection, retry, ordered/ambient wake IDs, and referenced versus
  unreferenced provider reads.
- Focused copied-frontend tests cover upload, staging/removal, failed-send restoration,
  authorized render/download, object-URL cleanup, and disabled read-only controls. Run
  the full frontend suite/build, `make verify`, and an isolated packaged Computer Use
  flow for local, writable remote, read-only, restart, and exact resource cleanup.
- Measure upload resident allocation/latency and stored bytes before changing the
  existing encoding path. Record any material optimization with prior cost or threat,
  owning boundary, preserved contracts, trade-off, and measured verification; do not
  add a cache or transport abstraction from intuition alone.
- Commit each buildable, independently verifiable and rollbackable change below 1,000
  changed lines. Push at three completed features or 2,000 aggregate changed lines,
  then obtain manual web-session and Daybreaker reviews for security, structure,
  duplicated policy, overimplementation, SSoT, lifecycle cleanup, and removable state.
