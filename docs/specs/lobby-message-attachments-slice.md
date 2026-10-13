# Lobby message attachments

Status: implementation and flow evidence retained; remote-human HTTP authorization
cut over at `e0622cb`, with final D-03 manual cross-review closed at `5693e13`

## Definition

Reconnect the copied lobby composer and message attachment renderer to one durable
Rust-owned upload, message-binding, authorized-read, and provider-read lifecycle.

## Transfer rework: storage prerequisite (slice 1, 2026-10-12)

Persistence owns one bounded content interface for all room message attachments
and connector uploads. SQLx 0.9 has no approved safe incremental-BLOB route on
the existing connection (settled owner decision); schema 93 uses child chunk rows
for every file, with no retained whole-BLOB representation. Each storage read or
write step transfers one piece of at most 64 KiB; SQLx transient copies are fixed
multiples of that bound, independent of file size. The final piece is shorter. This bounds driver copies while keeping a 50 MiB file to 800
rows. No spool, second runtime connection/driver, unsafe code, or transaction
across network reception is introduced. A piece append is one SQL statement;
the existing upload boundary still inserts a completed file in its atomic local
transaction. Later reception can use one-piece transactions on the same writer
lease, releasing the writer between pieces. This slice does not claim to bound
existing whole-body HTTP, download, or raster-validation allocations.

Affected entry points: human/local uploads and authorized downloads; managed
provider, attendee and connector attachment reads/uploads; message.send pending
binding and connector publication; backup export, server move and staged archive
restore/reconstruction; retained storage accounting, expiry and parent/message
deletion. All content access uses this persistence owner. Custody, identical
bytes, sizes, room/server quotas, file limits, atomic publication and failures
remain unchanged; metadata-only accounting counts each parent once.

Schema 92 -> 93 is transactional: refuse an unexpected source shape or an
existing replacement object, copy metadata by column name, split BLOBs inside
SQLite and verify every total length before committing. Failure rolls back the
conversion and version update. The frozen archive catalog retains 87–92, with
staged upgrade before Prepared and truthful historical-field provenance. New
archives use chunk rows and need no >16 MiB attachment-cell exception; the outer
envelope and early too-new-manifest refusal remain unchanged. Historical file
cells copy by named column inside SQLite into the existing historical staging
schema, then the normal staged migration converts them; SQLx never fetches or
binds an entire historical file cell. Existing raster validation alone assembles
compressed input from piece reads under its current decoder budget. As with previous
schema bumps, newer archives require an equally new or newer receiving app.

Acceptance: byte-exact piece and whole reads at non-aligned sizes and 50 MiB;
genuine schema-92 conversion and hostile-shape rollback; 87–92 archives restoring
into current storage; deletion/expiry leaving no pieces; unchanged quota races
and existing upload -> bind -> download flows. No wire, route, client, UI,
download-budget, upload-protocol, message-count or raster-policy change.

Verified for this slice: `cargo test -p agentsassemble-persistence` (512 passed,
one ignored), server integration filters `message_attachments_boundary` (4),
`connector_boundary` (22), and `server_move_process` (1), plus `cargo fmt --all`
and `make architecture-check`. Coverage includes piece/whole equality at 50 MiB,
genuine schema-92 hostile-shape/rollback cases, 87–92 staged restore including
rasters, quota races, deletion/expiry, and real loopback HTTP/native move flows.
No transfer-throughput, peak-RSS, writer-occupancy or packaged-UI measurement was
performed; whole-file/raster boundaries remain for the subsequent slices.

## Transfer rework: streamed human downloads (slice 2, 2026-10-12)

Both `GET /api/attachments/ma_*?view=1` and `?download=1`, through plain
HTTP and `secure_http`/`secure_channel`, serve the same bytes and headers with a
truthful Content-Length. Neither transport assembles a whole room file. Each
64 KiB piece is read in a short transaction that revalidates the exact local
identity or durable room session, history permission and canonical message
reference; the connection is released before any network await. Attachment,
message, room, membership, session or account deletion/revocation interrupts the
body with an error, never a successful short EOF.

Streamed admission charges a fixed 1 MiB allowance per response (SQLx piece
copies, one yielded 64 KiB piece and the bounded HTTP write buffer, with margin),
inside the existing shared 512 MiB host envelope. A principal may have 32 streams
(32 MiB), beside the existing sixteen retained small/AI responses; their combined
weight still cannot exceed 400 MiB. Ten 50 MiB downloads plus four visible images,
room icons and reads from other devices therefore fit without a file-size refusal.
Encrypted queued records remain separately charged by the unchanged 8 MiB global /
512 KiB per-direction gates. No whole-file prefetch or sliding credit window.

Reuse body-policy deadlines: 30 seconds idle, at least 64 KiB per 120 seconds,
and a one-hour ceiling. Transport owners enforce expiry even without body polling,
drop the response and release admission; connection/channel close and virtual
request cancellation also release it. Only nonempty pieces accepted by the bounded
transport count. With the existing channel-wide stop-and-wait ACK, download expiry
closes the secure channel: a stalled ACK can also prevent an error record from
reaching the peer. Explicit child cancellation continues to preserve the channel.
AI/provider/attendee/connector base64 JSON reads and other asset tables retain
round-4 whole-value admission. Routes, wire records, clients and UI are unchanged.

Acceptance: byte equality at non-piece-aligned sizes and 50 MiB on both transports;
ten concurrent large streams by one principal and bounded host weight; controlled
stalls, progress expiry, cancellation and mid-stream deletion/access loss; an
unrelated database write completes while network consumption is stalled.

Verification: persistence crate tests (514 passed, one ignored), server unit tests
with four test threads (174 passed, one ignored), `message_attachments_boundary`
integration (five passed), and the production WebCrypto/RemoteTransport encrypted
HTTP/SSE/socket integration (one passed). Both real TCP transports preserve exact
non-aligned and 50 MiB bytes, including ten concurrent 50 MiB reads by one principal.
The encrypted flow also aborts a child request and interrupts an already-started
download on access loss. Controlled-clock tests exercise unpolled stall expiry,
piece lifetime/admission release and unchanged secure queue maxima; unrelated
writes complete beside stalled bodies. `cargo fmt --all`, `git diff --check` and
`make architecture-check` pass without gate changes. The larger encrypted fixture
reports batch start/file completion to its existing 30-second harness waits; product
deadlines are unchanged. A default-parallel unit run hit the existing four-worker
runtime-authority pool; its gate was left unchanged and the four-thread run passed.
No peak-RSS, paced WAN throughput, packaged UI or provider verification was run.
SQLx piece bounds and Hyper's bounded writer establish the allocation envelope;
client whole-Blob retention and the ACK throughput ceiling remain later slices.

## Transfer rework: headless client (slice 5, 2026-10-13)

Picking, dropping and pasting stage local Files only; refuse a selection whole if
it exceeds the current host's message limit. Send snapshots text, reply and ordered
files into one outgoing message and frees the draft immediately. One client owner
per host/person schedules at most three files across rooms/messages; structured
host defer returns work to its queue with the host retry hint, never a failure.
Expose immutable snapshots/subscription, identity (client ID, room, person and
authority generation), message/file states, sent/accepted/total bytes, cancellation,
retry and discard. Publication uses the existing socket owner only after all files
finish, retaining completed attachments on retry and its same-request retry callback
when publication is uncertain. Delivery reconciles a `message_final` by room,
participant and the complete ordered attachment ID list, including before ACK.

Affected owners: `frontend/src/api/messageAttachmentTransfers.ts`,
`frontend/src/lib/outgoingMessages.ts`, `lobbyComposerModel.ts`, remote transport
and capability observation, domain attachment count and generated bindings. Keep
legacy upload unchanged for hosts without transfer advertisement; preflight current
room file policy. Plain/desktop data bodies are Blob slices; encrypted transport
pulls bounded pieces through negotiated binary records/shared credit. Interrupted
data and lost finish replies resolve via status. Closed session/channel requires
re-admission and a fresh incomplete transfer from zero, with no custody renewal.
Reject late work from retired room/person/generation; leaving/signout cancels owned
messages. Room/draft switching does not move work into a different draft.

Raise the domain limit to ten and advertise its numeric value separately from
transfer support: older hosts (including slice-3 hosts) still allow eight. Consumers
derive validation and provider attachment/read byte budgets from the domain value.
No views, styles, markup, preview URLs or composer wiring in this slice. Native
streaming saves need a separate IPC/write lifecycle and remain outside this slice.
Acceptance uses controlled transports/clock for concurrency, defer, resume, finish
uncertainty, partial failure/retry, cancel, stale authority, skew and bounded bodies;
run affected Rust tests, bindings, TypeScript, api/lib Vitest and architecture gates.
Packaged UX and peak RSS remain unverified until UI integration.

Owner decisions (2026-10-13): cancellation ends at publication dispatch;
`sending` is non-cancellable and has no host cancellation contract. Inject the
acknowledged `socket.command("message.send", roomMessagePayload(request))` path,
not `say()` (which discards ACK). Lost ACK enters `outcome_unknown`, never an
automatic new publication. Delivery resolves it; explicit retry retains the same
attachments and uses the socket's exact-request retry callback when supplied.
Definite refusals expose a typed code: muted/temporary transport failures allow
retry, while read-only/revoked authority, invalid content and expired attachments
are final. The host owns pending-attachment TTL; `attachment_unavailable` is final.
This owner handles file messages only, never text-only sends. Re-admission to the
same participant with a new generation restarts incomplete files from zero;
late old work is ignored. Room departure/signout cancels pre-publication work and
detaches already-dispatched work, without claiming to retract publication.
The UI must share one model per host/person, admit current room upload clients,
feed validated room events, and call leave/close at membership/signout boundaries.
Provider consumers now admit ten context/observation IDs and ten distinct reads;
the existing two attempts at 10 MiB each raise the read byte budget 160 -> 200 MiB.

API/limit prerequisite verified: bindings regenerated; domain 67, protocol 6,
provider 303, persistence 526 and server 190 library tests passed (two existing
ignored tests). Real plain HTTP and production WebCrypto transport integration
passed, including 50 MiB transfer/resume/finish/bind and capability discovery.
TypeScript and `vitest run src/api src/lib` passed 79 files / 474 tests. Earlier
concurrent runs hit the existing five-second transport-test timeout; a two-worker
run and the final default run after Rust checks completed passed without timeout
or gate changes. Formatting, diff and architecture checks pass. At that prerequisite
checkpoint the outgoing model/scheduler remained pending the publication decision;
no packaged UI, peak RSS or desktop streaming-sink acceptance was claimed.

Headless completion: `OutgoingMessages` now owns admission, immutable snapshots,
subscriptions, send/cancel/retry/discard, room delivery and leave/signout cleanup.
Submitted bytes are the file's dispatched high-water offset; accepted bytes come
only from host receipts (legacy has only final acceptance). Retryable owner busy
returns to the queue after 250 ms when it has no host hint. Re-admission drops an
old socket's retry closure and explicit retry uses the newly injected publication
owner. Completed attachments remain pending until binding or the host's TTL.
Retry reuses the begin operation ID when its response was lost; cancellation or
re-admission starts a new operation. A cancelled final failure stays non-retryable.
TypeScript and API/lib Vitest, including ten model regressions, pass;
`cargo fmt --all`, diff and architecture checks pass.
No Rust/type bindings changed in this completion. Composer/timeline wiring,
packaged UX, peak RSS and native streaming save remain outside this task.

## Transfer rework: human upload owner (slice 3, 2026-10-13)

Required entry points: authenticated begin/data/finish/status/cancel under
`/api/message-attachment-transfers`, reached directly and through the existing
encrypted virtual HTTP router. The legacy `/api/message-attachments` whole-file
JSON route, connector/AI uploads and `message.send` publication stay unchanged.
Advertise `x-agentsassemble-upload-transfer: 1` alongside the preview/upload-credit
capabilities; binary encrypted records and client scheduling are later slices.

Begin resolves the exact current writable human and room before accepting bytes.
It rejects empty/oversized declarations, reserves declared bytes against room and
host storage limits, and returns an opaque handle, future attachment ID, 64 KiB
piece size and expiry. An operation ID identifies one transfer for that custody;
repeated begin returns it, conflicting input is refused. Data consumes raw bytes
incrementally, accepts only the next explicit offset and never exceeds declared
length. Accepted offsets survive interrupted requests on this host process.
Finish requires exact length, rechecks current authority, file limit and quota,
then atomically exchanges reservation for ordinary one-hour pending custody.
Non-images are never assembled. Safe rasters assemble only after acquiring the
existing two-decoder budget, preserving format/dimension/allocation limits and
upload-time preview generation. Finish is idempotent; status recovers its result.
Cancellation, expiry, shutdown and authority/room loss remove incomplete bytes
and release reservation. An app-owned 10-second sweep observes expiry and authority
loss; each begin also performs cleanup. Finished receipts expire after one hour
and are capped at 4,096 (capacity returns the same defer result). Incomplete identities cannot bind, read, count as retained
files, export or restore. Finished publication remains solely `message.send`.

Use connection-local, disk-backed SQLite TEMP metadata/piece tables on the existing
single connection. They disappear on connection/process close, never enter the
main-schema archive catalog or VACUUM export, and cannot be supplied by restore.
There is no durable schema change. Piece transactions release the writer before
network awaits; partial final pieces retain the exact accepted byte offset.
All ordinary attachment insertion quota checks include live reservations.

Admission allows three active transfers per human, 64 per host: one MiB fixed
application/HTTP buffer allowance each, including bounded driver copies and HTTP
receive buffers (64 MiB total). This conservative allowance is independent of file
size. The legacy 512 MiB/four-copy pool stays intact: combined upload-buffer
envelope 576 MiB, plus the unchanged encrypted 8 MiB queue pool, SQLite caches,
metadata and raster working memory. At most two new raster assemblies add 100 MiB
compressed input beside the existing two 72 MiB decoder allocation limits;
decoder/preview working-set RSS is not claimed to equal those allocation limits.
Begin contention returns structured `defer` with a retry hint, permitting ten
client-queued files without treating capacity as upload failure. Data retains
30 s idle / 64 KiB per 120 s / one-hour deadline semantics; server storage/credit
waits are excluded from idle/minimum-progress time, not from the hard ceiling.

Acceptance: exact 50 MiB non-image and raster transfer -> finish -> message bind,
legacy metadata/preview parity; short/long declarations and gap/overlap/replay
refusal; interrupted resume; repeated finish/status recovery; reservation races,
cancel/expiry release and policy lowering; incomplete bind/read/backup exclusion;
three admissions/fourth defer; bounded bodies on both transports and an unrelated
room write completing while a request is stalled. Peak RSS and packaged UI remain
separate measured acceptance; structural buffer tests must not claim those results.

Verified: `cargo test -p agentsassemble-persistence -p agentsassemble-server --lib
-- --test-threads=4` passed (524 persistence / 181 server, one ignored each);
the affected HTTP integration suite passed seven tests and the production
WebCrypto/RemoteTransport integration passed one. Both transports upload, finish,
bind and download varied 50 MiB bytes and a safe raster exactly. Persistence tests
compare generated large-raster previews with legacy validation, exercise offset
and length failures, concurrent quota reservations, cancellation/expiry, policy
lowering, invisible incomplete custody, file-backed backup exclusion and hostile
restore refusal. Transfers pin the room incarnation (`room_uid`); replacement
cannot inherit old bytes or quota. Controlled stalls allow an unrelated write;
clock tests exclude server waits while retaining idle/progress/hard deadlines.
An additional focused expiry/incarnation/restart regression passed. `cargo fmt
--all`, `git diff --check` and `make architecture-check` passed without gate changes.
The cleanup timer belongs to the server runtime, is joined at shutdown and does
not occupy the connection tracker. No peak-RSS, paced-WAN, packaged-UI or real
provider verification was performed. Transfer handles/receipts are process-local;
finished attachments themselves retain ordinary durable pending custody.

Decided (reviewer, 2026-10-13): closing an encrypted channel ends its room session
and with it the session's incomplete transfers. Interrupted child requests resume
while that authority is valid; after a new admission the client starts the file
again from zero. Carrying an incomplete transfer over to a newly admitted session
of the same person (a custody-renewal rule) is a separate follow-up, to be weighed
after the measured acceptance shows how often real links drop mid-file.

## Transfer rework: encrypted transport (slice 4, 2026-10-13)

Required entry points: `secure_channel`, `secure_channel_requests`, `secure_http`,
`secure_queue` and production `frontend/src/lib/remote/remoteTransport.ts`; the
Node WebCrypto peer in `tests/central_owner_boundary` verifies their real router
flow. Routes, authority, upload/download owners and frontend views stay unchanged.

Request and response body pieces use authenticated binary plaintext containing
direction/type, request ID, exact next byte offset, length and at most 64 KiB of
raw bytes. Control remains JSON; AEAD sequence/order authentication and the
handshake stay unchanged. Advertise in existing response headers, opt in through
request headers, then confirm in response headers for this channel only. Both
version skews retain the exact legacy JSON/base64 body and ACK/consumed shapes;
neither binary data nor sliding credits may precede mutual agreement.

Replace record ACK and upload consumed stop-and-wait with cumulative byte credit
in both directions. Each direction shares one 384 KiB data window across all
bodies, backed by reservations from the existing 8 MiB global / 512 KiB direction
budgets before advertisement; the remaining capacity serves control/accounting.
Count promised, queued, processing and unacknowledged ownership without gaps.
Return upload credit after owner acceptance and download credit after downstream
consumption. Concurrent bodies progress fairly, and deadlines reclaim a stalled
body's share without preventing other bodies or control traffic. Independent
cancellation promptly drops its charges, including records behind other traffic.
Withheld channel credit/ACK closes within the existing 30-second idle bound and
releases every charge. Preserve 64 KiB/120-second progress and one-hour ceiling;
server-imposed credit waits do not count as client stalling. Do not raise gates.

Acceptance: byte-exact uploads/downloads including 50 MiB, replayed legacy shapes
for new-client/old-host and old-client/new-host, unchanged queue maxima and window
bounds, three concurrent uploads and ten downloads, isolated stall/deadline and
cancel, withheld credit cleanup, malformed binary/offset and overflowing,
underflowing or replayed credit refusal. Measure loopback one/ten 50 MiB transfers
in each direction on both paths; report timings, including the heavy integration
test, without pinned throughput expectations. Partial implementation or missing
measurements do not establish completion of this contract.

Binary checkpoint: `x-agentsassemble-body-binary: 1` advertises the raw codec;
request value `1` opts in using legacy body shapes and response
`x-agentsassemble-body-binary-confirmed: 1` confirms. Only subsequent requests
with value `2` use binary bodies. Each in-flight request retains its selected
codec, including legacy children on an upgraded channel. This checkpoint keeps
the existing ACK/consumed pacing and does not advertise sliding-window support.
The production WebCrypto integration preserves 50 MiB upload/finish/bind/read,
ten large downloads and frozen legacy request/data replay against the new host
(17.11 seconds). Server unit tests, affected frontend tests/typecheck, formatting
and architecture gates pass. Sliding-credit, multiplexed deadline/cancel cleanup
and old/new throughput comparisons remain required work.

Reviewer decision (2026-10-13): request credit returns when the consumer pulls
bytes from the virtual body. The streamed upload owner pulls its next piece after
storing the previous one; ordinary requests and the legacy JSON upload use their
existing admitted, charged bounded collector. Keep those owners unchanged.
Downloads return credit when the client reads the response body. Finish the shared
384 KiB window, fair multiplexing, deadline and independent cancellation semantics
above, including adversarial credit checks. Measure both paths on plain loopback
and paced links at 50 ms and 100 ms RTT without timing assertions.

Window protocol: `x-agentsassemble-body-window: 1` advertises support; request
value `1` opts in with legacy records and response
`x-agentsassemble-body-window-confirmed: 1` confirms the reserved ingress window.
Only subsequent requests with value `2` and confirmed binary mode use cumulative
`credit {id, offset}` and omit ACKs for body and credit records. Response headers
still precede their own body. ACK sequence numbers still count every AEAD record.
FIFO capacity waiters share 384 KiB; a body may retain at most 320 KiB, leaving
one piece for other consumers. This trades one single-body in-flight piece for
progress by other bodies while a consumer stalls. Cancellation reaches the writer
even after the router task has finished. Timeout credit receipts expire with the
error record ACK; late valid credits cannot release capacity twice. A negotiated
cancel uses `cancelled {id}` / `cancel_confirm {id}` to return transmitted capacity
only after the receiver has discarded earlier records; queued unsent bytes drop
immediately. Request cancellation drains the virtual ingress queue even while its
consumer awaits storage. The channel alone owns its advertised reservation, so a
pending durable mutation cannot retain it after channel closure.

Round-9 correction contract (2026-10-13): in `secure_writer.rs`, an error with no
outstanding body records retires nothing and remains ordinary ACK-controlled
traffic. Immediate lazy-body authorization/storage failure and failure after all
prior records were credited must reach the client, then allow more channel work
or close cleanly with every charge released. Peer/timing-controlled writer input
must not panic; unwind/abort must close the channel and release its admission and
queue/window reservations. `secure_channel.rs`/`SecureClient` own admission closure.
Code correction to the report: `Output::send` marks `op:error` ACK-controlled,
and the writer's ACK branch precedes its cancellation-notice branch; the quoted
`expect("cancel receipt")` is not reached by that error in the reviewed code.

Both `remoteCreditWindow.ts` and the host writer measure the existing 30-second
withheld-credit bound from emission of the oldest uncredited record of each body,
not reservation/queueing or another body's credit. Partial cumulative credit
advances to that body's next outstanding record; release removes its deadline.
One timer/select deadline uses the earliest body deadline. Host expiry may retire
an isolated stalled download with an ACK-controlled error while healthy work
continues; withheld error ACK/cancel confirmation still closes within its bound.
Acceptance: fake-clock cross-body credits, partial credit and local send backlog;
body-stream errors before data/after full credit; abnormal writer cleanup. No gate,
window size, wire protocol, provider or product UI change is needed.

Round-9 verification: server library 194 passed / one existing ignored fixture;
after removing the replaced idle helper, the three body-policy tests also pass.
The production WebCrypto/router HTTP/SSE/room-socket integration passes (21.89 s).
TypeScript and API/lib Vitest pass: 81 files / 488 tests with two workers. Earlier
parallel-build runs timed out in the existing five-second transport tests; no
timeout or gate was changed. Formatting, diff and architecture checks pass.
Regressions exercise both stream-error boundaries, reader failure with continued
channel use, queue-charge release on panic/abort, per-body/partial-credit expiry,
and local backlog before emission. Credit rate/ceiling clocks start at first
emission and exclude server gaps without outstanding records; source/upload
absolute ceilings and all numeric gates stay unchanged. No packaged client,
WAN/peak-RSS measurement, provider run or external re-review is claimed.

Measured with `AA_TRANSPORT_BENCHMARK=1 cargo test -p agentsassemble-server --test
integration central_owner_boundary::secure::trusted_webcrypto_owner_uses_encrypted_http_sse_and_room_socket
-- --nocapture`: loopback TCP proxy, production Node WebCrypto/RemoteTransport,
real router/upload owner and in-memory SQLite fixture. Upload timing includes
begin/data/finish, with up to three concurrent admissions; publication and
byte-exact readback run afterwards. Downloads consume and compare every piece.
Legacy replay freezes both wire shapes and the channel-wide consumed chain.
These are single-run observations, not performance assertions or WAN/disk evidence.

| 50 MiB bodies | Legacy JSON/base64 + ACK | Binary + ACK |
| --- | --- | --- |
| Upload, one | 0.70 s / 71.8 MiB/s | 0.59 s / 85.4 MiB/s |
| Upload, ten | 7.30 s / 68.5 MiB/s | 5.93 s / 84.3 MiB/s |
| Download, one | 2.65 s / 18.8 MiB/s | 0.16 s / 319.8 MiB/s |
| Download, ten | 26.69 s / 18.7 MiB/s | 1.58 s / 316.6 MiB/s |

The benchmark-enabled integration passed in 93.12 seconds, including byte-exact
finish/bind/read/delete for measured upload fixtures. Opt-in measurement avoids
adding that workload to the ordinary integration. Shared-window bounds, credit
overflow/replay, withheld-credit cleanup and isolated stalled-body reclamation
remain unimplemented/unverified; binary throughput is not sliding-window acceptance.
Final checkpoint checks: server unit suite 183 passed / one ignored, ordinary
encrypted integration 17.01 seconds, frontend typecheck and 14 affected Vitest
tests passed, plus `cargo fmt --all`, `git diff --check` and unchanged architecture gates.

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
  session-to-purpose-ticket exchange is removed at `e0622cb`. Raw issuer secrets,
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
  blocks, byte-valid UTF-8 is the first text block followed by the separate metadata
  block, and remaining binary is an embedded blob resource. The UTF-8 branch was added
  only after the packaged Codex flow read the exact 59-byte metadata but reported that
  both the original blob and a later embedded text resource body were unavailable. The
  first text block is the smallest provider-consumable representation observed at the
  real boundary; it avoids base64's 4/3 expansion and JSON escaping for text without a
  MIME allowlist or a change to stored bytes, binary behavior, authority, item limit, or
  retry budget. The same accepted ID could otherwise repeatedly cause a 10-MiB SQLite
  read and 13.3-MiB base64 allocation. The active turn therefore owns one pending read
  per ID, at most two attempts (and successes) per listed attachment, and checked
  successful bytes bounded by twice the canonical eight-item/10-MiB input ceiling.
  The second attempt preserves one bounded retry when an MCP response is lost.
  The retired managed Antigravity terminal/file-staging path is outside this contract.
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
   room context. The final authorized provider matrix in `docs/PRODUCT_REIMPLEMENTATION_PLAN.md`
   exercises the real attachment path at each retained provider-visible boundary; no transcript,
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
  changed lines. Batch timing and scan authorization are owned only by the active
  `Standing project workflow` in `AGENTS.md`. When that workflow invokes review, it
  covers security, structure, duplicated policy, overimplementation, SSoT, lifecycle
  cleanup, and removable state.

### Historical packaged and provider evidence

These dated observations precede the managed Antigravity retirement. They preserve
revision-bounded evidence, do not restore its helper/hook contract, and do not
replace the current plan's provider scope or final verification authorization.

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

Historical retired managed-provider evidence: Antigravity CLI 1.1.22 exposed two
native-PTY permission boundaries. Its then-current `PreToolUse` contract uses `decision` rather than the obsolete `overwrite`
field, and a user-level ask hook takes precedence over a project allow. The first real
run also showed that the long private helper path wraps inside the terminal permission
card, so independently parsing the rendered command in both the hook and PTY rejected a
command that the hook had already validated. The managed hook at that revision became the single policy
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

## Encrypted upload consumption and timing (2026-10-12)

Required behavior: encrypted virtual HTTP must receive the current 10 MiB file
allowance (including its encoded envelope) without retaining consumed chunks'
queue charges. Keep 64 KiB chunks, 512 KiB directional and 8 MiB global queues.
For negotiated requests, host consumption returns request-bound credit; browser
uploads wait for it so concurrent senders cannot burst beyond transport admission.
Cancellation and expiry release only the affected request, without aborting durable handler work.

Authorized file collectors at message attachments, profile/appearance attachments,
agent avatars, persona import (including central-owner routes), and connector
upload use 30 seconds without nonempty data, at least 64 KiB per 120-second
window until completion, and a 60-minute total ceiling. Empty data/trailers do
not reset progress. Control/JSON and preauthorization keep the ten-second bound;
virtual HTTP uses the same policy. Size and timeout errors remain distinct.
Connector upload clients must allow this server-owned upload lifetime.

Owners/entry points: `secure_http.rs`, `secure_queue.rs`, `http_api.rs`, `web.rs`,
the authorized upload handlers above, `connector/client.rs`, and browser
`frontend/src/lib/remote/remoteTransport.ts`. No file-size, quota, storage, UI,
provider, MCP-envelope or authority changes belong to this slice. Upload failures
retain the existing uncertain-write/no-automatic-retry contract.

Verification: first reproduce queue exhaustion above 512 KiB; then prove a
10 MiB file envelope completes with released charges and unchanged gates,
stalled/trickling bodies time out, progress past ten seconds succeeds, and
cancelling one request preserves other queues. Use controlled polls/virtual time
and browser protocol peers; report separately any unverified packaged flow.

### Upload-credit capability across deployed versions

This encrypted channel has no general version negotiation. Any future frame
change must be capability-gated through an advertisement old peers already ignore;
never send a new field or operation to a peer that has not advertised support.
The host advertises `x-agentsassemble-upload-credit: 1` only inside virtual HTTP
response headers, preserving the existing response frame and handshake shapes.
The browser remembers support only for that channel and snapshots it at request
creation: only after advertisement does it send `upload_credit: true` on a
request and wait for request-bound `consumed` frames. An in-flight unpaced request
must stay unpaced even if another response advertises support.

Without that request opt-in the host emits no `consumed` frames, while still
releasing consumed queue charges. New browsers with old hosts and old browsers
with new hosts preserve pre-ebec3b14 small-body behavior without credit waits or
unknown frames. The old side's failure above its 512 KiB allowance remains allowed.
New/new channels retain paced 10 MiB uploads after advertisement. No probing,
retry, persistent capability cache, queue-limit or user-interface changes.

Affected owners are `secure_channel.rs` (strict request parser),
`secure_channel_requests.rs` (dispatch), `secure_http.rs` (advertisement and body
consumption), and `frontend/src/lib/remote/remoteTransport.ts` (channel capability
and request pacing). Verify both skews, request-local opt-in, unchanged frame
shapes, queue-charge release, and the existing encrypted 10 MiB regression.
Pre-ebec3b14 evidence: `git show ebec3b14~1:frontend/src/lib/remote/remoteTransport.ts`
accepts response headers via `new Headers` without a header-name allowlist;
`git show ebec3b14~1:crates/agentsassemble-server/src/secure_channel.rs` uses
`deny_unknown_fields`, so an unadvertised request must omit the new field entirely.

## Host retained upload-body admission (2026-10-12)

All authorized file-body entry points (message/connector attachments, profile,
appearance, agent avatar and persona import), over plain HTTP and encrypted
HTTP-over-WebSocket, share one host weighted budget and two concurrent uploads per
principal. Reserve before reading, using the route maximum for unknown length or
the declared length within that maximum; enforce that declared bound even on the
virtual transport. Hold through decoding and storage, including uncancellable
workers. Release on completion, failure, timeout and cancellation once retained
work ends. Exhaustion immediately returns retryable `upload_body_budget_exhausted`
(429), without awaiting capacity. Credentials and client credit opt-in are irrelevant
to this host bound. Existing queue gates and progress deadlines remain.

The host budget is 512 MiB, charging four times the encoded bound for body, JSON,
decoded bytes and storage copies; at 10 MiB this admits nine maximum-size requests
across principals. At a future 50 MiB file setting it admits one maximum request
without growing host memory. Small declared bodies share the same weighted budget.

MCP correction: hosted and standalone HTTP `/mcp` use a separate, non-waiting
two-envelope admission before RMCP parses JSON, bounded by the encoded-body
limit for the existing AI 10 MiB file ceiling. This consumes none of the shared
upload pool.
Parsed non-upload calls release envelope admission before dispatch; in particular
`room_wait_next` retains no body lease while waiting or queued on its cursor.
Hosted attachment uploads resolve the private connection to current server-owned
connector authority, right-size shared admission to the parsed payload, and
transfer custody from the envelope to that principal's two-upload limit. Keep the
applicable room-derived bound when known, without increasing the AI 10 MiB cap.
The authenticated lease follows decoding and storage through completion/error;
HTTP cancellation cancels detached tool work, while already-started storage keeps
its lease only until that retained work ends. Standalone HTTP and stdio forward
uploads to the existing authenticated HTTP owner; neither holds a host upload
lease for non-upload calls. Stdio has no HTTP envelope reservation to transfer.
No schema, provider, UI, retry or upload-uncertainty semantics change.
Acceptance: concurrent small chunked waits from one connector cannot consume
shared upload capacity or block another principal; authenticated MCP uploads obey
the principal cap; cancellation releases admission after retained work ends; the
separate pre-parse limit cannot be exceeded. Verify server crate tests, formatting
and architecture gates. Entry owners: connector/mcp_web, mcp_transport, mcp,
hub/client, attachment_web and upload_body_budget.
Regression acceptance: concurrent slow bodies across one/many principals stay within
admission, refusal changes no accounting, and success/error/timeout/cancellation
return capacity on both transports, including uncooperative credit-free clients.

### Storage ceiling only — schema 92 (2026-10-12)

Normal persistence opening upgrades schema 91 to 92 by rebuilding message attachment
and connector upload tables in one transaction. Connector publication copies pending
files into message rows, so both stored-size checks become 52,428,800 bytes. Copy
all rows with INSERT SELECT, verify counts and foreign keys before dropping originals,
rename replacements, recreate existing indexes, and commit the version together.
Failure preserves the original tables/version and reopening retries; no VACUUM.
Every other limit, key, index and custody constraint remains unchanged; fresh and
upgraded schemas agree. The domain owns a separate room-file ceiling; live upload
validators, avatars/personas, provider transports and settings remain unchanged.

Backup export, hostile reconstruction and RestoreJob preparation accept stored
message metadata through the ceiling. Extend the immutable archive catalog to 92;
accepted versions 87–91 retain their validation rules and restore through normal
upgrades to 92. Only schema-92 message content may exceed the 16 MiB cell bound.
For that cell, charge at most the historical 10 MiB against the existing 32 MiB row
budget; all other bytes retain their full charge and individual bounds. Thus large
content does not admit hostile wide metadata. Pending connector uploads remain
excluded from archives. Verify exact row/BLOB/custody preservation, transaction
rollback and retry, 50 MiB export/restore, historical restore, hostile wide-row
rejection and unchanged non-message storage limits. Transfer performance and larger
live uploads belong to later slices.

### Per-room policy and human admission (2026-10-12)

Two commits own (1) room policy, atomic accounting and settings controls, then
(2) human upload enforcement and preflight. `rooms.settings_json` gains strict
`attachment_file_limit_bytes` (10/25/50 MiB) and nullable
`attachment_storage_limit_bytes` (none/500 MiB/1/2/4/8 GiB). Missing fields mean
10 MiB and no quota; default fields are omitted from canonical serialization,
preserving existing revisions. Explicit quota null removes it. Existing
`room.settings.update` owns manager authorization, expected revision, receipt and
publication. Snapshots and strict backup reconstruction preserve both fields;
no schema change is needed. The manager-only settings-storage section follows
channels with the approved Korean labels and existing controls/classes.

Count retained pending/bound message files and pending connector files in the
room quota. The existing single-connection writer serializes cleanup, current
policy observation, quota check, server cap and insertion. Binding never charges
twice. Human writes return attachment_too_large (413, applicable limit in safe
error detail), room_attachment_quota_reached (429), or existing
attachment_quota_reached (429). Lowering affects new storage only; retained
files remain readable/downloadable through the 50 MiB ceiling.

POST /api/message-attachments resolves writable authority and its exact room
before collecting plain or encrypted bodies. The current room limit determines
4 * ceil(limit / 3) + 64 KiB body admission and the existing four-copy host
reservation; declared lengths and unknown-length collection share that bound.
Validate base64 length, decoded bytes/content, then revalidate authority/current
policy transactionally before quota, server cap and insertion. Keep existing
credits, queue gates and progress deadlines. Frontend preflight uses current room
settings and reports 파일은 최대 {N}MB까지 올릴 수 있어요.; metadata/downloads use
the retained ceiling. Entry owners: domain room_settings, persistence
message_attachments/asset_storage, profile_web/http_api, frontend api/room and
api/messageAttachments, settings controller and RoomSettingsModal.

Acceptance: unchanged defaults/revisions; manager-only updates and stale conflicts;
concurrent quota contention; pending-to-bound neutrality; lowering retains reads;
25 MiB refused before full collection in a 10 MiB room and byte-exact upload,
bind/download in a 50 MiB room over plain/encrypted routes; legitimate 50 MiB
admission within unchanged host budget. Run affected crate tests, fmt,
architecture-check, generated types, frontend tsc and affected vitest suites.
AI upload/read paths, avatars, appearance and personas keep today's 10 MiB;
connector policy enforcement and new staged/managed transfers are later slices.
No usage-report work, schema migration or provider verification belongs here.

Human-download entry points also include `readResourceBlob`'s caller-provided
bound and `desktop/src-tauri/src/message_attachment_save.rs`. Message downloads
pass their validated retained size to the existing encrypted resource budget and
accept raw desktop-save bytes through the room-file ceiling. Keep other resource
callers' 10 MiB default and the existing 64 MiB live-resource budget. The preflight
reads current settings through the existing settings HTTP authority before file
encoding; it does not introduce a client policy cache or edit the composer flow.

## Round-3 security corrections (2026-10-12)

Required behavior and entry points: human attachment responses at `profile_web`
(including encrypted virtual HTTP) must reserve validated stored bytes before BLOB
selection, with host-wide weighted admission and per-principal concurrency, and
retain admission with response bytes through completion/drop. Exhaustion is an
immediate, specific retryable refusal; refused reads must not allocate the BLOB.
Managed-provider, attendee and connector reads must reject metadata above their
10 MiB reader ceiling before BLOB selection, preserving `attachment_too_large`
with actual size and limit through each public error mapping. Connector insertions
keep their 10 MiB file ceiling but share transactional room quota accounting.

Archive reconstruction must reject schema-92 limits in all pre-92 settings and
nested settings receipts/events before current decoding; the same provenance rule
applies to post-87 conversation modes, delegation and delivery classification.
Schema-92 migration must reject replacement objects, validate the exact historical
source schema, create fresh replacements, and copy named columns byte-exactly.

MCP pre-parse admission must use trustworthy existing connector transport identity
if available. Otherwise use a global byte-weighted envelope budget that allows
many independent small requests, a bounded envelope, and a seconds-long no-progress
deadline. Never invent identity from untrusted headers or source addresses. Preserve
10 MiB connector uploads and release envelopes before long-running tool dispatch.

Verification: concurrent 50 MiB response leases and zero BLOB selection on refusal;
metadata-only oversized AI refusal through all mappings; transactional connector
quota refusal; historical restore plus version-smuggling rejection before activation;
stalled MCP requests alongside unrelated calls; hostile migration schema refusal
and byte-exact genuine upgrade. Existing delegation tools and UI are out of scope.

Download admission uses 512 MiB host weight, charging twice stored size for the
SQLite result and retained response, with sixteen concurrent responses per authenticated
principal: one client's scheduler runs four, while room-appearance reads and the same
person's other devices run beside it and a refusal is not retried by the client. Host
weight, not the count, bounds memory. Five concurrent 50 MiB responses fit,
leaving 12 MiB of weight for small images. Message and room-appearance readers
acquire after metadata validation, before selecting content. Public profile/prejoin
and agent avatars also share this byte pool and retain their existing shared six-read
anonymous gate; these public image routes have no authenticated reader identity.
`attachment_download_budget_exhausted` is an immediate HTTP 429; admission follows
response Bytes clones across both plain and encrypted transports.

MCP metadata audit: generated Codex, Claude Code, Antigravity and Grok registration
commands supply only the common `/mcp` URL. Hosted and standalone services are
stateless (`legacy_session_mode(false)`, a request-local session manager); neither
MCP session headers nor bearer headers are bound to a room connector. Host/Origin
and proxy/source checks prove ingress only. The private connection handle arrives
inside JSON arguments. Therefore use the authorized identity-free alternative:
256 MiB global weight, four times collected bytes rounded to 64 KiB, and a two-second
nonempty-data idle deadline plus ten-second total pre-parse ceiling. Of that weight,
32 MiB reserves the first 64 KiB for 128 requests (comparable to 127 public connections);
larger envelopes use only the remaining 224 MiB. Thus even exhausted large-envelope
capacity cannot deny unrelated small tool calls. 127 small stalled bodies use 31.75 MiB. The per-envelope
ceiling remains the encoded 10 MiB upload plus its bounded JSON overhead so existing
uploads keep working. Exhaustion, timeout and over-limit responses are distinct.

## Round-4 download fairness correction (2026-10-12)

Every principal-keyed download reservation retains the sixteen-response limit and
also limits retained weight to 400 MiB of the shared 512 MiB host pool. Four
50 MiB human reads (100 MiB weight each) fit; a fifth is refused while another
principal can still start a 50 MiB read. Release both count and weight only with
the final transport-owned response clone. Applies to human, provider, attendee
and connector admission; anonymous image admission remains host-bounded.

## Round-4 AI retained downloads (2026-10-12)

Managed provider attachment ingress, attendee tool HTTP and connector attachment
HTTP must use the same host download owner as human reads. Reserve against the
validated size and authority-derived principal before selecting any chunk. Keep
the 10 MiB reader ceiling. Admission refusal selects nothing and preserves
`attachment_download_budget_exhausted` with its retry message through all mappings.
Lease custody lasts through encoded HTTP body completion/drop or the managed
provider pipe frame's successful flush/drop, including queue backpressure and
cancellation. Returning a JSON value or queuing a frame does not finish delivery.

AI reads charge eight times stored size plus 64 KiB envelope/piece allowance:
raw bytes (1x), base64 text (4/3x), serialization capacity (up to 8/3x), and
the provider writer's copied frame capacity (up to 8/3x), rounded up to 8x.
HTTP uses this same conservative bound; its body retains the final encoded bytes.
Storage remains the schema-93 bounded piece owner. Per-principal count/weight
caps apply to these reservations too. Verify refusal before chunk selection and
slow/drop lifetime on each route and a backpressured provider pipe. No wire change.

## Round-5 hosted MCP download custody (2026-10-12)

`room_read_attachment` on hosted `/mcp` must read through the owned runtime,
using the same connector-session authorization, read-rate admission, published
attachment reachability and encoded download budget as connector HTTP. Transfer
the lease to request admission before building MCP content; the final RMCP body
owns it until EOF/drop, including slow consumption and cancellation. Remote
standalone MCP retains its authenticated HTTP boundary and timeout semantics.
Acceptance: a published attachment read through the real hosted RMCP service
holds shared download weight while its outer body is partly drained, releases on
drop/EOF, and preserves exact MCP content. No frontend, wire or provider change.

## Round-5 encoded AI class admission (2026-10-12)

All encoded provider, attendee, connector HTTP and hosted MCP attachment reads
share a class cap of host weight minus two maximum human reservations. Under
streamed human admission that is 512 MiB - 2 × 1 MiB = 510 MiB, independent of
the 50 MiB file ceiling. Six maximum 10 MiB AI reads (480.375 MiB) still fit;
smaller encoded reads cannot consume the final 2 MiB. Existing host and principal
caps still apply, and the encoded class weight follows the same delivery lease.
Acceptance: filling the encoded class with maximum reads plus a residual read
still admits two maximum-file human streams; release restores class capacity.
No asset, stream cost, wire, retry or frontend change.

## Round-5 room-file migration trigger boundary (2026-10-12)

Normal open and staged archive upgrade at schema 91→92 and 92→93 must reject
noncanonical triggers on every table the migrations mutate, including
`runtime_metadata`, before any copy or version write. Canonical 91/92 have no
triggers on either room-file parent or runtime metadata. Revalidate row and byte
counts after the version write, and schema-93 chunk completeness, before commit.
Genuine databases retain byte-exact content/custody and upgrade normally; hostile
source objects fail atomically without executing their triggers. This closes the
shared cause at both migration owners; no new schema, fallback or gate change.
Acceptance: hostile version-update corruption triggers at both source versions
are refused and original rows/version remain intact; genuine copies are exact.

## Image previews (2026-10-12)

Conversation images (single and grid) request a server-derived preview only after
their transport learns `x-agentsassemble-image-preview: 1`. Older hosts receive
the original view request with no probe/retry. The route variant is
`/api/attachments/{id}?preview=1`; stored metadata and event JSON stay unchanged.
Opening large fetches the original once, retaining the preview while it loads;
downloads and all AI reads retain original bytes and their existing entry points.
Browser image downloads reuse the already-owned original URL: a retained 50 MiB
original must not be charged twice against the unchanged 64 MiB resource budget.

At existing upload decode, generate static safe-raster previews with long side
at most 1100 pixels and encoded size at most 256 KiB. Originals <=256 KiB and
animated PNG/GIF/WebP use the original streamed reader. Apply EXIF orientation
before resizing; preserve alpha using lossless WebP, use JPEG for opaque pixels,
and reduce dimensions further when necessary to meet the byte ceiling. Validation
failures remain errors. Preserve existing two-decoder, dimension/pixel and 72 MiB
decoder allocation limits. GIF animation detection inspects frame headers with the
already-used GIF decoder library, preserving first-frame-only validation and avoiding
allocation/validation of later animation frames. Historical images generate lazily at first authorized
request, under serialized persistence ownership, and store the result (including
the no-separate-preview decision) once for concurrent first requests.

Schema 94 adds attachment-owned derived cache rows (including pending connector
uploads, transferred on publication). Cascade deletion, archive Ephemeral policy,
and a maximum of one <=256 KiB preview per retained asset bound extra host storage
to 4096 * 256 KiB = 1 GiB without consuming room quota or changing the user-asset
8 GiB ceiling. Archives 87–93 still reconstruct and upgrade; restored caches are
empty and regenerate lazily. Reject noncanonical source catalog objects and
preexisting destination objects before migration/version mutation, using the
round-5 schema migration pattern.

Preview readers share original room/session/visibility/deletion authorization and
recheck it on each piece. Admission precedes byte selection: cached previews and
original passthrough use streamed cost; lazy generation additionally reserves its
whole-input/decode/encoding cost (2× stored size + 160 MiB for decoder/rotation
and resize/encoding) until the blocking decoder releases the input. An in-process per-attachment guard serializes first generation and insertion.
Authorization/original selection and final store each use short transactions;
decode permit waits and decode/resize/encode run outside any database transaction.
The store rechecks current authority, immutable original metadata and existence,
and accepts an already-stored preview without overwriting it. Subsequent reads
use only the cache pieces. Responses
retain private/no-store rules across plain desktop and encrypted channels.
Entry owners: raster_assets, message_attachments/connector, message_attachment_reader,
schema_version/archive_schema/backup_policy, profile_web/attachment_download/
download_budget, transports, messageAttachments/read scheduler, LobbyAttachments.
The shared messageAttachmentDownload transport owns browser/native save delivery.

Acceptance: bounded large previews with alpha/orientation; small/animated exact
originals; concurrent lazy generation once; identical access refusal and per-piece
revocation; cascade cleanup; archive exclusion/historical restore/lazy regeneration;
old-host originals only; large-open original once while showing preview. Verify
affected crate tests, fmt, architecture-check, frontend tsc/vitest and report any
remaining real packaged-client verification separately.

## Slice-4 credit-window completion (2026-10-13)

The earlier ACK-only checkpoint and its pending work above are historical.
Both negotiated directions now use consumer-returned cumulative credit, shared
384 KiB capacity, FIFO admission, bounded body shares, independent cancellation
and the existing progress deadlines. Control ACKs retain their one-slot bound.
Controlled-clock tests cover stalled bodies, withheld credit, cancellation behind
control traffic and strict credit boundaries; the production WebCrypto harness
replays legacy shapes in both version-skew directions and rejects malformed peers.
Server unit suite: 189 passed, one pre-existing ignored. Frontend typecheck and
20 affected tests pass; formatting and unchanged architecture gates pass.

Measured aggregate MiB/s (one / ten 50 MiB bodies), with the benchmark command
above and `AA_TRANSPORT_RTT_MS=50` or `100` for paced bidirectional delivery:

| RTT | Upload legacy | Upload window | Download legacy | Download window |
| --- | --- | --- | --- | --- |
| Loopback | 73.96 / 68.53 | 131.18 / 125.90 | 18.28 / 18.67 | 315.91 / 320.16 |
| 50 ms | 1.147 / 1.076 | 5.473 / 6.521 | 0.958 / 0.972 | 5.680 / 6.765 |
| 100 ms | 0.573 / 0.576 | 2.894 / 3.459 | 0.543 / 0.540 | 2.972 / 3.546 |

The loopback benchmark passed in 93.51 seconds. Both paced runs completed all
byte-exact transfer stages, then failed unrelated room-control fixture cleanup
because its socket expired during the long run. The harness now keeps both room
sockets alive, exercising heartbeat delivery every second in the ordinary run;
the ordinary encrypted integration passed in 19.44 seconds. The full paced
benchmarks have not been repeated after that harness correction.
These measurements simulate RTT over loopback; no WAN, disk throughput or packaged
frontend verification is claimed.

## Round-6 encrypted queue cleanup (2026-10-13)

The secure-channel writer must bound send and ACK waits for every outbound record,
including ordinary responses without download progress. Use the existing 30-second
body idle bound per record; valid consumption/ACK advances to a fresh record deadline
so healthy slow consumers are not limited by total channel duration. Expiry closes
the channel and drops its entire buffered queue and direction/global charges, even
when cancelled download records sit behind an unacknowledged ordinary response.
Owners: `secure_channel.rs`, `secure_queue.rs`, `http_body_policy.rs`. Keep existing
stop-and-wait and download progress/window/ceiling semantics. Acceptance: controlled
clock regression proves cancelled records behind that head release both budgets;
successive timely ACKs keep a slow consuming channel alive beyond one idle interval.

## Round-6 live migration boundary (2026-10-13)

Normal open/staged upgrades at 91→92, 92→93 and 93→94 must compare every main-schema
trigger (name, table, SQL) against the frozen source-version trigger set before
mutation. Validate exact `runtime_metadata` DDL and incoming foreign-key topology
for metadata and all room-file parents/chunks/previews against canonical references.
Extra, missing or altered triggers anywhere, helper cascade references and smuggled
triggers targeting future replacement names refuse atomically. Do not require full
catalog textual equality: genuine older live databases retain ALTER TABLE spellings.
Write the version first in the transaction, copy, validate full rows and every BLOB
byte inside SQLite, then perform only drop/rename/index/cache DDL and commit. The
preview migration copies no existing bytes; its unchanged parents/chunks must retain
complete content. Owners: room_file_schema, room_file_chunk_schema,
message_attachment_previews and shared room_file_migration validation.
Acceptance: helper index/FK/trigger chains and future-table triggers at every source
version refuse without changing version or bytes; a fixture created from the real
oldest supported live schema (70) executes the actual upgrade stages through HEAD
and preserves bytes/custody, as do frozen archive source versions.

Genuine live schema 70 retains `CREATE TABLE runtime_metadata (key TEXT PRIMARY KEY,
value TEXT NOT NULL)` and its sole `sqlite_autoindex_runtime_metadata_1`; neither
metadata nor the two file parents has incoming FKs at 91/92. Source 93 has exactly
the two chunk→corresponding-parent attachment_id FKs (NO ACTION on update, CASCADE
on delete). The only main triggers at 91–93 are frozen `room_message_search_insert`
and `room_message_search_delete` on `room_message_search_records`. Older live
`operator_pairings` keeps unquoted CREATE/ALTER-added columns, unlike quoted archive
DDL; that unrelated textual difference is deliberately accepted. Full-row comparisons
use indexed attachment identity joins, avoiding temporary trees of whole BLOB sets.

## Lazy preview connection lifetime correction (2026-10-13)

`open_message_attachment_preview_reader` must release the single pooled connection
before waiting for decode capacity and throughout lazy decode/resize/encode. Keep
existing generation admission and decoder permits, with authorization and original
byte selection in short transactions. A per-attachment guard shared by store clones
keeps concurrent first requests to one generation, without serializing different
attachments or unrelated database writes; it stays with a cancelled blocking worker
until decoding ends. A final short transaction rechecks live authority, immutable
metadata and existence, tolerates a concurrently stored result and stores nothing
if the attachment was deleted. Errors remain visible; no cache/storage fallback.
Owners: message_attachment_preview_reader, raster_assets, message_attachment_previews,
SqliteStore/from_owned_pool. Acceptance: a controlled slow generation permits an
unrelated write promptly, concurrent first requests generate once, a concurrent
cache insert is retained, and deletion during generation leaves no derived row.

## Round-8 immutable raster validation failures (Medium 2, 2026-10-13)

An immutable raster validation failure at transfer finish is terminal. Release
staged bytes, the storage reservation and active transfer admission; retain only
a small receipt within the existing bounded, expiring transfer registry. Status
and repeated finish return the same validation error without assembly or decoding.
Concurrent finish must coalesce or answer busy, never queue repeated CPU work.
Capacity/defer, decode admission and storage-busy failures remain retryable; if
cleanup itself fails, retry cleanup without repeating immutable validation.
The lazy preview entry point must also remember immutable decode failures under
its existing per-attachment generation guard, including cancelled workers, and
recheck original authorization before returning that error. Negative receipts
must be bounded without evicting live failures into repeated decoding; deleted
originals may release receipts. No migration or original/upload-route changes.
Owners: message_upload service, raster validation, message_attachment_preview_reader,
SqliteStore. Acceptance: concurrent/repeated invalid finish validates once,
releases reservation/admission and reports the same status error; transient refusal
then succeeds; repeated/concurrent undecodable previews do not decode again.

Verification: server library 190 passed / one existing ignored; persistence library
526 passed / one existing ignored. Four affected preview tests pass after adding
capacity-refusal recovery. Upload regressions prove one admitted invalid finish,
busy concurrent finish, deleted staging/reservation, reusable active admissions,
identical terminal status/finish errors and successful policy-refusal retry.
The production Node WebCrypto encrypted-router integration passes in 18.82 seconds,
including concurrent/repeated invalid finish and status. Lazy generation observes
one worker across cancellation, concurrent/repeated reads and preserves authorization.
Negative preview receipts reserve capacity before decode, retain live failures and
reclaim deleted originals under the existing 4,096 retained-asset bound. Formatting,
diff checks and unchanged architecture gates pass; migrations and frontend UI are
untouched. No packaged UI, real WAN or storage-busy fault injection is claimed.

## Round-10 transfer response contract (High 1, 2026-10-13)

The upload API response validator and host/person outgoing scheduler must pin the
first successful begin's handle, attachment ID, declared size equal to the selected
File size, and stable piece size between 16 and 64 KiB. Later data/status/finish
responses retain those fields. Data and interrupted-data recovery may advance
partially, only within the submitted byte range; no response may move backwards
or skip unstaged bytes. Completion supplies the pinned attachment and exact size.
Retry hints are capped at five seconds. Eight consecutive responses/defer attempts
without byte or completion progress, or ten minutes including deferral/network
waits, end with typed retryable upload_stalled; only the person starts another
attempt. Invalid identity/range responses fail as invalid_upload_response, never
publish substituted attachments. Existing cancellation and explicit retry remain.
Entry points: createMessageAttachmentUploadClient, OutgoingMessages.upload/pump/retry.
Acceptance: controlled hostile sequences fail boundedly without publication;
interrupted multi-piece partial acceptance still resumes and publishes once.

Round-11 legacy receipt correction: uploadMessageAttachment must bind the returned
size to the submitted File and the filename/content_type to the host's request
canonicalisation: last path component, control removal, Rust whitespace trim,
120 Unicode scalars and attachment.bin for unsafe/empty names; MIME parameters
removed, Rust trim and ASCII lowercase, or mime_guess for invalid types. Safe
PNG/JPEG/GIF/WebP validation preserves original bytes, name and MIME type (only
the preview is derived). Generate MIME lookup data from the host's library.
A mismatch is a final invalid_upload_response and OutgoingMessages never publishes
it. Genuine legacy hosts remain supported without transfer advertisement.
Acceptance: different size/name/type refuses without publication; canonical names,
default names, normalized/guessed MIME types and preserved raster receipts pass.

## Round-10 publication ACK contract (High 2 validator, 2026-10-13)

message.send ACKs must match the sent command's canonical kind, normalized content,
reply target and complete ordered attachment IDs, and the expected actor for ordinary
messages and vote creation. Rust MessageSend uses clean_message (NUL removal, CRLF/CR
normalization, Rust whitespace trim, Unicode-character truncation); compare that
canonical value, preserving honest sends with raw whitespace/newlines. Vote creation
has empty content and also matches the canonical question, ordered options and
duration (omission means zero), using vote.rs clean_message limits, visible-text
filtering and caseless deduplication. Generate case-fold data from the host library.
Vote cast/withdraw/close are deliberately anonymous public markers:
projection::privacy_minimized_vote_transition clears the actor and ballot choice.
Those ACKs require the same vote kind/ID, empty content, no reply or attachments,
and the existing canonical anonymous event validation, rather than exposing identity.
Round-11 reviewer-of-record scope decision: the host is the authority for its own
rooms; a host lying to its own participants is outside the threat model because
it can fabricate any event. Do not add private actor/choice receipts for anonymous
vote cast/withdraw/close. Clients must prevent host-driven resource exhaustion and
unbounded loops, cross-host/account/room/generation leakage or actions, and automatic
duplicate publication; perform cheap consistency checks when both sides are present.
room.random.* validation remains unchanged. Invalid ACKs follow the socket's existing
contract-violation path. The UI-owned hook remains untouched: OutgoingMessage.eventId
is solely the validated ACK event or the exact event matched by observe.
Owner/entry point: roomSocketValidation::commandAckResultIsValid, used by roomSocketClient.
Acceptance: actor/content/reply/ordered-ID mismatches refuse; real canonical messages,
vote creation/anonymous transitions and random responses remain valid. Round-11
acceptance: different creation question/options/duration refuses; genuine normalized,
truncated, invisible-option-filtered and case-fold-deduplicated commands pass.

## Round-10 archive attachment semantics (Medium 2, 2026-10-13)

Ten-attachment publication requires schema/archive semantic version 95. The live
94→95 upgrade changes only metadata, transactionally after room_file_migration
source-graph validation; no DDL or product data changes. The archive catalog adds
95 with unchanged DDL. Decode source versions 87–94 with at most eight attachments
per event, including nested events in command/create/delete receipts; version 95
uses the current ten-attachment domain contract. Current export/backup manifests
carry 95. Previous binaries only support through 94, so their existing manifest
validation rejects 95 before preflight or writing database payload bytes, even
when the first incompatible event occurs much later. Keep every genuine historical
catalog restorable and preserve existing local state during metadata-only upgrade.
Owners: schema_version, room_file_migration, archive_schema_changes, backup_policy,
backup_codec manifest validation. Acceptance: a nine-attachment historical event or
receipt refuses; genuine old archives restore; current catalog/export agree and
94 upgrade preserves data. No envelope format change or compatibility fallback.

## Round-10 plain capability discovery (Low 1, 2026-10-13)

Only a successful, non-redirected GET /api/room-settings response discovers plain
host upload-transfer, attachment-count and image-preview capabilities. Other routes,
methods and failures never change known capabilities. Each upload-client creation
explicitly re-probes via its existing fetchRoomSettings admission read (native uses
the purpose-bound preference ticket; remote uses its session). Remove the old
GET /api/message-attachments 405 probe: it is not a successful admission response.
Recreating the client re-probes the host; an existing client's capability snapshot
remains stable for its requests. Secure transport's admitted capabilities stay with
its existing owner. Owners: remoteWorkspace::fetchPlainProductTransport and
createMessageAttachmentUploadClient. Acceptance: unrelated headerless/error responses
preserve all three learned capabilities, while a fresh designated probe and newly
created client can reflect an actual host capability change.
