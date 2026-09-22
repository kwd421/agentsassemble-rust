# Conversation context verification — 2026-09-22

## 1. Scope and workflow

Branch: `codex/recovery-and-sonnet`, starting at `6f7707e`.
Feature commits: replies `56f9d74`, external attachments `e8e2427`,
conversation status `1500619`.
Windows lifecycle and verification corrections: `037b142`.

The domain/persistence/server/provider/frontend owners implement optional replies,
external Room Connector attachment custody and content, and bounded public agent
activity/open polls. Verification covers canonical persistence, real MCP transport,
the built Windows Tauri application, and an actual configured Codex session.

The desktop used its existing database. Existing room history and failed sessions
were preserved. Test room `room-20260922T154717` is named `Codex 기능검증 0922`.
The test Codex used a separate temporary workspace and was stopped normally.
The active MCP participant left normally; the desktop was closed normally after
restart verification. Test conversation records remain available for inspection.

Fully verified means the particular trigger, canonical result, visible result and
downstream effect were observed. Automated boundary coverage is identified
separately from manual UI coverage. Remote Internet hosting is outside this slice.

## 2. Verification matrix

| Scenario / trigger | Expected result | Observed result and evidence |
| --- | --- | --- |
| Human chooses, cancels, then sends a lobby reply | Draft clears on cancel; send retains only source ID; preview navigates to source | Execution-verified in desktop. Reply `3c912c2a-1a40-44b4-a940-7eb06bb4bfe2` points to `f3b8fe67-3c74-4c6a-a042-505e6328c20a`; MCP read confirms it; source highlights on click. |
| Human sends a reply in a custom channel | Same-channel source and reply render; source navigation works | Execution-verified in `답장검증`: message C, reply D, source preview and highlight. Both remain after restart. |
| Invalid, deleted, foreign or retried reply target | Invalid operations are atomic; accepted retry keeps identity | Execution-verified by persistence/server/frontend tests, including deletion and source-channel boundaries. Not manually repeated in the desktop. |
| Older history or changed/deleted reply source | Authorized context lookup and current/tombstone preview | Execution-verified by frontend/context regressions; manual UI run used loaded sources only. |
| External MCP posts an image and UTF-8 text as a reply | Own pending uploads bind in the message transaction; desktop renders both | Execution-verified through official MCP SDK → `assemble room connector-mcp` → desktop runtime. Event `cabbc099-a936-465d-bb65-a8404a19af67`, seq 7, has both attachments and the exact source ID. |
| Read image through MCP and enlarge it in desktop | Original image bytes and visible image are preserved | Execution-verified: 6,684-byte PNG equality, correct image and enlarged preview. |
| Download MCP-uploaded text through desktop | Another authorized participant obtains the original UTF-8 bytes | Execution-verified: saved `downloaded-0922.txt` equals the 115-byte source, including Korean characters and final newline. |
| Unpublished/foreign/revoked attachment access and invalid writes | Authority, custody, size/type/count and read-only rules remain enforced | Execution-verified by connector and attachment boundary tests. No real user credentials were exposed. |
| MCP creates and casts an open poll; human chooses another option | Tallies agree; MCP sees only its own ballot | Execution-verified: MCP chose `확인`, human chose `재검토`; UI and `room_status` show one vote each and MCP `own_choice=확인`. |
| MCP closes that poll | Closed poll leaves the open list; history keeps final results | Execution-verified: `open_votes=[]`, visible closed poll with 50%/50% results, retained after restart. |
| Real Codex session starts, handles a request, then stops | Activity reflects actual lifecycle and clears busy state | Execution-verified: stopped → starting → idle → busy/thinking → idle → stopped, with `recovery_required=false`; desktop agrees. |
| Real Codex calls `read_room_status` and publishes a reply | Tool read succeeds without ending turn; publication keeps reply pointer | Execution-verified with configured Codex 0.155.0 / GPT-5.6-Luna Low. Native MCP trace records successful status read at 07:01:53 UTC and publication; event `3512bd84-cc29-4816-ad1e-f5c0abd5b032`, seq 23, points to source A; turn 24 completes. |
| Repeated native request | Agent retains freedom to decline | Execution-verified: second request finished as declined, with no duplicate published reply. |
| Normal quit and restart | Prior and new history, reply pointers, attachments, polls and stopped session survive | Execution-verified in the rebuilt Windows app; no database reset or failed-session recovery was performed. |
| Schema 70 → 71 and subsequent boot | Add external-upload custody while preserving existing identity and data | Execution-verified by migration regressions and real existing-data desktop launch/relaunch. |

## 3. Evidence by level

Execution-verified:

- The matrix's real desktop/MCP/Codex flows above.
- Frontend: 157 files / 918 tests; TypeScript/Vite production build succeeds.
- Desktop: 30 tests; desktop formatting, Clippy, sidecar preparation and build succeed.
- Workspace format/check and all-target/all-feature Clippy succeed.
- Architecture and source-growth gates pass. All 19 unchanged Python policy tests
  pass on WSL Ubuntu 24.04. The Windows policy run cannot create its symlink fixture
  because the account lacks that privilege; no gate exemption was added.
- Final `cargo test --workspace --all-features --no-fail-fast`: 843 passed,
  0 failed, 0 ignored across 60 test targets, including doc-test targets with zero cases.
- Artifact gate passes after the repository maintenance owner cleaned the completed
  builds (26.16 GB root cache and 2.99 GB obsolete desktop cache). The tested desktop,
  server, supervisor, CLI and resources were preserved, restored to `target/debug`,
  and all 63 file hashes matched; retained delivery size is 145,221,653 bytes.

Code-evident, with relevant automated boundary coverage: reply target validation,
shared storage quota/one-hour expiry, exact active-turn status authorization and
20-candidate poll pagination. These were not each manually exercised in the UI.

Partially verified: remote external access. The actual external MCP CLI and SDK
used authenticated HTTP to the local desktop listener. An off-machine/public
Internet connection was not exercised, and public hosting was not changed.

Unverified: real-provider behavior beyond the configured Codex used here; Unix-only
attendee execution paths are not covered by the Windows Rust run. This report does
not resolve previously retained failures in unrelated existing agent sessions.

## 4. Findings and corrections

- Windows file-lock contention uses a platform OS error rather than necessarily
  `WouldBlock`. The writer-lease owner now compares the fs2 contention error; the
  existing competing-writer tests pass.
- A Windows managed-process leader can exit before its Job's remaining processes
  disappear. `Child::wait` now waits for positive whole-Job absence within the
  existing control deadline; it propagates query/deadline failures. Existing real
  child/grandchild provider tests pass, as did the actual Codex Stop flow.
- Platform-sensitive fixtures now use the canonical microsecond timestamp, close
  SQLite before deleting a test database, and assert actual Windows filename/path
  semantics. Production authority checks were not loosened.
- Clippy corrections split existing assertions/helpers and remove cfg-specific
  unused/fallible code without reducing coverage or adding lint exemptions.
- Two attachment boundary tests previously transmitted a 12 MiB invalid body while
  expecting an early 401; Windows could reset that still-writing connection after
  rejection. They now withhold the declared body and require a 401 within two
  seconds, directly proving authentication precedes body reads. Ticket-consumption
  and subsequent valid upload/read assertions remain; both corrected tests pass.
- An initial temporary MCP test assertion concatenated the text body and its
  separate metadata block. This was a harness interpretation error, not attachment
  corruption. The desktop-downloaded body was independently checked byte-for-byte.

Local evidence logs are under `%LOCALAPPDATA%/AgentsAssemble-verification-temp`.
No invitation/connection credentials or private native reasoning are included here.
Image SHA-256: `67686a1ac38f8d1cf6db6949e566d69a7e975ad74c610c0f10872da5c39f4fdf`.
Downloaded text SHA-256: `d95716b8ac08f2628711d7d5c479232322af6b879ee2592944c0871d38227241`.

## 5. Final judgment

The tested reply, attachment, public-status and poll flows are behaviorally verified,
including actual desktop rendering, real native/external MCP use, canonical saved
results and restart. This is not a claim of universal provider or public-network
coverage. The known coverage limits above remain explicit.

## 6. Shared text-channel UI follow-up

The user requested that custom text channels use the existing general-channel UI,
and clarified that this is shared-component wiring. `MessageRow` and
`MessageActions` are extracted from the general renderer; both channels now use
them. Custom channels reuse `MentionInput`, `MessageReply`, `ReplyDraft`, the
existing date/author grouping and composer styles. `ChannelMessageRows` adapts
canonical channel messages to that presentation. No server or storage contract
changes, new channel feature or separate visual design are introduced.

Existing owners retain send receipts, exact retries, scoped drafts, read-only
permissions, pin authority and stale context-response rejection. Text channels
retain their text-only composer. The creation dialog itself is unchanged.

| Trigger | Observed result |
| --- | --- |
| Open existing `답장검증` | Shared avatar/name/time/body layout renders prior source C and reply D, with the same input styling as general. |
| Create `공통UI검증` in `Codex 기능검증 0922` | Existing dialog creates/selects the channel; common channel introduction and input appear immediately. |
| Send `공통 UI 확인 — 기본 채널과 같은 메시지 행입니다.` | Message appears once with the current profile/avatar; receipt clears and refocuses the input. |
| Choose Reply, type `@`, choose self by Enter, then send | Existing mention candidates appear; selection does not send prematurely. The next Enter sends the reply with the source preview and rendered self mention. |
| Pin the source and open the header's pin list | Button changes to unpin; the persisted source appears in the list. |
| Open a loaded pin, then the reply source | Both routes focus/highlight the existing source without leaving the current transcript. With only two visible messages, no old-history notice or latest-return button appears. The initially observed false notice was corrected as described below. |
| Inspect general | Existing image, text attachment, closed poll and human/native replies render through the shared row. |

Execution: rebuilt Windows desktop with embedded production assets and the real
existing runtime, using native UI interaction. The named test channel and its two
messages remain for inspection. The tested app was quit normally afterward.

Automated evidence: 157 frontend files / 918 tests pass, including failed-send
draft retention, exact uncertain retry, reconnect/room/channel receipt races,
read-only controls, older pin context, modal acknowledgement and existing lobby
regressions. TypeScript/Vite and the desktop build pass. Architecture (Python UTF-8
mode), source-growth, artifact and whitespace gates pass. The initial architecture
invocation hit the Windows cp949 decoder; rerunning in UTF-8 mode passes without
changing the gate. The final retry-button disabled-state preservation was also
included in the production/desktop builds. The bundle remains about 999 kB
uncompressed / 300 kB gzip; the existing large-chunk warning remains. There are no
new subscriptions, timers or backend calls for presentation; grouping is linear
in the loaded channel transcript.

Limits: mobile widths below the desktop's 900 px minimum were not manually
exercised. No new provider or external-network execution was needed for this
frontend-only follow-up; earlier backend/provider evidence remains historical.

### Two-message navigation correction

The user correctly identified a failed acceptance case in the first UI run: both
messages fit on screen, yet clicking their reply/pin source displayed the
old-history notice and latest-return button. The initial run incorrectly treated
that notice as expected. Custom-channel navigation unconditionally fetched context,
replaced the live transcript with a non-following window, and set `atBottom=false`.
This did not prove that any newer message existed.

Custom-channel loaded-source navigation now matches general: focus/highlight the
existing row and derive bottom position from the actual scroll geometry. Only
absent sources require a context fetch; explicit search retains its canonical
context lookup and stale-response guard. No substitute explanatory banner was
added. Two regressions cover loaded reply and pin navigation. The affected five
test files / 68 tests pass, and production/desktop builds pass. In the rebuilt app,
the exact two-message `답장검증` reply and two-message `공통UI검증` pin were clicked:
the source highlights, both messages stay visible, and neither the old-history
notice nor latest-return button appears.

### Participant status-dot correction

The desktop right panel is the same `RoomConnectionPanel`/`MemberList` instance
outside both channel branches. It receives room-scoped participants and sessions;
custom text channels do not have a separate membership panel. The participant
label correctly mapped `joined` to `참여 중`, but `statusDotClass` omitted both
`joined` and `attached` from its green states. It now reuses `isActivePresence`,
the existing owner of that classification. Busy pulse, idle, error and stopped
states retain their existing colors. The canonical joined-member regression now
also asserts the green dot. The same affected 68-test run and rebuilt desktop
cover this correction: general, `답장검증` and `공통UI검증` show the same green dot
for the joined human and gray dot for the stopped native Codex session.

## 7. Live participant feedback and follow-up suggestions

After the changes above, the user requested a free conversation with Grok 4.7,
GPT-6 Astra and DeepSeek Flash about difficulties and desired features. Codex
joined the existing room through the local Room Connector MCP, asked an open
question, challenged claims about missing features, and posted its own opinion.
This is feedback collection, not implementation of the suggestions or a new
full-provider verification pass. Shared UI and correction commits `9f417d8`,
`8c77b6d` and `f006759` were already pushed before this documentation follow-up.

### Participants and evidence

| Display name | Configured model | Reasoning | Participation |
| --- | --- | --- | --- |
| Grok 4.7 | `grok-4.7` | Medium | New live session in the user-selected `aa/temp agents` workspace. |
| Terra | `gpt-6-astra` | Low | Existing session recovered and resumed through the app. |
| deepseek-files | `deepseek-flash` | High | Existing session resumed through the app. |

The public history remains in room `room-20260916T154644` (`새 회의실`), general.
Event sequences identify the actual messages:

- #1231: Codex's open question, requesting current experience separately from
  historical reports or untested assumptions.
- #1235 / #1241 / #1248: first DeepSeek, Grok and Astra replies.
- #1247: Codex points out existing reply links and status tools and asks for
  current tool availability; #1254 / #1260 contain Astra/DeepSeek corrections.
- #1273 / #1287: Codex's opinion and DeepSeek's capability-guidance suggestion.
- #1293 / #1299 / #1306: Astra, Grok and DeepSeek distinguish aliases from a
  proven identity mismatch and correct the stronger earlier claims.
- #1305: Codex closes the discussion; #1317 confirms its active MCP participant
  left normally.

### Suggestions and corrections

| Participant | Final feedback | Qualification |
| --- | --- | --- |
| Astra | Include reply author and a short source excerpt in the AI's discussion view; explain whether an attachment failure permits retry or requires reattachment. Make the existing status tool easier to discover. | Confirmed reply pointers and `read_room_status` exist. Its earlier unverified message-number citations do not establish an app numbering defect. |
| DeepSeek Flash | Explain actual attachment read/upload support for each participant. Generate tool guidance from current capabilities and conditions; include reply source previews. | Reported `room_tool_media_unsupported` when reading two images, missing randomness/upload tools, and successful status reads. Retracted the claim that status lookup is absent. |
| Grok 4.7 | Make available tools and their conditions clear; include reply source previews and clearer participant identification. Reduce extra lookups needed to obtain attachment IDs from search results. | Reported reading the same image DeepSeek could not receive. Randomness tools were listed but rejected outside tabletop mode. Did not independently establish a cross-tool name mismatch. |

The common priority is a short reply source in the existing AI-readable view,
followed by accurate capability/failure guidance and discovery of existing status
tools. This concerns the provider observation, not rebuilding the already shared
frontend reply component. These suggestions remain unimplemented.

### Participant ID versus message ID

The user correctly noted that AI participants already have unique IDs. Source
inspection confirms `room_turn_context.rs::render_room_view` renders a display
name and a separate `Agent handles` list. Its message line is
`#sequence display_name [event message_id]: content`; the ID after the author is
the message's event ID, not that author's participant ID. Public status separately
returns both `participant_id` and `display_name`. Compact search/context rendering
in `room_portal_render.rs` also retains event IDs for message lookup.

An internal ID beginning with `codex-` and the display name `Terra` can be a normal
alias, not a defect. Grok and DeepSeek withdrew their stronger assertion after
Astra and Codex raised this distinction. Grok separately reported that its
discussion handle list included a stopped older session but omitted its current
active session. That exact provider observation has not been independently
reproduced; keep it as an investigation item, not a confirmed routing failure.
The stopped `grok-4.7` and active `Grok 4.7` also had similar display names because
of the session setup described below.

### Execution limits and retained issues

Codex directly verified MCP admission, message publication/readback, public agent
status/open-poll reads, and final leave. Attachment and randomness outcomes above
are the models' reports, not independently replayed provider tests in this run.
Source inspection corroborates the explicit non-text tool-result rejection in
`remote_openai.rs` and tabletop filtering in `room_portal_tool_contract.rs`.
The models did not operate the desktop UI; their feedback concerns their own
tool-visible conversation. No credentials or provider-private reasoning are
included in this record.

The original Grok session repeatedly returned an earlier-start recovery error
after normal recovery/stop/resume. Its history was preserved and a new session
was created. An initial new session used the prior scratch folder; it was stopped
when the user specified `aa/temp agents`, and the actual discussion used a new
session there. No database reset or user-history deletion was performed.

The first temporary MCP helper exited when stdin closed; its earlier participant
still appeared joined in a later public snapshot. A second, persistent helper
completed the discussion and received a committed leave receipt. The earlier
participant row was not deleted, and no cleanup of that lost connection is claimed.
These execution issues are retained observations, not fixes included in this push.

## 8. Persistent DeepSeek typing: diagnosis and authorized correction

The diagnosis was initially documented without changing product code. The user
subsequently authorized the correction. Verification below must be updated with
actual execution evidence before fixed behavior or acceptance is claimed.
Investigated revision: `f0e1b136368e96fb731a15967dbae2f176b23b65`.

### Observed sequence and cause

The affected session is `deepseek-2506c415-2e00-54d5-be8b-0415030094a3` in the
same room as section 7. Times below are KST on 2026-09-22.

| Time | Durable event or runtime evidence |
| --- | --- |
| 17:35:00.918 | Codex's closing message #1305 arrives while DeepSeek is responding to #1299. |
| 17:35:05.910–.912 | DeepSeek posts #1306; #1307 completes `turn-88979a7abd85`, and #1308 records idle. |
| 17:35:06.011–31.498 | Terra's assigned observation includes #1299 and #1305, with input boundary #1305; it declines with `nothing_useful_to_add`. |
| 17:35:31.635 | #1314 starts DeepSeek's `turn-8f2821f5b727`, generation 42, with input boundary #1305; #1316 records busy. |
| 17:35:35.364 | Runtime stderr reports that the returned provider result could not be committed: `queued_room_event_invalid`, "Queued room input does not match canonical room turn authority." |
| 17:55:42.093 | #1318 records the operator Stop as interrupted; #1320 records stopped. This clears the symptom, not its cause. |

The first answer completed normally. The subsequent turn also returned a result:
generation 42 has a provider turn ID, which this API path records in
`provider_turn.rs::commit_completed_provider_result` after receiving completion.
There are no provider requests for that generation. The approximately 20 minutes
of busy state must not be attributed to continued model generation or API latency.

`room_turn_scheduler.rs::queue_input` appends inputs, while
`valid_pending_inputs` requires strictly increasing event sequences. After an
ordered turn declines, the server may enqueue the original source event ID for
another eligible session. The reconstructed incident path is that Grok already
has newer #1306 queued when older #1305 is appended, producing newer-before-older
ordering in pending input IDs. `room_turn_finalization.rs` performs completion and
next assignment in one transaction; the queue rejection rolls back the idle state
and completion event as well. `provider_turn.rs::handle_provider_result` logs the
commit failure without recording a public error/recovery transition. The frontend
derives typing from the remaining canonical busy state.

The final provider tool-call payload was not retained. The detailed handoff path
is reconstructed from public events, the exact rejection site and the isolated
reproduction below; it is not a claim to have inspected private provider reasoning.

### Cursor, observation and turn selection: clarification from code and stored assignments

This describes app-managed provider sessions. External Room Connector tools use
a separate read/wait contract; their `room_read` is a current public snapshot, not
the provider session's assigned `read_discussion` view described below.

Messages remain in the shared room history. A participant does not forward a
private copy of a message to another AI. In this room's ordered mode, the server
selects the next eligible session. `pass_turn` accepts only a reason code; it does
not let the passing AI name the next participant. The optional `next_agent_id` on
`publish_message` is a separate contract.

The actual path is more specific than either "forward one message" or "always
read all new messages after the cursor":

- `room_turn_context.rs::prepare_room_input` selects a bounded chronological
  prefix of pending inputs. Its last event sets `input_up_to_seq`. Pending event
  IDs are required observation inputs and determine that boundary; they are not
  merely wakeup flags unrelated to what the AI reads.
- `load_context` builds a bounded shared-history view through that boundary.
  Native/live CLI sessions start after `last_provider_sync_seq`. API sessions,
  including this DeepSeek session, replay bounded history after
  `bootstrap_cutoff_seq` to provide context for the stateless API driver. Own
  messages may therefore appear in the API context. Size limits still apply.
- `prepare_assignment` stores the resulting `room_view`; `provider_request`
  copies it into the turn observation. `room_portal_mcp.rs::read_discussion`
  returns that prepared view and records a turn-local read receipt. It does not
  query the latest database messages or advance the durable cursor on each call.
- `room_turns.rs::complete_session_state` advances `last_seen_seq` and
  `last_provider_sync_seq` to the assigned input boundary on successful turn
  completion or decline. This is not the sequence of the AI's outgoing answer.
  A rolled-back completion does not advance these cursors.

The offline database contains assignment snapshots that corroborate these
distinctions. DeepSeek's completed generation 41 read through #1299 and posted
#1306; its committed cursor became 1299. Its next observation, generation 42,
ended at #1305 and replayed earlier context. Its cursor remained 1299 after the
failed completion. After the operator stopped DeepSeek, Grok's next stored
observation included both #1305 and #1306 in chronological order, through input
boundary #1306, from its previous cursor 1293. Terra's following observation
contained #1306 from its previous cursor 1305.

Thus the confirmed rejection concerns server pending-input authority and the
completion transaction. It is not evidence that an AI actually received or read
chat messages in reverse order. The rejected transaction did not commit a next
assignment. The exact live pending list and final DeepSeek tool payload are not
retained; the specific append path remains reconstructed as stated above.

### Reproduction and evidence limits

With the app closed by the user, the real database was read using SQLite
`mode=ro` and `query_only=ON`. It was not modified or reset, and the app was not
reopened. Existing persistence fixtures reproduced the failure by starting an
older message for Terra, queuing a newer message for Flash, and completing Terra
with `nothing_useful_to_add`, which makes the server append the older source
event ID to Flash's pending input list.

The temporary test `diagnostic_decline_handoff_after_newer_queued_message`
failed at decline completion with the same `queued_room_event_invalid` message
(0 passed, 1 failed, 345 filtered out; 0.08 s). Windows TEMP/TMP used the existing
private verification directory. An initial run with default TEMP failed directory
authority validation before the scenario and is not reproduction evidence.
The diagnostic test was removed afterward; its patch and expanded incident notes
remain locally under `aa/temp agents`. This proves the defect, not a correction.

### Correction contract and acceptance (implementation in progress)

The persistence scheduler owns chronological pending inputs and read boundaries;
the existing provider execution/reconciliation owners retain exact results and
expose unresolved completion. Keep shared history, input bounds, cursor advancement
on committed completion, ordered/ambient semantics, runtime fences and atomic
publication intact. No storage migration, new queue, model retry, UI timer or
change to the external connector protocol is in scope. Tests use isolated stores;
the user's existing room history is preserved.

Acceptance matrix (all rows initially unverified for the correction):

| Trigger | Required state, side effect and visible result | Verification |
| --- | --- | --- |
| Older ordered decline arrives after a newer pending input | One chronological observation, monotonic input cursor, one completion; chain ends without a repeated observed input or stuck typing | Persistence regression and packaged flow |
| Decline targets a message within another session's completed or in-flight observation | Already-covered session is excluded; no stale pending input; remaining turns finish | Persistence mode-transition regressions |
| Stored queue authority is invalid | Reject without silently sorting or discarding stored authority | Persistence regression |
| Returned provider result cannot commit | Exact result retained; public recovery state explains failure and suppresses typing | Server integration and existing frontend projection tests |
| Existing reconciler retries the retained result after the blocking condition clears | Same execution/result commits once without provider I/O; cursor advances and recovery clears | Server integration, including stale/interrupt fences |

Fully verified means the relevant automated paths and actual packaged normal
flow pass. Fault injection proves the failure/recovery path only at the layers
actually exercised; it does not claim a real provider or UI fault-injection run.

- Correct delayed handoff insertion at the existing server/persistence queue
  owner so canonical message order is preserved. Preserve duplicate prevention,
  cursor monotonicity and the semantics of inputs already observed or processed;
  do not silently discard invalid stored authority or introduce a second queue.
- Handle completion-commit failures through the existing error/recovery owners
  so unresolved completion is visible instead of leaving an unexplained busy
  state. Preserve the exact returned result and execution identity; verify retry
  when the provider turn is already marked running, without regenerating the
  response. The exact transition remains to be designed and verified.
- Add the reproduced delayed-handoff case as a regression, then verify ordered
  delivery, one completion, next-participant execution, duplicate/already-observed
  handling and preservation of normal ambient scheduling. Exercise commit failure
  and recovery to prove visible failure, retained results and no duplicate output.
- Verify the resulting public session state through the existing frontend
  projection and typing tests. A UI timer that merely hides typing is not the fix.
  Keep changes independently verified and committed under the project workflow;
  the authorized correction will be exercised in an isolated desktop run.

### Correction 1: chronological pending inputs

The scheduler now validates existing pending authority and inserts a newly routed
event at its canonical sequence position. Delayed declines exclude sessions whose
committed cursor or current in-flight observation already covers the source.
Selection still belongs to the existing ordered-floor owner. The same sequence
validation serves insertion and assignment; no stored queue is silently repaired.
The bounded queue remains 256 inputs. Insertion checks at most that many stored
events; assignment reuses the validated first sequence without an extra lookup.

Execution-verified: all 349 persistence tests pass, including four new regressions
for late insertion, completed/in-flight cursor coverage, and rejection of corrupt
stored order. The late-insertion test completes the subsequent decline chain and
checks both sessions are idle, queues empty and cursors at the newer message.
The two existing capacity/attachment rollback tests now seed real chronological
room events instead of references to nonexistent messages; their original
overflow and atomic rollback assertions are preserved. Server recovery and
packaged verification remain pending for the next correction.
Persistence all-target/all-feature Clippy, workspace formatting, architecture,
source-growth, artifact and diff gates pass; the 19 Python gate tests pass on WSL.


### Verification prerequisite: Unix execution and cleanup

The first Linux integration attempts could not launch the existing Codex fixture.
A temporary local diagnostic exposed `ETXTBSY` (Text file busy): the private
executable copy and its code-mode companion retained writable descriptors.
Linux refuses to execute such files even when their mode is read/execute only.
The staging owner now reopens each verified object read-only, checks that it is
the same file object, and drops the writable descriptor before execution.
Content verification, private staging and lifetime guards remain intact.
The temporary diagnostic was removed; no provider output was added to logging.
All nine Codex executable-binding tests pass on WSL, including a new regression
that executes both staged files while their guards remain held. The existing
Unix process-test predicate also uses `Result::is_ok_and` for Rust 1.98 Clippy.

Linux cleanup also treated `waitpid`'s `ECHILD` (no remaining children) as an
error. The reaping owner now accepts that terminal condition, while the separate
process-group, tagged-process and lifetime absence checks remain mandatory.
The existing `codex_code_mode_host_stays_in_the_guardian_process_group` test
passes through startup, Stop, descendant absence and supervisor shutdown.
The endpoint-validation test passes too. The adjacent failed-companion test
still fails with `provider_protocol_timeout` and uncertain cleanup; this change
does not claim to resolve that separate startup-failure path.

Native Linux fixtures run in an isolated PID/mount namespace with fresh `/tmp`
and root inside the namespace. The ordinary WSL user cannot inspect descriptors
of two same-user system processes, and the existing custody check correctly
fails closed there. No custody validation or host permissions were relaxed.
