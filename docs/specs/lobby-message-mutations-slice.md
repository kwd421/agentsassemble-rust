# Lobby Message Mutations Slice

Status: active design owner

## Definition

A current writable room human edits an eligible message or deletes an eligible message or poll
through the copied lobby UI, while the room mutation transaction remains the sole owner of
authorization, current history, search, pins, vote state, attachment custody, replay, and public
mutation events.

## Current contract

The original reachable client sends browser-only `message.edit` and `message.delete` WebSocket
commands. Both require the current server-derived `message.modify` capability. Edit accepts exactly
an event ID and content; delete accepts exactly an event ID. Request identity, canonical payload
hash, principal and room write budgets, sequenced publication, ACK ordering, and exact replay use the
existing durable room-command owner.

Only a nondeleted ordinary `message_final` human message can be edited, and only by its current
human author. Content uses the existing 12,000-character canonical message normalization. The
server permits an empty normalized edit only when the message already owns at least one bound
attachment; the copied dialog deliberately keeps its narrower reachable control disabled for an
empty draft. Edit preserves event identity, sequence, author, creation time, and attachments, sets
one server timestamp, refreshes the canonical search record, and appends one `message_updated`
event referencing the target ID and sequence. It never routes the edit to the ordered/ambient floor
and never invokes an Agent Session.

Only a nondeleted ordinary message or poll can be deleted. A human may delete their own human
message, the message or poll of an Agent Session they own, or any eligible target while holding
current room-operator authority. Read-only humans and Agent Bridges cannot mutate. Joined state,
role, ownership, invite scope, and mute remain room-owned facts; mute blocks new speech through the
existing send policy but does not invent a second `message.modify` capability or rewrite historical
ownership.

Deletion retains the target event ID, sequence, kind, creation time, and non-sensitive historical
author attribution needed for its tombstone, while removing content, target routing, attachments,
and—when the target is a poll—its definition. It sets one server deletion timestamp and appends one
`message_deleted` event referencing the target ID and sequence. Poll deletion also removes the one
current vote projection and all current ballots, and redacts every stored ballot/close transition's
actor, choice, vote link, content, and attachment fields while retaining cursor-contiguous
tombstones. Reload, history, search context, pins, provider context, and live projection must expose
no deleted poll definition, ballot choice, voter identity, or attachment reference.

Rust improves the original post-commit cleanup boundary without changing reachable behavior. A
bound lobby attachment belongs to exactly one `(room_id, event_seq)` row and cannot be shared.
Therefore deletion verifies the target event's canonical attachment metadata, deletes only those
exact bound rows, removes the exact search record and pin, updates/redacts affected event rows,
removes the vote projection when applicable, removes that exact event only from Agent Session
`pending_inputs`, appends the public mutation, and stores the command result in one SQLite
transaction. An `inflight_inputs` reference has already crossed the provider boundary and is not
cancelled or reinvoked. Failure commits none of these changes. Pending uploads, profile images,
prejoin avatars, room appearance, other messages, and any referenced asset outside that exact
binding are untouched. No cleanup retry, orphan sweep, reconciliation task, or silent failure is
needed.

The current `room_events` row remains the authoritative history representation after a mutation;
the appended mutation is the sequenced live transition, not a second current-message store. Search
and vote tables remain transactionally derived projections with no independent writer. A changed-
payload reuse of a request ID conflicts; an exact replay returns the stored result without another
event, redaction, deletion, budget reservation, or publication.

## Non-goals

- editing attachments, poll definitions, Agent Session messages, system events, vote transitions,
  custom-channel messages, side chat, or message revision history;
- undo, restore, bulk deletion, retention jobs, moderation audit UI, or a generic mutation framework;
- re-invoking providers after edit, deleting an Agent Session, or treating message ownership as
  participant role authority;
- HTTP mutation routes, optimistic client authority, Python/legacy compatibility, fallback cleanup,
  periodic polling, heartbeat, timer, retry, or reconciliation.

## Acceptance criteria

1. Strict domain parsing rejects aliases, extra fields, malformed IDs, and over-limit content.
   Target validation rejects missing, duplicate, deleted, cross-room, or unsupported event kinds.
2. Current local and admitted read/write humans can edit only their own human ordinary messages.
   Current operators and exact Agent Session owners can delete only the permitted ordinary message
   or poll; read-only, bridge, stale, and unrelated principals fail before any mutation.
3. Edit atomically updates the target and its search projection, appends one exact mutation, records
   one result, and never advances the room floor. Exact replay is inert and changed replay conflicts.
4. Delete atomically tombstones the target, removes its pin and search result, removes only its bound
   attachment rows and pending Agent Session queue references, and appends one exact mutation. Poll
   deletion additionally removes current vote state/ballots and redacts all linked transition
   records without leaking identity or choice. An already-inflight observation remains untouched.
5. Snapshot, paginated history, search/context, pins, attachment reads, vote summary, live events,
   reload, and normal restart all agree on the same post-mutation state. Deleted attachment reads and
   deleted poll summaries fail closed; unrelated assets and messages remain byte-for-byte reachable.
6. The copied controls preserve confirmation, Shift-delete, modal ordering, strict ACK validation,
   edited marker, and tombstone behavior for local and admitted clients. Mutation events do not
   become visible chat rows or provider inputs.
7. Repository-wide review finds one owner for each validation, SQL transition, search replacement,
   pin removal, vote removal/redaction, and attachment deletion policy, with no fallback, meaningless
   polling, heartbeat, periodic timer, unbounded retry, or swallowed failure coupled to the slice.
8. Focused domain/persistence/protocol/server/frontend tests, real authenticated TCP, complete gates,
   isolated packaged local/read-write/read-only flows, resource evidence, exact cleanup, and the
   threshold-triggered critical-web/Daybreaker manual reviews are recorded without extrapolation.

## Verification path

- focused domain tests for exact payloads, normalization, event-kind and authority decisions;
- persistence tests for atomic edit/delete, exact replay/conflict, search/pin/attachment consistency,
  poll transition redaction, rollback injection, and absence of provider assignments;
- authenticated TCP tests for local, admitted read/write/read-only, stale authority, strict ACKs,
  sequenced mutation delivery, reload/history, and failed attachment/vote reads;
- copied frontend tests for controls, dialog behavior, strict projection, and no polling or local
  authority; then `make verify`;
- isolated packaged local and remote browser flows, normal restart, measured CPU/memory/disk/latency,
  exact resource cleanup, and threshold-based critical-web plus Daybreaker manual source review.

## Pending-input lifecycle correction

- Prior threat: manual cross-review traced a busy Agent Session whose queued ordered or ambient
  event was later deleted. The tombstone remained referenced by `pending_inputs`; when the active
  provider turn completed, strict queue validation rejected the contentless target and rolled back
  that unrelated completion. The retained provider result could then be retried by reconciliation.
- Intent and owner: the room-turn scheduler remains the sole pending-queue owner and exposes one
  narrow operation that removes an exact event ID from every room session's pending queue. Message
  deletion invokes it inside the existing mutation transaction before tombstoning. It deliberately
  leaves `inflight_inputs` unchanged because those observations already crossed the provider effect
  boundary; deleting history neither cancels nor reinvokes them.
- Preserved contracts and verification: target authorization, tombstone/search/pin/attachment/vote
  ownership, command replay, active-turn authority, and publication ordering are unchanged. An
  injected command-result failure proved the queued references roll back with the rest of deletion;
  successful deletion removed both ordered and ambient pending references, after which the existing
  provider turn completed once with no replacement assignment. The focused test and warning-denied
  persistence Clippy passed. No fallback, polling, heartbeat, timer, retry, reconciliation cleanup,
  cache, or background task was added.

## Durable command-result owner correction

- Prior structure: manual review found the new mutation path had copied the raw `command_results`
  insert already repeated by room messages, settings, roles, mute, random tools, and Agent Session
  lifecycle. The values currently matched, but multiple production SQL writers could diverge from the
  one replay reader without a mechanical failure.
- Intent and preserved contract: `command_admission` now owns the sole raw result insert beside its
  replay/admission reader. Each product command still owns its result shape, event list, transaction,
  and commit boundary and calls one neutral helper with those exact values. No trait, framework,
  state, alternate transaction, or forwarding-only compatibility wrapper was added.
- Verification: repository-wide source search found one remaining production
  `INSERT INTO command_results`; focused mutation and role tests and warning-denied persistence
  Clippy passed with unchanged exact replay, rollback, event, and budget behavior.

## Room event-sequence owner correction

- Prior structure: the same `MAX(seq) + 1` SQLite allocation was independently defined by room
  turns, Agent Session lifecycle and creation, room settings, human admission, and profile
  projection. The process writer lease and owning SQLite transaction kept current results ordered,
  but a future query change could have split the durable cursor contract silently.
- Intent and preserved contract: one small `room_event_sequence` module now owns that exact query;
  every product writer still allocates inside its existing transaction and owns event construction,
  insert, rollback, and publication. The change adds no sequence cache, counter table, lock, trait,
  migration, or alternate authority and does not alter the existing single-writer cost.
- Verification: repository-wide source search found one remaining production allocator query; the
  full persistence test suite, structure gates, and warning-denied Clippy passed with unchanged
  contiguous cursor, replay, rollback, profile, admission, settings, and lifecycle behavior.

## WebSocket mutation surface exposure

- Prior incomplete boundary: the atomic persistence owner and copied lobby controls existed, but
  the canonical `RoomAction` registry did not contain `message.edit` or `message.delete`. The strict
  browser command parser therefore rejected both controls before they could reach the server-owned
  transaction; the UI alone was not a reachable implementation.
- Intent and owner: the protocol registry now exposes exactly those two WebSocket actions and bumps
  the product-surface revision. Local and admitted-human command dispatch route them directly to the
  existing mutation transaction and publish its existing `CommandOutcome`; no HTTP route, client
  authority, adapter, compatibility path, fallback, or second mutation owner was added.
- Preserved contracts and verification: the existing command admission still owns current-principal
  revalidation, exact request/payload identity, inflight capacity, and principal/room budgets. The
  persistence transaction still owns authorization, edit/delete state, replay, result, and event
  construction, while the common room runtime owns sequenced publication and strict ACK/NACK. The
  protocol suite, all 94 server unit tests, generated-surface checks, focused browser surface/socket
  tests, workspace all-target check, and diff check pass.

## Authenticated TCP mutation verification

- The local authenticated socket creates, edits, exactly replays, deletes, and reloads one durable
  tombstone. Every committed mutation ACK names the same event and sequence as the published event,
  while exact replay emits one deduplicated ACK and no second publication.
- An admitted read/write human creates and mutates only its own message through the session socket.
  A read-only session receives an exact rejected NACK for both mutation actions and no event.
- The three real-TCP cases and warning-denied server Clippy pass. They add no product path, timer,
  retry, polling, fallback, or alternate authority; stale-session revocation and deleted-resource
  boundaries remain separate acceptance units and are not claimed here.
