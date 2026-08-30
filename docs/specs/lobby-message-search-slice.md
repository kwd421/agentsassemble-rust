# Lobby Message Search Slice

Status: active implementation owner

## Definition

A current room human or active Agent Session searches the complete canonical lobby-message record
and reads one bounded chronological context window through the copied product surface without
receiving private event fields or invented custom-channel data.

## Current contract

The authoritative input remains `room_events`. Search indexes only public, non-deleted
`message_final` events and matches the casefolded author, visible content, and attachment filenames.
It preserves the original SQLite `unicode61` phrase behavior for every query and additionally uses
the original whitespace-insensitive match when the compact query has at least three Unicode scalars.
Results are newest-first in pages of 30; context is the selected event with at most 15 earlier and 15
later lobby messages in chronological order.

Search metadata is a derived SQLite projection, not message authority. One minimal record table owns
the event pointer, canonical creation order, casefolded search text, and compact text. A contentless
`unicode61` FTS table preserves the phrase candidate path without copying result content. The canonical
message transaction inserts both projections, and database-owned deletion removes the FTS row whenever
its record loses its canonical reference. There is no sync-on-read task, second database, raw-event
copy, compatibility rebuild, or migration path.

Human reads use `GET /api/room-search` and `GET /api/room-search/context` with a fresh, purpose-bound,
one-use `message-search-read` ticket. Local desktop issuance and remote session exchange converge on
that one purpose, the ticket is consumed before request validation, and persistence revalidates the
current room human plus `room.history` permission in the same read transaction. Both responses are
private/no-store. An invalid nonempty cursor fails rather than silently restarting pagination.

`channel_id=lobby` and `channel_id=all` currently search the same implemented lobby owner. Any
concrete non-lobby channel remains explicitly unavailable until custom-channel messages have a Rust
authority. No HTTP fallback supplies local visible events when the canonical search request fails.

Agent tools reuse the existing RoomPortal MCP and room-actor ingress. `search_messages` and
`read_message_context` carry exact turn authority to the same persistence search owner, expose no
human bearer or filesystem request queue, require the current discussion receipt, and return the
same bounded public projections. They are read-only: they neither publish a message nor synthesize a
meeting decision. Existing per-turn tool admission remains the resource owner; tabletop random
availability remains separately gated.

### Measured design evidence

A temporary release probe used 100,000 representative canonical messages and was removed with its
generated artifacts after measurement. Re-reading, decoding, casefolding, and compacting canonical
JSON for an absent or old match took 0.65–1.09 seconds per query. A minimal normalized record scan
took 62–68 milliseconds for the same worst-case reads and less than one millisecond when the page
filled from current messages. The measured minimal records and page index used about 28.5 MiB next
to about 47.8 MiB of canonical rows and their primary index. A contentless `unicode61` index added
about 6.4 MiB; direct deletion was verified. The original-style duplicated result table plus two
content-storing FTS indexes added about 100 MiB and was rejected.

The accepted trade-off is one bounded normalized string projection per public lobby message plus the
small contentless phrase-candidate index. It removes repeated attacker-triggerable JSON
parsing/casefolding from the single SQLite connection while retaining complete-history phrase-token
and whitespace-insensitive behavior. Final-schema CPU, disk, latency, deletion, and concurrent-writer
impact must be remeasured before the slice exits; no broader performance claim is made.

## Non-goals

- custom-channel message storage, search, context, attachments, or pins;
- message edit/delete implementation or a speculative mutation framework;
- room voice, Mafia, side chat, friends, or generic plugin search;
- provider internet browsing or a generic provider-tool adapter rewrite;
- PostgreSQL, Python/legacy reads, index migrations, fallbacks, or placeholder results;
- v0 scripted research, agendas, forced rounds, synthesis, decisions, tasks, or artifacts.

## Acceptance criteria

1. A fresh Rust authority atomically indexes every committed public lobby message and removes every
   record/FTS row whose canonical owner is deleted, without indexing hidden, deleted, or non-message
   events.
2. Search covers messages older than the current snapshot, Unicode casefold, whitespace-insensitive
   phrases, author/content/attachment filenames, exact short-token behavior, and 30/30/remaining
   newest-first pagination with strict opaque cursors.
3. Context rejects unknown/non-lobby targets and returns at most 31 canonical events in chronological
   order with all shared public-event redaction, including `provider_turn_id`, intact.
4. Local operator, read/write guest, and read-only guest reads succeed only while current
   `room.history` authority remains valid. Missing, expired, replayed, crossed-purpose, wrong-room,
   and revoked grants fail closed without leaking search or context data.
5. The copied frontend has one strict local/remote authority path, validates the complete bounded
   response before use, never falls back to visible in-memory messages, paginates, and navigates to
   an old result through the server context window.
6. Current provider harnesses expose both search tools through one provider-neutral RoomPortal
   contract, enforce exact active-turn/receipt/budget ownership, and prove search/context without
   exposing a human token or bypassing terminal-outcome rules.
7. A fresh packaged local flow and isolated admitted read-only browser search, navigate, restart, and
   revoke successfully. Authorized real Codex Terra, Antigravity Flash, and OpenCode free-model
   sessions exercise the same RoomPortal search contract where their configured harness permits it;
   unavailable credentials or provider behavior remain explicit rather than mocked.
8. Final measurements record canonical/search disk bytes, representative selective and absent-query
   latency, write cost, and bounded memory/task behavior. The full architecture, policy, source-line,
   generated-binding, frontend, Rust, Clippy, formatting, and diff gates pass.
9. Computer Use owns only the packaged verification resources and removes the exact app, children,
   isolated data, caches, and regenerable artifacts when verification ends.

## Verification path

- focused domain/persistence tests for normalization, indexing, pagination, context, redaction, and
  deletion lifecycle;
- real loopback TCP tests for local/remote tickets, replay/cross-purpose/wrong-room denial, response
  headers, and context bounds;
- focused RoomPortal MCP/terminal/provider tests for exact turn and receipt ownership;
- copied-frontend API/controller tests followed by packaged local and isolated-browser verification;
- representative release measurements of final schema/read/write costs;
- `make architecture-check`, `make verify`, and exact pushed-range critical-web plus Daybreaker Blue
  High manual source review at the configured batch threshold.
