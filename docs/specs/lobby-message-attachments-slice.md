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
  wake/input includes only the referenced IDs from its canonical pending events. The
  persistence assignment derives those IDs from the same strictly decoded events after
  selecting the bounded inflight prefix, stores them with the durable provider-turn
  envelope, and recomputes the exact list from the session's inflight event authority
  before restart recovery. Attachment-only messages remain routable and their room
  view names each opaque ID, filename, content type, and byte size without loading or
  copying the BLOB. `read_attachment` is an exact active-turn Room Portal tool: its
  per-room channel carries the session, turn, generation, execution, input cursor, and
  requested ID back to the room owner. SQLite recomputes the inflight ID set and checks
  the current joined, unmuted participant and `start_dispatching` execution before it
  loads one BLOB. The portal validates the returned metadata/size and base64-encodes
  only that requested item at the standard MCP content boundary: verified images are
  image blocks and other files are embedded blob resources. Antigravity's original
  `agentsassemble-room media <id>` behavior remains a private file path rather than
  terminal base64. Its helper lazily writes only the requested item as a `0600` file in
  the runtime-owned directory and clears that turn projection on normal completion,
  abort, and the next turn. This choice is required by an observed bound: one 10-MiB
  item encodes to 13,981,016 bytes while the Antigravity terminal tail retains only
  64 KiB. It avoids preloading up to eight 10-MiB items and bounds temporary disk to
  items the agent actually requests; no unmeasured latency or resident-memory reduction
  is claimed. Cross-review then demonstrated that connection concurrency alone left
  cumulative work unbounded: the same accepted ID could repeatedly cause a 10-MiB
  SQLite read, 13.3-MiB base64 allocation, and Antigravity file sync while helper output
  refreshed its inactivity deadline. The active turn now owns the smallest complete
  ledger: one pending read per ID, at most two attempts (and therefore successes) per
  listed attachment, and checked successful bytes bounded by twice the canonical
  eight-item/10-MiB input ceiling. The second attempt preserves one bounded retry when
  an MCP response or Antigravity's post-response staging is lost.
  Failed or cancelled reads release only their reservation but consume an attempt;
  finish and terminal actions wait for pending reads, while abort retains a tombstone
  only until those reservations release. Deterministic tests cover concurrent duplicate,
  repeated success, failed-attempt exhaustion, terminal finish, and abort cleanup. This
  preserves exact-turn authority and bounded retry without a generic
  rate limiter, cache, background task, or new operating policy.
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
  work for images outside the viewport. One AppView-lifetime scheduler owns the
  capacity used by every exact room-and-authority reader and survives lobby replacement
  by another channel, admin, or plugin view. It admits at most four local
  or remote reads, below the server's eight concurrent grant ceiling;
  the fifth remains queued until an active transport actually settles, even when its
  caller has already aborted, and cancellation before deferred transport entry performs
  no transfer. Intersecting images retain only
  their own generation-owned object URL and revoke it on viewport exit, replacement,
  room or authority change, abort, or unmount. Authority replacement layout-aborts the
  old readers and revokes their object URLs before the new principal can paint, while
  any abort-ignoring transport retains its shared slot until actual settlement. The
  stable capacity owner is neither retired by React StrictMode's effect reconnect nor
  recreated by lobby unmount and re-entry. Arbitrary
  files read only after an
  explicit click, trigger one programmatic download, and revoke the temporary URL
  immediately. One item failure leaves other successful items intact and retry schedules
  only that item. The replaced `Promise.all` path could mount up to 1,600 simultaneous
  requests and theoretically demand 16 GiB of response bodies for 200 events with eight
  10-MiB files each. Deterministic mount, intersection, click, failure, queue-barrier,
  cancellation, cross-generation, view-re-entry, and StrictMode tests now prove zero
  offscreen/download-only starts, a four-read peak, no fifth dispatch before actual
  release, and no retained URL after authority replacement; CPU and wall
  time are not assigned speculative benchmark numbers. Read-only clients expose neither
  upload nor send controls.
- The remote browser keeps the standard object-URL anchor download owned by the browser.
  The packaged macOS WebKit client instead uses one exact native save command after the
  same authorized scheduler read. This boundary was added only after the release bundle
  reproduced a main-thread hang on both file-card and image-preview `blob:` downloads:
  WebKit attempted an invalid main-frame request (`requestURLIsValid=0`), Wry received no
  completion, and the whole UI stopped responding. The desktop command accepts only the
  bundled caller, a raw body between one byte and the shared 10-MiB absolute item ceiling,
  and the domain owner's canonical message filename. JavaScript cannot provide a target
  path; the native save panel selects it, cancellation is explicit, and a write failure is
  returned rather than falling back to another transport. Raw IPC avoids an additional
  JSON/base64 expansion, but it still incurs one bounded `Blob.arrayBuffer()` conversion
  and one native body clone before the blocking file-dialog/write worker; no CPU, latency,
  or resident-memory improvement is claimed beyond removing the observed deadlock and
  avoiding base64's known size expansion. A manual review then identified that a direct
  final-path write could follow a selected symlink, truncate an existing file before a
  later write failure, or leave partial bytes after `ENOSPC`; a second review demonstrated
  that a pathname-based named temporary file still left its parent and source entry
  replaceable in a shared directory. The final desktop owner uses Bytecode Alliance's
  maintained capability filesystem rather than implementing path traversal or Windows
  reparse handling. It opens every absolute parent component without following links and
  then performs target inspection, staging creation, and rename relative to that retained
  directory handle. The staging directory is `0700` and current-user-owned on Unix; on
  Windows its opened handle receives and verifies the existing owner-only inheritable
  DACL policy. Its payload is likewise `0600` or owner-only, is fully written and synced,
  and moves from the retained private directory into the retained destination. An existing
  symlink or other non-regular target is rejected; a hard link or target changed after the
  check is replaced as a directory entry rather than opened. The accepted cost is one
  private directory, one temporary file, component-wise handle opens, one full write, one
  file sync, and one rename per explicit save, plus two maintained direct capability crates
  and nine newly locked transitive packages; no directory-sync or crash-durable filename
  claim is made. The same owner handles file cards and image preview downloads, so WebKit
  never re-enters the failing path through a second UI.
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

Observed packaged download verification on 2026-08-29 used an isolated release bundle
and authority. The local desktop file card saved the 3,307-byte `README.md` with SHA-256
`f67361205428e74aea136e126fc7fdf4ccf66007d6e839a9df4e815e79a9eae1`; the image-preview
download saved the 6,684-byte `deepseek.png` with SHA-256
`67686a1ac38f8d1cf6db6949e566d69a7e975ad74c610c0f10872da5c39f4fdf`. Both matched
their uploaded source bytes and left the packaged UI responsive. A writable remote
Chrome client then uploaded the real 8,332-byte `grok.png` through the copied composer,
and both remote and host timelines rendered the committed message. A completely fresh
read-only invite removed its secret from browser history, projected every referenced
attachment, exposed no usable composer or upload control, and downloaded the same
3,307-byte README with the same SHA-256. Normal shutdown and relaunch restored the local
and remote messages and lazily rendered the remote image again. The UI stopped its owned
public ingress, the exact incognito window and bundle were closed, the original ignored
sidecar was restored byte-for-byte, and only the isolated app data, caches, WebKit data,
and build root were moved to a recoverable Trash folder; no owned app, server, or
`cloudflared` process remained. The implementation then passed `make verify`, including
the mandatory architecture/800-line gates, copied production frontend and original-CSS
check, 93 frontend files with 591 tests, 26 Tauri tests, all Rust tests including the
real TCP attachment boundary, and warning-denied Clippy. No real provider was started
for this desktop-only download correction because it does not change the already
verified canonical Agent Session read boundary.

After the capability-relative replacement correction, a newly built release bundle
repeated the file-card save through the native panel into a canonical non-symlinked
parent. The resulting `README.md` was again exactly 3,307 bytes with the same SHA-256
and byte content, was owner-only (`0600`), and did not leave a staging entry. Opening
and closing channel search after the save proved that the packaged UI remained
responsive. Normal quit stopped the exact desktop, supervisor, and server processes;
the one owned executable-staging directory that remained after process exit was
verified unopened and removed, while app state, WebKit/cache data, package, and saved
evidence returned to the isolated recoverable Trash root. The ignored sidecar was
restored to SHA-256
`9bb26e769cdad1a0c3949b674cb41e845c2a8e78edd652008f70a69963025aea`.

After the atomic-save correction, the same isolated release package saved the
3,307-byte README again with the same SHA-256 and exact byte comparison, and the
packaged UI remained responsive while opening and closing channel search. The regular
replacement and Unix symlink-rejection tests cover the final-path policy directly.
The exact app and owned server were then stopped; no public ingress or provider had
been started, the original ignored sidecar remained byte-identical, and the isolated
runtime state, caches, WebKit data, package, and output were returned to the same
recoverable Trash folder.

Observed follow-up: an interrupted macOS provider/server test run left 159
`agentsassemble-*-exec-*` executable-staging directories (more than 10 GiB total),
causing a later guardian-readiness test to fail with `ENOSPC`; one deterministic
guardian-death run reproduced a 64-MiB orphan. Commits `af6297d`, `11fa808`, and
`afd3997` fixed that separate lifecycle owner rather than hiding cleanup inside the
attachment path. Filesystem-staged provider images and Unix private companion copies
now share one provider lease root, while the running desktop image and server sidecar
share one distinct desktop lease root. Linux and Android provider images retain their
sealed `memfd` execution path. Active leases are retained and only unlocked crash
directories are reclaimed by the next creation or owner drop. The final
forced-death/full-verification run left no old executable directories and only a
zero-byte lock in each managed root.
