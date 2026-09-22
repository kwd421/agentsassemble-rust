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
| Open the pin, return to latest, then open the reply source | Both routes retrieve the channel context, focus/highlight the source and expose the shared latest-history return control. |
| Reopen general | Existing image, text attachment, closed poll and human/native replies render through the shared row. |

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
