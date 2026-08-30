# Lobby Votes Slice

Status: active design owner

## Definition

A current writable room human or active Agent Session creates and changes one canonical lobby
poll, while every current human—including read-only viewers—reads its exact current summary through
the copied room UI without a second provider-specific or client-owned vote implementation.

## Current contract

The original reachable product represents a poll as one `message_final` event whose event ID is the
vote ID. `vote_cast`, `vote_withdraw`, and `vote_close` are later `message_final` transitions that
reference it. A cast replaces that participant's prior choice, a withdrawal removes it, and only
the first close transition ends the poll early. A finite deadline closes the poll at its canonical
UTC timestamp without a background job; zero or omission means no deadline. The copied composer,
poll card, refresh, cast/withdraw toggle, creator/host close control, countdown, history projection,
and restart behavior are already present but currently fail explicitly against Rust.

Rust keeps the durable event sequence as the public transition history and adds one transactionally
maintained current-vote projection owned by the same persistence mutation. The projection stores the
poll binding, bounded option tallies, total, close state, and one current ballot per participant.
It has no independent writer, reconciliation task, cache, or timer. Poll definition and public
transition content remain in `room_events`; the projection is the single current-summary authority.
Every mutation validates and updates both before one commit, and any failure writes neither.

This split addresses a reachable cost rather than future scale. The room mutation owner admits up
to 14,400 commands per minute, the original summary scans every historical ballot transition, and
the copied poll card refreshes after each transition. Repeated changes to one long-lived poll would
therefore perform a cumulative quadratic scan. The current projection makes a mutation and summary
cost proportional to at most ten options plus one participant ballot lookup while retaining the
append-only public history. Exact before/after measurements and the accepted storage cost must be
recorded with the implementation; the controlled result follows.

The persistence read implementation recorded that evidence on 2026-08-31 with a temporary,
uncommitted debug microbenchmark on Darwin arm64/Apple M5, Rust 1.97.1, and SQLite 3.54.0. One
two-option in-memory poll had 14,400 appended ballot transitions and one current ballot; twenty
sequential reads compared the original-shaped ordered JSON transition query plus `RoomEvent`
decoding with the current authenticated projection summary. The raw path materialized 14,401 rows
and 6,343,053 encoded bytes per read and took 3,612,597 microseconds total (about 180,630 per read).
The projection path returned a 323-byte summary and took 8,305 microseconds total (about 415 per
read). Its one state row plus one current-ballot row held 105 logical text bytes, excluding SQLite
record and index overhead. This is controlled cost evidence, not a production latency claim; the
raw measurement also omitted tally aggregation, so it does not overstate that work. The accepted
trade-off is one bounded state row per poll, one row per current voter, and their indexes in exchange
for avoiding repeated historical materialization. The append-only public history, anonymous public
tallies, viewer-only own choice, current authority check, and atomic event/projection write contract
remain unchanged; the summary adds no write, timer, cache, retry, or background task.

Human vote writes remain the existing `message.send` action with one strict tagged payload:

- `message` accepts exactly content and optional attachment IDs as today;
- `vote` accepts an empty content, question, two through ten case-insensitively distinct options,
  optional zero-or-30-through-86,400-second duration, and optional poll attachments;
- `vote_cast` accepts exactly one vote ID and a choice matching option text case-insensitively or a
  one-based option number;
- `vote_withdraw` and `vote_close` accept exactly one vote ID;
- identity, actor, deadline, and timestamps always come from current server authority.

Questions are canonical visible text capped at 300 characters; each option is capped at 100.
Ordinary read/write humans may create, cast, and withdraw while joined and unmuted. Only the poll
creator or current room operator may close early. Read-only humans cannot mutate but can request
`room.vote.summary`. Agent Bridges cannot use the browser summary action.

`room.vote.summary` is an exact read-only WebSocket action. It revalidates the current human,
capability, room, poll binding, deadline, and current viewer ballot in one read transaction; returns
question, options, duration/deadline, creator, timestamps, tallies, own choice, total, closed state,
close time, and reason; and creates no command result, event, write-budget debit, task, timer, retry,
or alternate HTTP read. Invalid, deleted, cross-room, or malformed polls fail explicitly.

A poll creation is provider-visible ordinary room speech and may enter the existing ordered/ambient
floor once. Ballot and close transitions publish in sequence and refresh poll cards but never queue
another Agent Session turn. Provider room views expose the canonical current poll summary rather
than asking each adapter to reconstruct ballots.

The common RoomPortal owns `create_vote`, `cast_vote`, `withdraw_vote`, and `close_vote` for every
provider transport. A tool call stages one typed terminal outcome; persistence then applies the same
vote owner with the active Agent Session participant and exact turn receipt. No Codex, Antigravity,
OpenCode, DeepSeek, terminal-helper, or future adapter may define vote validation, tallying, or
authority separately. A provider may close only its own poll, and one terminal vote action completes
the current room turn just like a published message.

The copied countdown's deadline-owned timeout is retained only while a finite open deadline exists;
it is cancelled on close/unmount and does not fetch or mutate. Summary refreshes are caused by
mount, explicit user refresh, or sequenced vote events—not polling.

## Non-goals

- message editing/deletion or vote tombstoning, which follows after this owner is complete;
- custom-channel polls, anonymous/secret ballots, multiple-choice ballots, quorum, scheduled polls,
  notifications, analytics, or a generic workflow/voting framework;
- HTTP vote reads, periodic summary polling, deadline workers, Python/legacy compatibility,
  fallback state, local optimistic authority, or a provider-specific vote implementation;
- voice, Mafia, plugin hosting, or the excluded scripted-meeting pipeline.

## Acceptance criteria

1. Exact domain parsing and one persistence transaction create a poll or append a cast, replacement,
   withdrawal, or authorized close while preserving replay identity, room budget, event order, and
   rollback semantics.
2. The current-vote projection and public event history have one mutation owner. Duplicate replay
   changes neither; changed-payload request reuse conflicts; no other SQL, tally, or state-transition
   implementation exists repository-wide.
3. Local and admitted writable humans create, vote, change, withdraw, and close through the copied
   UI. Read-only humans see live totals and deadline state but cannot mutate. Reload and normal
   restart preserve the exact summary and own choice.
4. `room.vote.summary` is strict, current-authority-bound, read-only, and returns the same result for
   local and admitted viewers except their own choice. Deleted, missing, expired-for-write,
   unauthorized, bridge, revoked, and cross-room paths fail closed without hidden work.
5. Poll creation routes once through the existing floor; ballot/withdraw/close events never create
   another queued provider input. Public snapshot, history, and live projection stay cursor-complete.
6. Codex Terra, Antigravity Flash, and OpenCode Hy3-free receive the same common vote tools and use
   the same persistence owner; provider failure remains explicit with no print/exec/legacy fallback.
7. Measured raw-log scan and projection costs, schema/storage trade-off, CPU/memory/disk/latency,
   security boundaries, complete repository gates, real TCP, copied frontend, packaged human flows,
   actual provider flows, cleanup, and both manual reviews are recorded without extrapolation.

## Verification path

- deterministic domain and persistence tests for payload limits, identities, replacement tally,
  deadline/close ordering, exact replay, rollback, read-only summary, and floor-routing separation;
- authenticated TCP tests for writable/read-only humans, strict summary response, revocation, and
  sequenced live updates without mutation on reads;
- copied frontend tests for strict payload/summary acceptance, creator/host controls, countdown
  cancellation, explicit refresh, and absence of polling;
- common RoomPortal contract tests plus one real turn on each required provider;
- isolated packaged local and remote read-only/read-write browser flows, restart, resource cleanup,
  measured costs, `make verify`, and threshold-based critical-web plus Daybreaker manual review.

## Frontend direct-path implementation record

- Prior symptom and cost: the copied composer and poll card constructed vote requests, but the
  current socket client rejected every non-ordinary message before signing it. A separate unused
  `/api/room/vote` helper targeted no Rust route, while ballot/withdraw/close events were discarded
  before they could trigger a live summary refresh. The only working refresh was therefore the
  viewer's explicit action; adding polling would have hidden rather than fixed those ownership gaps.
- Intent and owner: `roomMessagePayload` now owns the one exact copied-request-to-`message.send`
  mapping for ordinary messages and all four vote transitions. `roomVoteSummaryContract` owns the
  exact browser result shape and its internal tally/deadline/closure consistency checks. One shared
  frontend vote-transition predicate replaces the repeated kind lists. The obsolete HTTP helper is
  removed rather than retained as compatibility or fallback behavior.
- Preserved contracts and trade-off: all question, option, duration, identity, permission, and
  mutation authority remains on the Rust server. The UI adds no optimistic ballot state and stores
  only a privacy-minimized transition marker (vote ID, event ID/sequence, room, and timestamp) long
  enough to refresh the visible card; voter identity and choice are not copied into that marker.
  Transition markers are filtered before display and message-count/backfill decisions. This accepts
  one bounded marker per already-received transition instead of a polling task or a second cache.
- Resource and security result: no HTTP request, interval, heartbeat, retry, task, or deadline worker
  was added. Summary reads occur only on mount, explicit refresh, or a sequenced transition. Moving
  payload construction out of the transport reduced `roomSocketClient.ts` from 788 to 746 lines and
  removed the broad legacy-shaped request assembly. The targeted 58 tests, full 102-file/642-test
  frontend suite, TypeScript production build, and original-CSS verification passed. Packaged UI and
  real-provider acceptance remain pending and are not claimed here.

## Manual review record

- Finding: both independent manual reviews identified the historical `77697b7` Low where deadline
  expiry and manual close shared `vote_closed`; `aa77058` restored deadline-first `vote_expired`,
  and both reviewers marked the finding closed with no remaining actionable item.
- Final approval: critical web (`GPT-5.6 Sol`, verified very-high reasoning) and Daybreaker Blue High
  independently approved exact `da62f4c..aa77058`, correction `b49d710..aa77058`, and cumulative
  `b95e128..aa77058` as `APPROVE C0/H0/M0/L0`; cross-review status `MATCHED`. Neither used an
  automated scan, provider, packaged app, or Computer Use for this source review.
