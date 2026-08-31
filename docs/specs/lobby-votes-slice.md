# Lobby Votes Slice

Status: completed Rust contract and packaged acceptance

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
6. Codex Terra, Antigravity Flash, and OpenCode Muse Spark receive the same common vote tools and
   use the same persistence owner; provider failure remains explicit with no print/exec/legacy
   fallback. Add Grok to this matrix only after the live OpenCode catalog exposes an eligible model
   whose cost and tool-call capability have been verified.
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
  Transition markers are filtered before display and message-count/backfill decisions. The normal
  canonical history remains server-window-bounded. A fixed search-history window retains only the
  latest event ID per poll actually displayed in that window and never merges transition records
  into the displayed history.
- Resource and security result: no HTTP request, interval, heartbeat, retry, task, or deadline worker
  was added. A manual review found that a successful local cast, withdrawal, or close initially read
  the summary once after its ACK and once again after the same durable sequenced event. Each read
  repeated the current-authority SQLite transaction, so the sequenced transition is now the single
  post-write refresh owner; mount and explicit user refresh remain separate intentional reads. This
  removes one direct read per local transition without cache, debounce, timer, or optimistic state.
  A follow-up review found that the fixed message-search history window discarded every live event,
  including that owner. It initially admitted privacy-minimized transition markers, but later manual
  review observed that each ballot replacement could append another marker and repeatedly copy the
  growing array; the existing 14,400-command/minute room budget made an unbounded tab-memory and
  latency path concrete. The fixed window now ignores ordinary messages and transition records,
  keeps one latest revision token only for each poll already displayed, and ignores unrelated poll
  IDs. A focused test proved two transitions collapse to the latest token while an outside poll and
  ordinary message add no retained key or visible row. Historical polls still refresh without
  polling, voter identity, choice, or unbounded event retention.
  Cross-review then followed that path to its earlier search owner and found a reachable privacy and
  functionality mismatch: the raw search index admitted contentless ballot transitions before public
  projection, so human search could expose the transition author's name and the shared Agent Session
  context path could expose both voter identity and choice; meanwhile the strict browser context
  decoder rejected the poll definition itself. The derived index now reuses the existing
  `message_visible_text` owner, indexes the visible poll question, and excludes every contentless
  transition. The common public-event projection also removes its old Agent Bridge exception, so all
  consumers retain only the privacy-minimized transition marker. The browser accepts only an exact
  poll definition under limits generated from the Rust vote owner and continues to reject transition
  records. The canonical event log remains unchanged, so historical poll cards are reachable and
  refresh from sequenced events without making ballot history searchable or provider-visible.
  The next manual review found that finite polls were still rejected because their domain-owned
  deadline uses Chrono's exact `+00:00` UTC offset while ordinary event timestamps use serde's `Z`
  form. The search decoder now preserves those two strict wire owners instead of broadening its
  event timestamp parser: finite deadlines accept only the producer's `+00:00` form, and a test uses
  that real shape while rejecting an equivalent nonzero offset. This changes no stored event,
  deadline calculation, timer, or compatibility policy. The focused message-search suite passed
  all seven tests, and the complete `make verify` passed the same structure, production build,
  104-file/644-test frontend, desktop, Rust workspace, real TCP boundary, and warning-denied Clippy
  gates recorded below.
  Moving payload construction out of the transport reduced `roomSocketClient.ts` from 788 to 746
  lines and removed the broad legacy-shaped request assembly. The final correction's `make verify`
  passed every structure/800-line/source-growth/policy/generated/CSS/diff gate, the production
  frontend build and 104-file/644-test suite, 26 desktop tests, 51 domain tests, 227 persistence
  tests, 6 protocol tests, 150 provider tests, 94 server tests plus the real TCP/integration suite,
  and warning-denied workspace Clippy. Packaged UI and real-provider acceptance remain pending and
  are not claimed here.

## Manual review record

### Provider terminal-path implementation

- Prior gap and intent: provider turns could finish only with ordinary speech or decline, so an
  Agent Session could not use the same room-owned poll state transitions as a human. The common
  RoomPortal now stages one typed create/cast/withdraw/close terminal outcome, and the exact active
  turn transaction applies it through the existing domain and persistence vote owner. Provider
  adapters do not own vote validation, ballot state, close authority, sequence allocation, or
  publication.
- Preserved contracts and resource result: the transaction revalidates the execution receipt,
  input cursor, durable Agent Session participant, membership, mute state, and poll ownership before
  committing the vote and common turn finalization atomically. Provider poll creation rejects
  browser upload custody and is the only vote transition routed to the floor. DeepSeek and
  Antigravity converge on the same staged outcome; Antigravity's private helper and one-use hook do
  not treat transcript or printed text as publication. No durable state, network polling, heartbeat,
  retry, cache, background task, or compatibility path was added.
- Verification result: `make verify` passed all structure/source-growth/policy/generated/CSS/diff
  gates, the production frontend build and 104-file/644-test frontend suite, 26 desktop tests,
  52 domain tests, 228 persistence tests, 6 protocol tests, 152 provider tests, 94 server tests plus
  the actual TCP/integration boundaries, and warning-denied workspace Clippy. Packaged UI and the
  three real-provider acceptance flows remain pending and are not claimed by this result.

- Finding: both independent manual reviews identified the historical `77697b7` Low where deadline
  expiry and manual close shared `vote_closed`; `aa77058` restored deadline-first `vote_expired`,
  and both reviewers marked the finding closed with no remaining actionable item.
- Final approval: critical web (`GPT-5.6 Sol`, verified very-high reasoning) and Daybreaker Blue High
  independently approved exact `da62f4c..aa77058`, correction `b49d710..aa77058`, and cumulative
  `b95e128..aa77058` as `APPROVE C0/H0/M0/L0`; cross-review status `MATCHED`. Neither used an
  automated scan, provider, packaged app, or Computer Use for this source review.
- Finding: the critical web review of `a0725be` found one Low where finite vote deadlines emitted as
  `+00:00` were rejected by the search context's `Z`-only parser. `21792df` separated the two exact
  timestamp wire owners and the reviewer marked the finding closed; the prior searchable-ballot
  privacy Medium remained closed.
- Final approval: critical web (`GPT-5.6 Sol`, verified very-high reasoning) approved `21792df`, exact
  `a0725be..21792df`, full correction `04050fa..21792df`, cumulative `aa77058..21792df`, and related
  repository-wide owners as `APPROVE C0/H0/M0/L0`. Daybreaker Blue High could not run because its
  real usage allowance is unavailable until 2026-09-06; under the user's standing instruction, the
  same very-high session performed the security cross-review. No automated scan was used.
- Finding: the provider terminal-path review found no actionable item. It independently ruled out
  external Browser-to-AgentBridge authority escalation, temporary provider text or Antigravity
  transcript/print publication, competing vote/idempotency/sequence owners, partial terminalization,
  and unbounded polling or fallback introduced by this batch.
- Final approval: after a replacement session approved the review plan under Pro reasoning, critical
  web (`GPT-5.6 Sol`, explicitly verified very-high reasoning) approved `fe229e9`, `242caf2`,
  `11af535`, exact `d965dce..11af535`, cumulative `aa77058..11af535`, pushed HEAD `11af535`, and
  related repository-wide owners as `APPROVE C0/H0/M0/L0`. Daybreaker Blue High remained unavailable
  until 2026-09-06, so the same session performed the authorized security cross-review. The review
  used public immutable source only and no automated scan, provider, packaged app, or Computer Use.
- Finding: the first real Terra turn exposed one reachable copied-policy conflict. The persistence
  observation still required exactly `publish_message` or `decline_to_speak`, while the transport
  already owned those terminal actions plus the four vote actions. Terra therefore refused an
  explicit `create_vote` request and committed no vote. `68ee8cc` removed that stale terminal subset;
  persistence now requires one terminal action exposed by the room transport and does not duplicate
  its catalog. This adds no state, fallback, retry, polling, heartbeat, timer, cache, or background
  task and preserves the same exact-turn, authority, transaction, release, and publication owners.
- Verification result: the corrected isolated macOS package started a real `gpt-5.6-terra` Codex
  app-server session from the copied UI. A UI request produced one empty-body `message_kind=vote`
  event with the exact question and two options, one current-vote projection row, and a completed
  provider turn with no agent text publication. The same package had already passed human create,
  cast, replacement, withdrawal, close, normal restart, and persisted-summary flows. `make verify`
  then passed all mandatory gates, the 104-file/644-test frontend suite, 26 desktop, 52 domain,
  228 persistence, 6 protocol, 152 provider, and 94 server tests plus TCP/integration boundaries and
  warning-denied Clippy. A real Antigravity `gemini-3.6-flash` native PTY session then cast `YES` on
  that poll: the exact Agent Session owned the sole ballot, the tally became `[1,0]`, its turn
  completed, and no transcript, printed text, or agent message was published. The installed OpenCode
  1.17.18 catalog no longer exposes `opencode/hy3-free`; its only current Hy3 entry is the nonzero-cost
  `opencode-go/hy3`. The copied selector therefore rejected that retired identifier before session
  creation. The paid Hy3, a direct unlisted start, and every fallback remained unused.
- Verification result: under the user's replacement choice, the same copied selector resolved exact
  `opencode/muse-spark-1.2-contributor-free`. The live catalog reported it active with zero input,
  output, and cache cost and tool-call support; it exposed no `opencode`-namespace Grok model. An
  isolated package then started the exact Muse Spark Agent Session through OpenCode's persistent
  loopback server. One addressed turn called the common `cast_vote` terminal action with `NO`, wrote
  one empty-body agent-owned `vote_cast`, completed without an agent text publication, and changed
  the canonical projection to `[1,1]`. The stored session retained the exact model, provider session,
  executable and workspace identities, `meeting_read_only` workspace permission, idle runtime, and
  completed generation; room vote authority remained with its joined Agent Session participant.
- Packaged remote result: the managed tunnel issued separate one-use read/write and read-only
  invites, each consumed exactly once. The writable browser created a finite poll, cast, replaced,
  withdrew, and creator-closed it while the host updated from sequenced events. A fresh read-only
  browser saw both polls, had composer and ballot controls disabled, received the host's live tally
  change, and restored the exact tally after reload. Normal application restart restored the open
  poll, `[1,0]` tally, and the host's own choice before Muse Spark added the second ballot. SQLite
  independently recorded the two invite scopes, two admitted sessions, completed provider turn,
  empty-body vote event, and current ballots; no client-owned or provider-specific tally existed.
- Resource and cleanup result: the Muse Spark turn took 10.484 seconds from durable input event to
  durable ballot event. An idle process snapshot measured 0.0% CPU for both Rust servers, provider
  guardian/anchor, and OpenCode serve; the desktop renderer measured 0.7%. OpenCode serve's physical
  footprint was 401.0 MiB at observation and 833.8 MiB peak. This is observed upstream persistent-
  session cost, not an inferred Rust leak; replacing the process with per-turn startup would change
  the current provider-session contract and was not introduced as a speculative optimization. The
  isolated database was 480 KiB and the provider workspace remained empty. Normal quit removed the
  desktop, both servers, guardian, anchor, and OpenCode serve; the 1.1 GiB package, app data, caches,
  WebKit data, workspace, and generated sidecar were then removed by exact path. No polling,
  heartbeat, retry, silent failure, fallback, cache, timer, compatibility path, or background task
  was added for this verification.
- Final approval: critical web (`GPT-5.6 Sol`, verified very-high reasoning) approved `68ee8cc`, exact
  `d158c29..68ee8cc`, cumulative `aa77058..68ee8cc`, and the repository-wide terminal-action owner as
  `APPROVE C0/H0/M0/L0`, with no actionable finding. Daybreaker Blue High remained unavailable until
  2026-09-06, so the same session performed the authorized security cross-review. It used public
  immutable source only and did not rerun the provided packaged-app or `make verify` evidence.
- Later catch-up finding: Daybreaker Blue High traced a state race that the earlier review missed. A
  provider could stage a valid cast, withdrawal, or close while the poll was open, but a human could
  close, delete, or expire the poll before the retained result reached persistence. The vote
  transaction then returned a deterministic current-state rejection, while the server retained the
  same provider result for one-second reconciliation and retried it without a terminal state change.
  This could stop ordered progression and repeat SQLite and log work indefinitely.
- Correction intent and owner: the vote owner now classifies only `vote_not_found`, `vote_expired`,
  `vote_closed`, `invalid_vote_choice`, and provider-close `permission_denied` as deterministic
  terminal action rejections. The same transaction marks the exact provider execution failed, emits
  an internal error and `turn_finished`, advances the Agent Session to attached/idle without
  requeuing or reinvoking its consumed input, and assigns any independent pending work. Storage,
  corrupt-state, stale-turn, and uncertain-effect failures remain errors and are not converted into
  success or a retry fallback. A committed terminal result is released by the existing server owner.
- Preserved contracts and verification: vote authority, projection/event atomicity, provider-session
  identity, input cursor, ordered/ambient floor ownership, and post-commit publication ordering are
  unchanged. The focused closed-during-cast test proved one `error`/`turn_finished`/session-state
  commit, failed execution phase, attached/idle session, no recovery flag, no next reconciliation
  candidate, and no ballot event. Both focused provider-vote tests and warning-denied persistence and
  server Clippy passed. No polling, heartbeat, timer, fallback, compatibility path, background task,
  or new retry was added.
