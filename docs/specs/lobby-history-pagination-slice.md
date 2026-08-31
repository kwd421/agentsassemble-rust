# Lobby History Pagination Slice

Status: packaged-verified implementation owner; correction cross-review approved

## Definition

A current room human reads the canonical public lobby event record before an exclusive sequence
cursor through the copied WebSocket timeline, without turning a read into a durable mutation or
crossing the authenticated frame limit.

## Current contract

The copied client requests `room.history` while initially backfilling a short snapshot and when the
reader scrolls to the top. Rust now serves that action on the public authenticated WebSocket and the
copied control consumes its strict page validation. It returns an exclusive `before_seq` page in
chronological order, defaults and caps `limit` at 200, reports the page's `oldest_seq`, the read
transaction's room `last_seq`, and whether an earlier event exists, and writes no command-result
record.

Rust `room_events` remains the only history authority. One persistence read transaction revalidates
the current principal and `room.history` capability, fixes the room high-water sequence, selects at
most 200 records immediately before the requested cursor, verifies stored and decoded room/sequence
identity, and applies the same public event projection used by snapshot and live delivery. An
invisible event becomes the existing minimal `event_hidden` envelope; it is not removed from the
viewer cursor.

`before_seq=0` means the newest page. Any positive cursor is exclusive, including one above the
current high water. Results are chronological, and the next request uses the returned nonzero
`oldest_seq`. Empty pages return `oldest_seq=0`. `last_seq` is the same transaction's durable room
high water, not the last event in the page. `has_more_before` is true only when a durable earlier
event exists.

History stays on the authenticated WebSocket because its request is connection-, cursor-, and
ordered-ACK-coupled. It does not enter the room mutation queue, principal mutation admission,
durable room write budget, command replay table, provider bridge, or an HTTP route. Both local and
admitted human sessions converge on the same current-principal persistence read. Agent Bridges do
not receive browser history pages and continue using their assigned RoomPortal context/search
contract.

The process socket-admission owner charges both one history read and its requested event count after
exact payload parsing and before the SQLite read. Fixed ten-second request/event ceilings are
`10 / 1,000` per principal, `320 / 3,200` per room, and `640 / 6,400` per process. The principal
ceiling preserves one complete copied-client interaction (its existing maximum is five 200-event
pages), while the request dimension prevents tiny limits from bypassing transaction cost and the
event dimension bounds connection- and room-sharding against the single SQLite connection.
Exceeding either independent read dimension returns a definitive
`history_read_limited` NACK without closing the socket, consuming mutation budget, or disabling
ordinary frames. The same mutex first preflights all three scopes and commits request/event cost only
for a read that all three admit. A saturated narrow scope therefore remains saturated until its
fixed window turns over but cannot debit a broader scope for work that never reaches SQLite. The
separately owned raw frame attempt remains charged under the existing ingress contract. No retry,
timer, worker, or alternate read path is introduced.

The product inner-frame ceiling is 256 KiB. The server first preserves the requested newest-near
cursor subset and then removes only the earliest returned records until the exact correlated ACK
fits that existing encoder. This cannot skip history: a shortened page reports its actual oldest
sequence and `has_more_before=true`, so the next exclusive request retrieves the removed earlier
records. A single canonical event already fits because message content and attachment metadata are
independently bounded. Oversize or encoding failure is explicit; it does not close and reconnect as
a pagination fallback.

The copied frontend accepts only a complete bounded page: no more than 200 events, strictly
increasing sequences below the requested cursor, exact room identity, valid public projection,
matching oldest sequence, a sane high water, and a consistent `has_more_before` relation. A failed
page remains visibly retryable and never merges partial or locally fabricated history. Existing
anchor preservation and one-user-interaction/one-page scheduling remain the browser lifecycle owner;
this slice adds no poll, heartbeat, recurring timer, worker, cache, or compatibility path.

## Non-goals

- message edit or deletion, vote operations, pins, search, or attachment behavior changes;
- per-user read cursors, unread synchronization, restart anchor restoration, or virtualization;
- custom-channel history, side chat, friends, direct messages, voice, or Mafia;
- Agent Session search/history tool redesign or a generic pagination framework;
- HTTP history, Python/legacy reads, schema migration, fallback data, or placeholder results.

## Acceptance criteria

1. The public protocol advertises exactly one `room.history` action, and local, read/write, and
   read-only current humans can use it while bridges, revoked sessions, absent membership, and
   missing `room.history` permission fail closed.
2. Payload decoding is exact and bounded. Pages preserve the original exclusive-cursor, newest-page
   zero cursor, chronological order, 200-event maximum, transactional high water, and no-more flag.
3. Authorization, high-water selection, page read, identity validation, and public projection share
   one persistence transaction. Private fields never cross the page and hidden events retain cursor
   continuity.
4. Small-message pages return the requested 200 events. Large-message pages fit the existing exact
   authenticated ACK limit by shortening only from the early side, with no skipped sequence between
   successive pages and no silent socket-close retry.
5. The read creates no room event, command result, write-budget debit, mutation queue item, provider
   wake, background task, timer, poll, or disk owner.
6. The copied packaged client initially backfills a short tail, loads exactly one older page per top
   interaction, preserves its visible anchor, exposes failure/retry, and works for an admitted
   read-only browser across normal restart or reload.
7. Focused persistence, protocol, frontend, and real-TCP boundaries plus the complete repository
   gates pass. Query, serialization, memory, and wire-size costs are measured before completion.

## Admission correction evidence

Manual review found that a tiny `room.history` frame could previously cause a worst-case read and
decode of 201 stored events plus repeated exact ACK encoding without a corresponding workload
debit. With 12,000-character message content this is roughly 2.4 MiB of stored JSON per maximum
page; even a one-event page begins the same authorization/high-water/page transaction, while the
store intentionally owns one SQLite connection. The existing release real-TCP
large-event case used about 101 MiB peak process memory and 0.09 seconds of test-body time for 51
events; exact 200-event frame fitting took about 6 milliseconds per release process including test
startup. These measurements do not justify caching, another connection, or a generic work
framework, but they establish a reachable CPU/memory/latency amplification boundary.

The correction keeps raw frame, history-request, and requested-event counters as independent
dimensions inside the one process socket-admission owner. It preserves parser, current authority,
projection, cursor, exact frame fitting, read-only access, and the copied five-page interaction.
The accepted trade-off is a visible rejection of additional history work inside the same fixed
window. Focused owner tests prove principal and room aggregation plus small-limit resistance without
wall-clock waiting; the authenticated TCP boundary proves the sixth maximum request is rejected
while the same socket still answers a ping. The complete `make verify` gate then passed: structure,
800-line growth, policy, formatting, workspace build and generated types, copied frontend build/CSS
and 628 tests, desktop checks and 26 tests, all Rust unit/integration/doc tests, and warning-denying
Clippy.

Daybreaker then found that the first correction charged all three history scopes before combining
their decisions. After a principal reached its 1,000-event ceiling, 28 more rejected 200-event
requests could therefore consume the remaining process event budget without another database read
and temporarily deny unrelated rooms. The corrected owner now resets expired windows, preflights
principal, room, and process counters under the same mutex, and commits all three only when the read
is admitted. It retains no reservation or refund state: previously admitted cost remains permanent
for the fixed window, while rejected parsing/response cost is already bounded by raw ingress.
A deterministic same-instant regression saturates one principal, repeats more than a complete
process budget of rejected work, and proves both a peer in that room and another room remain
admissible. This removes cross-scope availability debit without weakening the original DB-work cap.

## Strict browser acceptance evidence

The protocol exporter now derives `ROOM_HISTORY_MAX_EVENTS` from the Rust domain owner, so request
defaults and ACK bounds have no independent TypeScript literal. Snapshot, live-event, and history
validation share one public-event predicate; history additionally requires no more than the exact
requested limit, the bound room, a contiguous increasing range below the exclusive cursor, exact
oldest and high-water anchoring, and `has_more_before` consistent with the first durable sequence.
Only a fully validated result is returned to the projection, replacing the copied client's numeric,
boolean, and empty-array coercions. The bounded validation is one pass over at most 200 events and
adds no cache, timer, retry, fallback, or persistent state. Focused history/controller tests passed
22 assertions, the complete frontend passed 101 files and 639 tests, and the production build plus
original-CSS verification passed. Packaged local and admitted read-only verification remains the
active acceptance boundary rather than being inferred from these deterministic checks.

## Packaged browser evidence

The isolated macOS release `AgentsAssemble History Verify 831A`
(`app.agentsassemble.rust.historyverify0831a`) was built from local `bfb6ccf`, whose parent is the
pushed server correction `b95e128`. The original comparison remains `d5046473010d1353a81ee38337360e6d98f7bd6f`.
Through the copied composer, the local operator committed 205 distinct messages. The resulting
durable room record was contiguous through sequence 206 including room creation. After a normal
application quit and relaunch, the fresh subscription omitted the earliest messages; scrolling the
copied lobby to the top made the first message and true channel introduction visible. No local
history source or fabricated page was present.

The host then opened its owned quick tunnel and issued one one-use read-only human invite. A fresh
Chrome incognito window admitted `History ReadOnly 831A` through the production join flow. The URL
credential was removed after admission, the composer remained disabled with the read-only reason,
and the initial snapshot did not contain the first message. One top interaction made the first
message and channel introduction visible. Reload restored the same admitted session without the
credential in the URL and repeated the same latest-snapshot then pre-cursor-history behavior.

Immediately before the final read the database contained 208 room events and 206 command results;
after reload and another complete top-history interaction both counts were unchanged. The invite
row was `read_only`, `use_count=1`, `max_uses=1`. Thus the observed history path created no room event
or command result. No provider ran. The incognito window closed without touching the normal Chrome
window, public ingress stopped through its owning UI, and process inspection found no owned tunnel,
desktop, supervisor, sidecar, or database writer after normal quit. The exact package and
identifier-specific Application Support, cache, and WebKit data were moved to the recoverable
`~/.Trash/AgentsAssemble-History-Verify-20260831-0415` bundle, and Computer Use was reset.

On the pushed correction, Daybreaker Blue High manually re-reviewed `4cbbdcd..b95e128` and
`e1f7cca..b95e128`, marked the missing pre-read admission High and cross-scope debit Medium closed,
and returned APPROVE C0/H0/M0/L0. It ran no automated security scan, tests, provider, or app. The
critical web review and the local frontend commit's threshold batch remain pending and are not
claimed as approved here.

## Browser live-window ownership correction

- Finding: the later Daybreaker catch-up review found one Medium where the canonical React owner
  retained every raw page and live event for every visited room, sorted and reprojected the full
  array after each batch, and then republished that full timeline through a second callback for the
  lobby to merge again. The existing room ceiling of 14,400 commands and 32 MiB of accepted command
  content per minute made the hidden duplicate's linear heap and whole-history CPU growth concrete.
  The same review found one Low where the source gate counted 500--799-line review candidates but
  printed only the 800-line strong subset. The critical web review of the same pushed cumulative
  range reported no actionable finding and `APPROVE C0/H0/M0/L0`; Daybreaker reported
  `REVISE C0/H0/M1/L1`.
- Intent and owner: the canonical socket now retains only the same generated 200-event bound used by
  a server history page and evicts all inactive-room raw event arrays when an authoritative initial
  window replaces the active projection. Explicit history pages are projected once and returned to
  the lobby instead of being copied into the live socket window. The lobby remains the one owner of
  messages the user is actually displaying; it can keep scrolling through ordinary paginated
  history, so this correction does not impose a new retention policy or virtualization framework on
  reachable chat behavior. The current canonical participant profile is reapplied at the display
  boundary, including to older pages, so separating raw custody does not create a stale identity
  cache. A non-resume snapshot revision replaces that display window, while a
  resume or live batch merges into it. The duplicate app-controller callback and full-timeline
  republish path were removed.
- Mutation and profile invariants: a history page is projected with the current canonical
  participant/Agent Session profile at read time. A later `message_updated` or `message_deleted`
  event whose target has already left the 200-event raw window is projected as a private transition;
  the lobby applies it to a currently displayed record by exact durable event ID and never renders
  or retains the transition as a chat row. Snapshot replacement, cursor-contiguous history,
  mutation tombstones, current profile projection, vote revision signals, scroll anchoring, and the
  explicit jump-to-latest control remain intact. No HTTP path, cache, polling, heartbeat, retry,
  fallback, worker, or timer was added.
- Trade-off and verification: one displayed lobby history can still grow when the user deliberately
  reads more pages or leaves a long live conversation visible; bounding that state would change the
  current scroll contract and requires a separately specified bidirectional/virtualized reader.
  This correction removes only the avoidable hidden raw duplicate and inactive-room retention. A
  focused regression proves 201 live raw events retain sequences 2--201 and advance the history
  cursor, while page results remain separate and current-profile-projected. Mutation projection,
  historical vote revision, lobby paging, canonical synchronization, and production frontend build
  checks also pass. Complete `make verify` passed the architecture/source gates, generated protocol
  check, 647 frontend tests and production/CSS build, 26 desktop tests, every workspace unit,
  integration, and doc test, and warning-denied workspace Clippy. Final correction re-review is
  pending and is not claimed as approved here.

`useCanonicalRoom.ts` is in 800--1,000-line structure-review territory after this correction. It
remains intact because the live-window and history-page paths share one accepted socket, projection
generation, current-profile state, cursor authority, and fail-closed operation check. Moving either
path out would add public React state transfer and socket/projection interfaces while obscuring the
one acceptance invariant; no unrelated domain, lifecycle, or second state owner was added. This is
a reviewed cohesive-file decision, not a LOC exception or a reason to raise a gate.

The gate correction now prints every 500-line review candidate with its file name while preserving
the distinct 800-line strong label and 1,000-line rejection. Its command-output regression and the
policy test suite pass; it adds no exception or threshold change.

Correction cross-review found four Medium issues and no Critical, High, or Low issue. Daybreaker
found that replaying the same off-window mutation changed object identity on every merge, producing
a deterministic React update loop, and that an in-flight history page was authorized only by room
ID, allowing a pre-resync page and scroll anchor to enter a replacement window. The critical web
review found that a fixed search-context window ignored edits and deletes for records already on
screen, and that unique, non-rendered vote-transition rows accumulated in the deliberately
unbounded display array. Both reviews returned `REVISE C0/H0/M2/L0` for `881c0c4` and its cumulative
range; these are the only findings recorded from those reviews.

The correction keeps mutation folding referentially idempotent, binds page acceptance, failure
state, completion, and scroll anchors to the exact room, canonical-window revision, and request
identity, and reconciles only canonical mutations whose durable record is already present in a
fixed search window. Ordinary displayed messages remain unbounded under the existing scroll
contract. Vote-transition rows are no longer retained there; one latest revision token is kept only
for each non-deleted poll currently represented in displayed history. This bounds hidden vote state
by visible poll count while preserving vote-summary refresh, current search context, page cursors,
snapshot replacement, and exact edit/delete behavior. It adds no timer, retry, polling, fallback,
cache, worker, or new authority. Focused idempotency, replacement-window, fixed-context mutation,
and vote-revision regressions pass; the complete frontend suite passes 650 tests and the production
TypeScript/Vite/original-CSS build. Final correction re-review remains pending and is not claimed as
approved here.

Daybreaker correction re-review found one additional Medium: single-transition identity reuse did
not cover a retained batch with two transitions for the same target, so replaying edit A then edit B
could still rebuild the final B object and retrigger the effect. The display merge now coalesces a
canonical batch to its final transition per durable target before folding it. Two-edit and
edit-then-delete replays both preserve the already-final target identity in focused regression, and
the production frontend build passes. Re-review of this follow-up is pending; no approval is claimed.

The critical-web correction re-review found one further Medium: search-context, jump-to-latest, and
post-success display replacements did not retire a same-room, same-window page request, so its page,
failure, or scroll anchor could re-enter the replacement display. The hook now has one private
request-retirement operation that clears the exact request identity, anchor, loading flag, and
obsolete load error before every display replacement; stale promise continuations are consequently
inert under the existing identity check. A controlled pending-page regression proves a search
context remains exact after the old page resolves.

The final critical-web re-review found one Low stale-scroll race: a consumed anchor's already
scheduled animation-frame callback could outlive request retirement. The private retirement owner
now invalidates restoration by epoch, room, and canonical-window revision. Final manual re-review
by the critical web session and Daybreaker found no remaining actionable issue and approved
`a958bab`, `33cd2b9..a958bab`, and pushed HEAD with `C0/H0/M0/L0`.

## Verification path

- focused parser and persistence tests for cursor edges, 200/remaining pages, current authority,
  projection redaction, hidden sequence continuity, and zero writes;
- authenticated real-TCP tests for local/read-only success, revocation, bridge/permission denial,
  exact response correlation, and multi-page frame fitting;
- copied frontend controller/view tests for strict rejection, initial backfill, top-scroll anchoring,
  one-page scheduling, and visible retry;
- fresh packaged local and isolated read-only browser verification followed by exact resource cleanup;
- final query/wire/memory measurement, `make verify`, and threshold-based critical-web plus
  Daybreaker Blue High manual source review.
