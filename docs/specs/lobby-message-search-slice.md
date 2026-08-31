# Lobby Message Search Slice

Status: complete and verified

## Definition

A current room human or active Agent Session searches the complete canonical lobby-message record
and reads one bounded chronological context window through the copied product surface without
receiving private event fields or invented custom-channel data.

## Current contract

The authoritative input remains `room_events`. Search indexes only public, non-deleted
`message_final` records with visible text or attachments and matches the casefolded author, visible
content, and attachment filenames. A poll's visible question is its search content. Private ballot
and close transitions have neither visible text nor attachments and never enter the derived index.
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

Codex, Antigravity, OpenCode, and the common API adapter receive that same provider-neutral ingress.
Antigravity maps `search` and `context` through its existing private helper and exact PreToolUse hook;
it adds no transcript, printed-result authority, request file, second queue, process, timer, polling,
or fallback. Query cleaning, cursor size, and message-target identity reuse their domain owners, and
the helper prints only the canonical MCP JSON returned by the same RoomPortal tools.

The hook previously rejected command substitution and shell control operators but admitted unquoted
POSIX pathname, brace, and tilde expansion. A room instruction could therefore induce a command such
as `speak *` and publish workspace filenames after the shell expanded the otherwise allowlisted
argument. The POSIX grammar owner now rejects those unquoted expansions while preserving explicitly
quoted or escaped literals; the distinct Windows `cmd.exe` grammar remains unchanged. Focused hook
tests cover both the exact helper commands and expansion denials. This correction adds no runtime
state or background cost and preserves the existing private-helper, one-command, turn-budget, and
terminal-outcome contracts.

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
and whitespace-insensitive behavior.

A removed release probe then measured the final schema with 100,000 representative canonical
messages. Canonical storage was 54,829,056 bytes, the complete search projection was 46,383,104
bytes, and the live database was 101,576,704 bytes. After 100 production-path `message.send` writes,
those values were 54,882,304, 46,428,160, and 101,752,832 bytes. Across 25 reads, a selective query
had 21.861 ms median, 22.539 ms p95, and 22.723 ms maximum latency; an absent query had 20.732 ms
median, 21.394 ms p95, and 21.691 ms maximum latency. Across the 100 writes, latency was 0.530 ms
median, 0.772 ms p95, and 1.245 ms maximum. The release probe reported 16,449,536 bytes maximum RSS
and a 7,160,288-byte peak memory footprint. Search owns no background task, timer, process, or polling
loop; page and context allocations remain bounded by 30 and 31 results on the existing single SQLite
connection. Dataset construction took 4.978 seconds but is not claimed as a production write metric.
The probe source and generated database were removed after measurement.

### Copied-frontend authority cutover evidence

The copied frontend previously sent a durable room-session bearer directly to the search endpoint,
could issue an unauthenticated local request, substituted only the loaded in-memory timeline after an
empty canonical result, and cast lobby context into custom-channel events despite the absence of a
Rust custom-channel message owner. Those paths could cross purpose authority, hide failed canonical
reads, or present invented history.

The frontend now resolves one local-or-remote `RoomHttpAuthority`, obtains a fresh one-use
`message-search-read` grant for each search or context read, sends only that grant to the target, and
validates the complete private/no-store response before exposing it. The parser accepts only the
currently emitted visible lobby `message_final` variants, including a strict canonical poll
definition, and rejects unknown or private fields,
and bounds pages, context windows, strings, attachment metadata, sequence order, and target identity.
Room, channel, or authority changes synchronously invalidate pending requests and clear their visible
query/results. Concrete custom channels report the unimplemented owner rather than synthesizing
context or falling back to loaded events. Message attachments, pins, and search share only the small
HTTP-authority value; each feature retains its own ticket purpose, parser, and lifecycle owner.

This cutover adds no persistent browser state, worker, process, disk owner, compatibility path, or
generic provider/search framework. Per response, validation is bounded by the existing 30-result or
31-event wire limits and existing attachment limits. No runtime latency or bundle-size improvement is
claimed. `make verify` passed the architecture and 800-line gates, generated bindings, the production
frontend/original-CSS build, 98 frontend files with 612 tests, 26 desktop tests, every workspace unit,
integration, and real-TCP boundary test (including search ticket replay/cross-purpose/wrong-room and
post-exchange revocation), document tests, warning-denied Clippy, formatting, and diff checks. The
final whole-repository gate and packaged evidence are recorded in `docs/VERIFICATION.md`.

### Packaged and real-provider evidence

An isolated copied release client searched an older lobby message, navigated through its bounded
before/target/after context, and retained both the canonical message record and search result across
a normal application restart. An admitted read-only browser performed the same search and context
navigation, remained unable to write, and retained its result across reload. Revoking its reusable
invite rejected a fresh identity while the already admitted session remained valid under its own
session authority; this preserves the existing future-admission-only revoke contract rather than
inventing session revocation.

Actual Codex `gpt-5.6-terra` Low and Antigravity `gemini-3.6-flash` Medium sessions each received only
the non-inferable target `CTX_TARGET_q4v8n1`, invoked their RoomPortal search and context helpers, and
returned the withheld adjacent values `CTX_PRE_a7p9m2|CTX_NEXT_z6k3r5`. OpenCode
`opencode/hy3-free` remained visibly unavailable: the packaged turn entered recovery-required with
no fabricated result, and the separately authorized installed free-model CLI probe returned an
external provider error. No mock, alternate model, print mode, transcript path, or fallback was
used. The exact app, provider children, server, tunnel, isolated data, caches, and regenerated
measurement artifacts were stopped or moved to Trash after verification; Computer Use was reset.

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
