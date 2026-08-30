# Lobby History Pagination Slice

Status: active design owner

## Definition

A current room human reads the canonical public lobby event record before an exclusive sequence
cursor through the copied WebSocket timeline, without turning a read into a durable mutation or
crossing the authenticated frame limit.

## Current contract

The copied client already requests `room.history` while initially backfilling a short snapshot and
when the reader scrolls to the top. The original reachable server returns an exclusive
`before_seq` page in chronological order, defaults and caps `limit` at 200, reports the page's
`oldest_seq`, the transaction's room `last_seq`, and whether an earlier event exists, and writes no
command-result record. Rust currently advertises the `room.history` capability but omits the action
from its public product surface, so the copied control fails explicitly.

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
