# Lobby message attachments

Status: implementation and flow evidence retained; remote-human HTTP authorization
reopened by repository audit D-03

## Definition

Reconnect the copied lobby composer and message attachment renderer to one durable
Rust-owned upload, message-binding, authorized-read, and provider-read lifecycle.

## Approved target contract after Phase 0B

- This slice owns only attachments on the ordinary lobby `message_final` path. Custom
  channels remain outside this owner; votes, message edit/delete, search, and history
  paging are separate implemented message authorities rather than attachment state. An attachment-only ordinary message is a
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
- Local uploads use fresh one-use operation-, room-, and principal-bound HTTP grants;
  the server creates the opaque attachment ID only inside the insertion transaction.
  Local grants originate only at the typed desktop control boundary, and human reads
  additionally bind local grants to the exact asset. Admitted remote humans present
  their durable session credential in the bounded Authorization header at the target
  upload/read route, which resolves the
  exact room, principal, operation, and asset before access. The audited preliminary
  session-to-purpose-ticket exchange is a Phase 0B removal target. Raw issuer secrets,
  cross-scope credentials, read-only or muted uploaders,
  expired local grants, and revoked/left/kicked sessions fail closed. A joined member with
  `room.history` may still read while muted or read-only. Authentication happens
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
  loads one BLOB. The portal validates the returned metadata/size and projects only
  that requested item at the standard MCP content boundary: verified images are image
  blocks, byte-valid UTF-8 is the first text block followed by the helper-only metadata
  block, and remaining binary is an embedded blob resource. The UTF-8 branch was added
  only after the packaged Codex flow read the exact 59-byte metadata but reported that
  both the original blob and a later embedded text resource body were unavailable. The
  first text block is the smallest provider-consumable representation observed at the
  real boundary; it avoids base64's 4/3 expansion and JSON escaping for text without a
  MIME allowlist or a change to stored bytes, binary behavior, authority, item limit, or
  retry budget. The terminal helper still validates the separate descriptor before it
  stages those exact bytes.
  Antigravity's original `agentsassemble-room media <id>` behavior remains a private
  file path rather than
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
  aborts local grant issuance or remote target authorization, file conversion, and
  target transfer; currentness is checked again after authority resolution and
  immediately before target dispatch, and a late result
  cannot enter another room's draft. The server has no attachment-grant revoke command,
  so the client does not invent one: an issued but undispatched local purpose-bound
  one-use grant remains unusable to the retired client and expires at its existing
  short TTL.
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
  or remote reads;
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
  directory handle. The staging directory is `0700` and current-user-owned on Unix. On
  Windows, the named parent must still have the same file identity as the retained parent
  handle before the safe native create call installs a protected, current-owner-only,
  inheritable DACL at creation time. The unpredictable staging name is then opened without
  following reparse points relative to the retained parent, with read/write/delete sharing
  denied, and its current owner plus exact inheritable DACL are verified through that
  retained handle before payload creation. Share denial alone does not cover `WRITE_DAC`;
  creation-time ownership and DACL placement prevent a different parent-authorized
  principal from substituting an object it owns and using the owner's implicit `WRITE_DAC`
  during payload creation. If the parent path is redirected after the identity check, the
  relative open or owner/DACL validation fails closed; the implementation does not delete
  an object it could not validate, so an adversarial race may leave one empty private
  directory outside the retained parent. Safe Win32 wrappers currently require the selected
  Windows path to be valid Unicode, and an unrepresentable path is rejected rather than
  falling back to a racy creation path. Its payload is likewise `0600` or owner-only, is
  fully written and synced, and moves from the retained private directory into the retained
  destination. Before that rename the desktop
  owner also writes the platform download-origin marker: macOS `com.apple.quarantine` with
  the observed browser-download `0083` flags, or Windows `Zone.Identifier` with `ZoneId=3`.
  Marker storage failure rejects the save instead of producing an unmarked fallback. Other
  Unix targets have no common equivalent marker. Existing target metadata is not copied
  wholesale because the original browser-owned download had no such product contract and
  copying arbitrary ACLs or extended attributes could preserve unsafe state. An existing
  symlink or other non-regular target is rejected; a hard link or target changed after the
  check is replaced as a directory entry rather than opened. The accepted cost is one
  private directory, one temporary file, component-wise handle opens, one full write, one
  platform marker write, one file sync, and one rename per explicit save, plus the two
  maintained direct capability crates, one maintained macOS xattr crate, and a direct
  declaration of the already locked Windows API crate; no directory-sync or crash-durable
  filename claim is made. The same owner handles file cards and image preview downloads, so
  WebKit never re-enters the failing path through a second UI.
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
   room context. Codex Terra, Antigravity Flash, and OpenCode Muse Spark each exercise the
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
  changed lines. Batch timing is owned by `docs/PRODUCT_REIMPLEMENTATION_PLAN.md`;
  then obtain manual web-session and Daybreaker Blue High source reviews for security,
  structure, duplicated policy, overimplementation, SSoT, lifecycle cleanup, and removable
  state. The Standard Scan already started for pushed HEAD `b46aa02` is a one-time review;
  Deep Scan and later automated scans are not part of this workflow.

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

After the Windows-handle and download-origin correction, a package built from the final
source repeated that exact save. The 3,307-byte result again matched SHA-256
`f67361205428e74aea136e126fc7fdf4ccf66007d6e839a9df4e815e79a9eae1` and an exact byte
comparison, retained mode `0600`, carried a real `com.apple.quarantine` value beginning
`0083;`, and left no save-staging entry. Channel search toggled on and off after the save.
Normal quit stopped the exact desktop, supervisor, and server chain; the sole remaining
owned executable-staging directory was unopened and removed, the ignored sidecar still
matched SHA-256
`9bb26e769cdad1a0c3949b674cb41e845c2a8e78edd652008f70a69963025aea`, and the isolated
runtime state and package returned to the recoverable verification root. Focused macOS
tests, the full `make verify`, warning-denied Clippy, and a Windows all-target/all-feature
cross-check passed. The Windows exclusive-handle test is compiled for its native target,
but packaged Windows execution remains explicitly unverified without a Windows host.

The subsequent Windows `WRITE_DAC` correction was verified by a warning-denied Windows
all-target/all-feature cross-build and the full host `make verify`: architecture and
800-line gates, copied production frontend and original-CSS verification, 93 frontend
files with 591 tests, 26 desktop tests, every Rust/TCP/integration/doc test, Clippy, and
the diff gate passed. Windows-only native tests now cover creation and retained access to
the private staging directory plus the existing-handle/later-handle sharing boundary.
They compile but cannot run on the available macOS host, so Windows packaged execution
and cross-principal runtime behavior remain explicitly unverified until a Windows host is
available.

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

Packaged Codex verification on 2026-08-29 used the exact `gpt-5.6-terra` catalog
selection and an attachment whose first line was not present in the prompt. The
original embedded blob and the intermediate embedded text resource each exposed the
59-byte metadata but not the body. A release bundle built from the final source then
returned `CODEX_PORTAL_PROOF | PORTAL-CODEX-0829-cf7e3a | 59`, proving the first text
block carried both the previously unknown first line and the exact byte count through
the real Room Portal tool call. Focused provider tests and the full `make verify`
passed, including the architecture/800-line gates, copied production frontend and
original-CSS check, 93 frontend files with 591 tests, 26 desktop tests, every
Rust/TCP/integration/doc test, and warning-denied Clippy. The exact Agent Session was
stopped, the package was quit normally, and no owned desktop, server, or provider
process remained. The stop finalized as the existing fail-closed recoverable
`disconnected` state because runtime authority could not be confirmed; that separate
lifecycle behavior is not claimed as part of this attachment correction.

The same provider-visible boundary was then exercised by exact OpenCode catalog model
`opencode/hy3-free`; its packaged reply was
`OPENCODE_PORTAL_PROOF | PORTAL-OPENCODE-0829-a8e4d2 | 57`, again proving the unknown
first line and byte count came from the canonical attachment rather than the prompt.

Antigravity CLI 1.1.22 exposed two additional native-PTY permission boundaries. Its
current `PreToolUse` contract uses `decision` rather than the obsolete `overwrite`
field, and a user-level ask hook takes precedence over a project allow. The first real
run also showed that the long private helper path wraps inside the terminal permission
card, so independently parsing the rendered command in both the hook and PTY rejected a
command that the hook had already validated. The managed hook is now the single policy
owner: it accepts only one exact private-helper command or a `view_file` whose
`AbsolutePath` resolves to a current regular owner-only file exactly two components
under that turn's private `room-media` root. It writes a typed, one-use, owner-only
receipt of at most 12 bytes. The PTY classifies only the two observed native cards and
responds when that card type matches the consumed receipt; helper commands retain the
conversation-scoped exact-prefix choice, while staged files select one-time access and
never the provider's persistent non-workspace grant. Invalid pre-hooks and matching
post-hooks clear the receipt, and observation begin, finish, abort, or final private
runtime-directory removal clears both approval and media state. This removes the
duplicate terminal command parser and adds only one bounded file write, read, and unlink
per prompted approved operation; the receipt is not synced because it coordinates a
live process and makes no crash-durability claim. No background task, cache, generic
permission framework, model substitution, print mode, or fallback was added.

Cross-review of that correction found a same-workspace multi-session ownership defect.
The shared workspace hook intentionally keeps the first live helper executable as its
stable dispatcher, but the hook had also used that dispatcher's parent for the one-use
approval receipt and staged-media state. A second Agent Session therefore validated and
consumed state in its own private directory while the shared hook wrote into the first
Session's directory. Hook state now resolves from the exact process-local absolute helper
command belonging to the current Session. The owner rejects a noncanonical command,
symlink, non-file helper, non-private Unix helper, or non-private parent before reading or
writing state. A deterministic two-Session regression keeps one shared workspace hook,
routes the second Session's approval into its own directory, consumes it there, and proves
the first directory remains untouched. The accepted steady-state cost is one local helper
and parent metadata validation per pre/post hook; no task, cache, durable state, generic
registry, retry, or fallback was added. All 133 provider tests and the complete repository
verification passed.

The same verification exposed a pre-existing Windows-only OpenCode compile error: its
non-Unix spawn path called the existing protocol-owned `spawn_error` constructor without
importing it. Importing that owner is the whole correction and adds no new error policy or
compatibility path. A Windows all-target/all-feature source cross-check then passed. The
warning-denied Windows Clippy invocation still reports unrelated existing Windows-only
lint debt, and native Windows execution remains unverified without a Windows host; neither
is represented as passing evidence for this change.

A fresh packaged release then ran the copied UI with exact model
`gemini-3.6-flash`, Medium effort, and room-read-only permission. The model invoked the
private helper, opened its staged non-workspace file through native `ViewFile`, and
published `AGY_VIEW_FINAL | AGY-VIEWFILE-0829-7cb9e4 | 106`; neither the first line nor
byte count appeared in the prompt. The provider returned to idle, its exact Agent
Session and `agy` process were stopped, the package and server quit normally, Computer
Use was reset, and only isolated verification data and the regenerable package were
moved to recoverable Trash. Focused hook/card tests and all 132 provider tests passed.
The full `make verify` gate then passed: repository architecture and source-growth
policy, formatting, generated type parity, the production frontend build and original
CSS check, 591 frontend tests, 26 desktop tests, every workspace and TCP-boundary test,
and warning-denied desktop/workspace Clippy.

That packaged run also exposed a copied-frontend projection defect: durable
`turn_started` and `turn_state` events are authored by the server's `room-system`
actor but name the affected Agent Session in `participant_id`. Timeline authorship
correctly remains actor-owned, while transient progress now uses that explicit subject
identity. This prevents a second `room-system` typing row without changing the durable
event, session authority, or typing lifecycle and adds no new state or polling.

Computer Use then drove a fresh isolated release package named
`AgentsAssemble Typing Verify`, bundle identifier
`app.agentsassemble.rust.typingverify0829`. The accepted Agent Session was verified in
both durable state and its live process arguments as exact `gpt-5.6-terra`, Low effort,
and room-read-only permission. During a real turn, the copied chat rendered exactly one
`Codex · GPT-5.6-Terra` / `입력중...` row and the right panel showed that same Session as
responding; no `room-system` progress row appeared. The model published
`TYPING_ROW_TERRA_OK`, the transient row disappeared, and the Session returned to idle.
An earlier native model-menu attempt that resolved to default Sol was stopped and
excluded from the evidence. The accepted Terra Session was stopped through the product
UI, its exact process was absent, the package and owned server quit, Computer Use was
reset, and only the isolated app data, cache, WebKit state, package, and discarded
verification capture were moved to recoverable Trash at
`AgentsAssemble-Typing-Verify-0829.bJxZw3`.
The focused projection/typing/socket tests passed 36 cases, the complete frontend
suite passed 592, and the final `make verify` passed every architecture, source-growth,
formatting, generated-binding, original-CSS, desktop, workspace, TCP/integration,
documentation, warning-denied Clippy, and diff gate.

Manual critical-web review of pushed range `c6cf861..b46aa02` returned `APPROVE`
with Critical 0 / High 0 / Medium 0 / Low 0 and confirmed that the shared-workspace
Antigravity Session-state defect is closed. One authorized Codex Security Standard
Scan (`dd07d7d4-3762-4d15-8ca3-7ea0ed8d4529`) returned `REVISE` with Critical 0 /
High 1 / Medium 2 / Low 0. Its findings are: Windows helper approval applies POSIX
single-quote semantics to a command executed by `cmd.exe`; unauthenticated pre-join
avatar upload reads the bounded JSON body before validating invite and browser
credentials; and one writable participant can retain pending attachments until the
process-wide asset ceiling is exhausted. The first two require corrections at their
current owners. The third must be resolved against the explicit no-fixed-operating-quota
contract above rather than restoring the original per-uploader or per-room constants.

The Standard Scan High was confirmed at the Antigravity hook's command-policy owner:
POSIX single quotes do not quote `cmd.exe`, so the prior validator could approve a
Windows command whose hidden `&` started a second process. Windows validation now uses
only its accepted shell grammar: single quotes, control characters, `%`/`!` expansion,
and caret escaping are rejected, and command operators are accepted only inside balanced
double quotes. The shared product-command parser remains unchanged, while the generated
Windows prompt uses double-quoted examples and the Unix prompt retains its existing
single-quoted form. The added cost is one bounded character pass in the existing hook;
there is no task, state, fallback, generic parser, or provider-specific duplicate command
policy. Host tests exercise the exact reported injection and expansion cases, and the
Windows all-target/all-feature source cross-check passes. Native Windows execution remains
unverified until a Windows host is available.

The first Standard Scan Medium was also confirmed. The public pre-join avatar path
previously needed invite and browser credentials from the JSON payload, so one invalid
request could make the server retain and parse up to the existing roughly 13.4-MiB
base64 request bound before rejection. The upload transport now carries those two
canonical credentials in single-valued bounded headers, authenticates the invite and
current durable pre-join custody before body admission, and then preserves the existing
second durable revalidation in the final image-storage transaction. Other attachment
purposes continue to consume their one-use authorization before reading the body. The
frontend no longer duplicates room or credential authority in that upload body. This
uses the existing 128-connection and 30-second HTTP bounds rather than adding an
unmeasured limiter, ticket store, retry, or compatibility path. A real TCP regression
declares a 14-MiB body, sends no body bytes, and receives the invalid-invite rejection
within two seconds; focused invite/profile tests, the exact frontend request test, and
the production frontend build pass.

The second Standard Scan Medium is a valid availability observation but its proposed
fixed participant/room quotas are not accepted for this slice. A repository-wide policy
search confirms that current code has one physical-retention owner only:
`asset_storage.rs` applies the 4,096-item/8-GiB absolute ceiling across all four asset
tables, while `message_attachments.rs` alone deletes expired message-pending rows on its
write path and schema foreign keys alone delete event- or room-owned bound rows. No active
per-uploader or per-room count/byte policy remains. Limiting only pending rows would not
close the reported outcome because the same writable participant can bind eight files to
successive ordinary messages and continue retaining them; choosing a fairness threshold
therefore remains an operating-product decision, not an absolute security constant. The
accepted residual process-wide occupancy threat above remains explicit until the user
selects a configurable policy at that same accounting owner. No eviction, speculative
rate state, background cleanup, or replacement of a referenced asset was added. Focused
tests confirm exact pending expiry, canonical bound ownership, cross-owner retained-row
accounting, and the deliberate absence of the removed generic uploader/invite quotas.

Manual cross-review of pushed range `b46aa02..27b2c07` found no additional blocking
defect. The critical web review initially returned `REVISE` with one Medium for the
same process-wide occupancy path, then returned `APPROVE — ACCEPTED RISK` after
separating the user's explicit product decision from defects inside that contract:
Critical 0 / High 0 / Medium 0 blocking / Low 0, with one accepted non-blocking
Medium. Daybreaker Blue High independently returned the same final classification and
approved all four commits and the cumulative range. The retained-capacity fairness
threat remains recorded above; neither review erased or downgraded it.
