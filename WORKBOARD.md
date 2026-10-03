# WORKBOARD

## Active work

- 2026-10-03 Discord-style frontend pass 2 (user request, app and web): shared
  secondary buttons get Discord sizing/colour with layered defaults; the web login
  is one primary Google action; the server chooser uses icon/name/status rows,
  an icon refresh and a text logout; people rows drop the type line and agents
  without a model show status inline; the rail separates home; the lobby composer
  says `#general에 메시지 보내기`; friends header actions become icon buttons;
  room-mode copy becomes name plus caption; add-agent labels are Korean. Labels
  and actions used by tests are unchanged. Frontend 964, build, architecture and
  diff gates pass. Web login is verified on the real server; chooser, members,
  friends and room settings are verified in a stubbed component preview against
  a HEAD worktree. Native packaged and signed-in web room views remain unverified
  (app access not granted during this run). Evidence: `docs/VERIFICATION.md` ->
  Discord-style frontend pass 2.

- 2026-10-03 shared deletion recovery correction: server-wide web ownership now
  retains transactional grant provenance through the existing bounded command
  queue and immutable deletion replay. The shared shell exposes the exact pending
  target and explicit retry even with no rooms; directory absence is not success.
  HTTP8, persistence358, frontend964, Clippy and mandatory gates pass. Signed
  Mac/Chrome last-room retirement, exact completion recovery, restored-session
  re-entry and network failure/fresh authorized reconnect pass. The two macOS rows
  are the installed app (0e2827be) and isolated verification app (ec77a196); exact
  store IDs match. Ingress/processes are closed and isolated data/bundle moved to
  Trash; central rows and primary data are preserved. Windows and overall parity
  remain open. Contract: room-lifecycle slice -> Shared
  owner recovery after room retirement.

- 2026-10-03 server-wide owner account correction: profiles, bounded avatars and
  saved friends reuse the centrally redeemed server-owner grant without room
  admission. Existing authenticated directory invalidation now carries the canonical
  host profile revision, including zero rooms. HTTP9, persistence358, frontend962,
  Clippy and mandatory gates pass. Signed Mac/Chrome empty-server read/edit, live
  profile changes both directions, first-room creation and reload pass; originals
  are restored. Deletion progress/recovery and actual network reconnect pass in
  the following correction; Windows and overall parity remain open. Diagnose the two macOS registrations afterward,
  without deleting them. Evidence: `docs/VERIFICATION.md` -> Server-wide owner
  profile and friends without rooms.

- 2026-10-03 invitation transport correction: preserve Claude's shared dialog;
  human, connector, saved-AI-friend and device issuance reuse current room-manager
  transactions with explicit native or central-owner room-session transport.
  Native ingress controls remain host-owned; newly paired devices remain room-only.
  11 HTTP boundaries, 358 persistence checks, 958 frontend checks, Clippy and
  mandatory gates pass. Signed isolated Mac/Chrome issue/copy/revoke and AI packets
  pass, including MCP instructions and identifier input correction; affected 51
  checks pass. Empty-workspace account/friend access and full parity remain open.
  After parity work, diagnose the user's two macOS server rows without
  deleting registrations. Contract/evidence: identity-accounts-friends slice and
  `docs/VERIFICATION.md` -> Shared invitation transport correction.

- 2026-10-03 live owner directory correction: retain Claude's UI baseline; committed
  create/settings/archive/restore/close/delete/cleanup now invalidate a separate
  authenticated owner directory connection, including empty workspaces. Shared
  clients reconcile through the existing verified API without polling or granting
  guest authority. Affected checks, gates and signed isolated Mac startup/create/
  rename pass. Actual Chrome-to-Mac creation and cross-room naming in both directions
  pass beyond 30s after binding the existing authenticated HTTP-wait lifetime.
  Empty/reconnect UI, invitations, Windows and full parity remain open. Evidence:
  `docs/VERIFICATION.md` -> Live owner room directory correction.

- 2026-10-03 Discord-style UI correction (user request): the invite dialog is one
  480px Discord-sized link field with one primary action, collapsed link settings
  and a one-line hosting status; the sidebar hides the default empty banner and
  moves invite beside the room name; rail rooms show initials; the room menu
  anchors below its header; member roles move from per-row selects to the row
  context menu; user settings use a section sidebar and a save bar shown only
  with changes. Invite issue/copy/revoke, tunnel rules, role authority and
  moderation are unchanged. 952 frontend tests, build and architecture gates
  pass; signed isolated Mac package verified visually. Friends view, room
  settings copy and Escape dismissal of menus remain open. Evidence:
  `docs/VERIFICATION.md` -> Discord-style UI correction.

- 2026-10-03 server chooser and friends sizing correction: the native installation
  ID is read without starting/migrating a runtime and matched to the central list.
  The matching row shows "이 기기" and opens locally on explicit selection; the
  separate bottom hosting button is removed. Unregistered installations use a
  local row; inspection failure is explicit and does not block remote entry.
  Original friend-tab padding, sidebar/profile widths, main/empty-panel spacing
  and category order are restored. Signed isolated Mac verification passes the
  chooser, existing room/history entry, compact tabs and add/cancel. Required
  gates, affected checks and desktop Clippy pass. Browser/Windows verification
  and overall parity remain open. The isolated app is retained open for preview;
  public ingress remains off. Evidence: `docs/VERIFICATION.md` -> Isolated remote
  owner workspace.

- 2026-10-03 isolated owner-workspace verification: actual Chrome opens the shared
  room shell and creates a second room; the isolated signed Mac package sees both
  after restart. Missing live directory invalidation remains open. Expiry left the
  owner on an invitation-guest error; the existing connection gate now replaces the
  workspace at the same grant deadline without extending authority. All 952 frontend
  tests, mandatory architecture/source gates, frontend build and signed packaging
  pass. Subsequent native Chrome control verifies room switching, separated message
  histories, reload, friend create/edit/delete, natural expiry and fresh-grant re-entry.
  Chrome repaint after reload/expiry required opening/closing DevTools; cause unknown.
  All temporary public connections are closed and the isolated app exited. The user
  identified a friends presentation regression: the shared view was a simplified
  replacement rather than the original home/category/list/profile composition.
  The shared composition is corrected; signed Mac verification passes category,
  profile selection, create, edit cancel, confirmed delete and return to the room.
  The revised Chrome presentation still needs a fresh authorized ingress run.
  No new scan or subagent.
  Evidence: `docs/VERIFICATION.md` -> Isolated remote owner workspace.

- 2026-10-02 active correction: restore the same product behavior across app and
  web. Separate startup UI and environment-selected account settings do not satisfy
  the user's web-support request. Audit and correct all affected flows, not only
  the oversized Google button. Implementation and behavioral verification remain
  open; prior browser login success does not establish product parity. Acceptance:
  `docs/specs/identity-accounts-friends-slice.md` -> App and web behavior correction.

- 2026-10-02 app/web correction progress: shared startup and standard Google code
  exchange work in Chrome and the signed macOS package. The existing downloaded
  Web OAuth credential is reused; no replacement credential was created. Remote
  owner navigation now targets the existing public `/pair` shell instead of private
  `/app`; real Chrome entry shows the existing room/history. Owner profile settings
  initially remained suppressed by ordinary pairing treatment; the correction and
  real-flow evidence are recorded in the following row.

- 2026-10-02 central owner profile correction: server-issued provenance now permits
  the same owner profile/name/avatar API and shared settings. Ordinary pairing
  sessions remain room-only. Schema 70/71 upgrades preserve existing rows and
  default previous sessions to no owner-profile authority. Chrome profile save,
  reload and Mac member/history projection pass; the original name is restored.
  The additional stale bottom-panel defect is corrected: committed profile revisions
  refresh its authenticated snapshot without overwriting an open editor. Actual
  Chrome-to-Mac and Mac-to-Chrome changes update the bottom panel, member list and
  history without reload. Other acceptance rows remain open; this is not full parity.

- 2026-10-02 owner workspace in progress: the shared friends view now sends explicit
  native or remote owner authority to the same storage operations. Public access
  revalidates central-owner provenance in the transaction; ordinary pairing remains
  denied. Existing server boundaries, 358 persistence tests, focused UI checks,
  build, Clippy and mandatory gates pass. Packaged/browser integration follows
  room directory/create and invitation wiring; no whole-parity completion claimed.

- 2026-10-03 owner directory/create/switch is wired into the shared room shell,
  including the empty-room creation entry. Grant custody is origin/device/generation
  bound in additive schema 73; per-room replay preserves exact expiry. Existing
  central-owner and directory HTTP tests, 358 persistence tests and 46 focused UI
  tests pass. Full UI run exposed one stale room-chooser assertion; the updated
  existing empty-directory handoff/retry test passes. Clippy and mandatory gates
  pass. Signed isolated packaging/real-flow checks are in progress; invitations
  and remaining owner-workspace parity are still open. Installed 0.1.4 data is
  untouched by this isolated verification build.

- 2026-10-02 remaining app/web owner-workspace correction: room directory/create,
  invitations and friends still rely on native manager authority or guest-only
  presentation. Preserve private/native transport boundaries while extending their
  existing server-owned contracts; do not fake authority with frontend flags.
  Chrome account logout/reload and Google retry pass without affecting the Mac.

- 2026-10-02 release continuation: package 0.1.4 for schema 72 and the verified
  shared startup/profile fixes. Keep published 0.1.3 artifacts immutable. The
  installed Mac app is now verified 0.1.4 and opens the existing schema 72 data.
  Windows run 37016132161 passes with 34 native tests. Both signed artifacts and
  the production manifest are published as latest; public bytes match staged
  artifacts, and the installed Mac confirms latest 0.1.4. Windows interactive
  installation remains unverified. Previous app/data backups are retained privately.

- 2026-10-02 official Tauri updater implemented and delivered for macOS/Windows
  in release `desktop-v0.1.4`. Mac signed update, defer/install and latest-version
  checks pass; Windows signed build and native contracts pass, interactive update
  remains unverified. Contract: `docs/specs/desktop-updates.md`; evidence:
  `docs/VERIFICATION.md` -> Shared app/web startup and central owner profiles.

- 2026-10-02 macOS default server names now read the System Configuration
  computer display name instead of a router/DNS-derived hostname. Native identity
  4 and registration TCP 2 tests, Clippy, mandatory gates and signed package build
  pass. The installed app re-registers the same server and shows the corrected
  name after restart; room, history and profile digests are unchanged. Evidence:
  `docs/VERIFICATION.md` -> macOS computer display name.

- 2026-10-02 server OS labels implemented and deployed: both choosers display
  host-reported macOS, Windows, Linux or other OS; missing metadata is explicit.
  Owner-only projection preserves the prior hostname privacy boundary. Affected
  frontend 24, Worker 33, native identity 4 and registration TCP 2 tests pass,
  together with builds and mandatory gates. Signed macOS registration/restart
  visibly retains the macOS badge while offline. Windows/Linux native UI and
  interactive browser display remain unverified; no new external review claimed.
  Evidence: `docs/VERIFICATION.md` -> Central server operating systems.

- 2026-10-02 server naming completed and deployed: actual computer defaults,
  durable owner rename in both choosers, legacy alias correction, cross-owner
  hostname privacy and recoverable name edits. Frontend 952, Worker 32, affected
  native boundaries and mandatory gates pass. Signed macOS naming/restart and
  native failure display pass. Daybreak accepted Rust `3552a7c2` and Worker
  `900b2b9f`, closing all four findings with no new ones. Windows, interactive web
  rename and online cross-device reopening remain unverified. Contract/evidence:
  `docs/specs/identity-accounts-friends-slice.md` and `docs/VERIFICATION.md`
  -> Central server names.

- 2026-10-02 desktop client-first entry implemented: existing account servers appear
  before any local room runtime; hosting requires an explicit choice. Google return
  uses a separate bounded authentication process. Signed macOS Google login, restored
  account, empty guest chooser, explicit local hosting and unchanged database on
  client restart pass. Frontend 949, desktop 36 and affected native boundaries pass,
  as do Clippy and mandatory gates. Existing hosts are offline; actual remote room
  reopening and Windows execution remain unverified. Contract/evidence:
  `docs/specs/identity-accounts-friends-slice.md` and `docs/VERIFICATION.md`
  -> Desktop client-first entry.

- 2026-10-02 browser central Google entry implemented and deployed at the fixed
  identity origin. Real ordinary Chrome login resolves Nel Le and shows four
  existing servers; reload and logout pass. All four hosts are offline, so room
  reopening remains unverified. Private Chrome reaches Google authentication but
  needs user password/passkey completion. Worker 29 and frontend verification pass;
  no new security scan or review session. Contract: `docs/specs/identity-accounts-friends-slice.md`
  -> Browser central Google entry; evidence: `docs/VERIFICATION.md` -> Browser central Google entry.

- 2026-10-02 AI-readable connector invitation implemented: existing `/join` links
  serve setup/join guidance without JavaScript. Connected clients use `room_join`;
  clients without MCP receive registration and session reopening instructions.
  GET/HEAD preserve invitations and room state. Native HTTP/MCP, human entry,
  origin/input boundaries, frontend build and mandatory gates pass. One source owns
  copied and HTTP guidance. Real MCP-free Antigravity reads the link and requests
  registration approval; cancellation produces setup guidance. Approval registers
  MCP; a fresh CLI session with the same link joins, reads, publishes exactly once,
  leaves and releases its receipt. Test registration/settings are restored. Updated
  package and other AI flows remain unverified. Evidence: `docs/VERIFICATION.md` ->
  AI-readable invitation.

- 2026-10-02 requested Sites MCP deployment: existing native MCP cannot be uploaded
  as a Worker artifact. The user clarified the deployment adapter is authorized.
  The private gateway and ownership contract are in `infra/sites-mcp/DEPLOYMENT.md`.
  Local tests and actual isolated-room prepare/join/read/say/wait-cancel/leave/release
  pass; native storage confirms one publication. The private website is published,
  but production MCP returns 404 and Sites reports that MCP is not declared. The
  supported activation contract is missing from the exposed tools and official
  documentation; rejected trial manifest fields are removed. Test upstream and
  isolated host are cleaned up. Completion requires supported Sites activation
  and actual production verification. Evidence: `infra/sites-mcp/DEPLOYMENT.md`.

- 2026-10-02 cross-device central owner reopening correction complete locally:
  the Rust host publishes ready ingress under its durable key, the Worker issues
  and redeems short grants, and the selected host mints origin/device/room-bound
  operator sessions. The completed Daybreak review requested corrections for remote
  WebView reload, challenge retry, TCP boundary coverage, and Worker grant insertion;
  all four are addressed. Worker correction is deployed; affected tests, mandatory
  gates and signed macOS startup verification pass. Actual Mac-to-Windows reopening
  and approval of the corrected revision remain pending. Bookmarks retain
  invite/pairing admission. Contract, review and evidence:
  `docs/specs/identity-accounts-friends-slice.md` -> Central owner server reopening;
  `docs/VERIFICATION.md` -> Central owner reopening review corrections.

- 2026-10-01 follow-up after fast-forward to `67604759`: require live central
  validation before opening an existing local profile, including revoked/missing
  sessions. Distinguish the signed-in account name from the retained local profile
  in settings; preserve explicit profile edits and local room authority. Frontend
  932 tests, callback HTTP boundary, build, mandatory gates and Clippy pass. Signed
  macOS revocation/recovery and actual Google logout/login/restart flows pass;
  room/history/profile digests remain unchanged. Windows UI needs rebuilt-app
  verification. Evidence: `docs/VERIFICATION.md` -> Central startup validation and
  account display.

- 2026-10-01 Windows guest logout -> existing Google login correction completed
  locally: fresh durable account slots and purpose-bound native host claims preserve
  old guest identities, endpoints and local rooms. Worker 21, frontend 927, native
  36 and affected signing/HTTP tests pass; mandatory source gates and Clippy pass.
  Actual signed macOS guest -> logout -> restart -> existing Google login succeeds
  against the deployed Worker with the original room/message retained. Windows UI
  awaits the user's rebuilt-app retry. Evidence: `docs/VERIFICATION.md` ->
  Guest logout to existing Google account.

- 2026-10-01 user-requested Google profile defaults and account logout completed:
  approved central migration and Worker deployment apply first-import name/photo.
  Actual signed package login imports the existing Google account's name/photo;
  custom name, avatar label and photo bytes survive logout/re-login unchanged.
  Settings logout revokes the current central session and persists after restart.
  Rust mandatory gates pass. A clean central-owner checkout containing only the authentication
  patch passes structure and regenerated-map gates; its original working tree
  still contains unrelated shim deletions. Evidence and remaining acceptance:
  `docs/VERIFICATION.md` -> Google profile defaults and account logout.

- 2026-10-01 Cursor follow-up completed: official CLI browser login restores the
  native 43-model catalog. Preserve ACP authentication rejection as login required
  instead of malformed model data. New signed isolated package verifies Auto's
  real room read/publication with once-only approvals, Stop/Resume/Stop and native
  session reuse. Provider 290 tests, wire regression, workspace Clippy and mandatory
  structure/format/artifact gates pass. No provider update or catalog fallback.
  Gemini's English URL still has no custom-MCP or Spark control; Keep Activity is
  already On. Broader Spark availability does not establish custom-MCP eligibility.
  Evidence, scope and cleanup: `docs/verification/2026-10-01-provider-low-reasoning.md`
  -> Cursor and Gemini follow-up.

- 2026-10-01 user-requested minimum-reasoning real-provider verification: new
  isolated macOS room confirms Grok 4.7 Low, Codex GPT-5.6-Terra Low and DeepSeek
  Flash low/Thinking off read and publish correctly, then Stop/Resume/Stop.
  Ordinary external Antigravity CLI 1.2.14 with Gemini 3.8 Flash Low and individual
  MCP approvals joins, reads, publishes, leaves and releases its receipt. Its
  headless MCP permission refusals are separate from the successful user flow.
  Current installed Cursor fails before start at catalog lookup; earlier success
  is historical. Gemini web officially supports custom MCP for eligible US
  personal accounts, but the actual current account has no custom-app controls.
  Evidence and limits: `docs/verification/2026-10-01-provider-low-reasoning.md`.

- 2026-10-01 personal-server MCP preparation: the app-owned listener and public
  ingress now serve `/mcp`; invitation UI derives web setup instructions. Actual
  in-app-browser ChatGPT joins, reads, publishes, leaves and releases its receipt;
  the host UI and stored events independently confirm the result. Correct reproduced
  unauthenticated preparation, 30-second wait disconnect and transport payload
  logging. Connector 17, frontend 923, desktop 36, Clippy and mandatory gates pass;
  final signed-package logging/17-tool smoke passes. Test connections, ingress,
  owned processes and isolated data are cleaned up. Initial ChatGPT registration
  remains manual; persistent endpoint deployment remains unverified. A subsequent
  user-requested repeat creates a different room and a fresh web ChatGPT conversation;
  it reads an undisclosed nonce, publishes it correctly, leaves and releases the
  receipt. The previous room's stored events remain unchanged. Evidence:
  `docs/verification/2026-10-01-web-mcp.md`.

- 2026-09-30 actual external-invite verification: isolated signed macOS package,
  public tunnel and in-app browser verify human admission/avatar/message/attachment/
  reload/leave and consumed/revoked-link rejection. Real Antigravity Connector and
  Grok friend/companion join and publish. Correct reproduced attendee CLI missing
  provider state and historical-attachment relay omission; actual corrected CLI
  and two affected integrations pass, as do Clippy and mandatory source gates.
  All attendee runtimes stop and public ingress closes. Windows execution and
  installed CLI distribution remain unverified. Full evidence and limits:
  `docs/verification/2026-09-30-external-invites.md`.

- 2026-09-30 user-authorized feedback implementation: preserve public MCP
  rejection detail through API results; resolve bounded current reply author/excerpts;
  project search attachment IDs through strict consumers; clarify existing tool
  conditions. No frontend presentation changes or stopped-target routing changes.
  Persistence 356, focused API tool-result 3, frontend search 19 and type checks
  pass. Final verification and broader provider-suite limits are recorded in
  `docs/verification/2026-09-22-conversation-context.md` section 7.

- 2026-09-30 actual packaged Grok reproduction: normal start, same-session resume
  and real OK publication succeed. Denying access only to the newly created test
  session directory makes real Resume fail; persisted events change from
  provider_protocol_invalid to runtime_start_recovered_gone within one second,
  and the app visibly displays the overwritten generic error. Permissions restored.
  Evidence and exact limits: `docs/verification/2026-09-30-runtime-status.md`.

- 2026-09-30 direct runtime revalidation, per user instruction instead of external
  review: retained-completion recovery and macOS companion failure-custody tests
  pass. Managed-search failure is reproduced as a stale JSON-parsing test; updated
  to the current compact text contract and the same integration passes. ACP
  new/load cause loss and reconciliation error overwrite are reproduced and remain
  unfixed. Long-idle and other-platform outcomes remain unverified. The older
  2026-09-22 test status below is historical, not a current all-platform verdict.
  Full disposition: `docs/verification/2026-09-30-runtime-status.md`.

- 2026-09-30 user-authorized avatar consistency correction: preserve layout and
  provider/system icons; use the human profile photo/initials consistently in
  self profile, member list and general/custom messages. Profile storage remains
  authoritative; expose its label through participant projection and profile-update
  events, including existing memberships. No new polling or provider calls.
  Frontend 922 and persistence 355 tests, mandatory gates and signed packaged
  verification pass: existing general/custom messages, member and self avatars
  match; UI -> XY profile edit updates all three. Exact scope and limits are in
  docs/VERIFICATION.md. Existing test-only notes remain separate evidence.

- 2026-09-22: implemented the DeepSeek persistent-typing correction: chronological
  pending inputs preserve observation boundaries, completion failures expose
  existing recovery, and the existing reconciler commits the exact retained
  result without another model call. Persistence 350, affected server 5, frontend
  23 and the new managed failure/recovery integration pass. The actual isolated
  Windows desktop completes four sequential DeepSeek turns and returns to idle;
  original history is preserved. Unix executable-handle/reaping prerequisites
  are a separate correction. Affected Clippy and structure/format gates pass.
  Full-suite results remain incomplete: Windows fixture startup stalled, and
  Linux managed-search/failed-companion tests fail before completion. The final
  artifact gate is blocked by an oversized cache whose cleanup Cargo rejects
  for missing CACHEDIR.TAG; automatic review also blocked isolated-data deletion.
  Exact evidence and limits: `docs/verification/2026-09-22-conversation-context.md`
  section 8. App closed; no full verification pass claimed.

- 2026-09-22: completed the user-requested live feedback conversation with
  Grok 4.7, GPT-6 Astra and DeepSeek Flash; Codex participated through Room
  Connector MCP. Suggestions, model corrections, participant/event ID distinction,
  execution issues and evidence limits are recorded in
  `docs/verification/2026-09-22-conversation-context.md` section 7. These are
  follow-up suggestions, not implemented changes or additional acceptance coverage.

- 2026-09-22: connect custom text channels to the existing general-channel UI.
  Extract the general message row/actions into shared components and reuse its
  mention input, reply preview, grouping and composer styles. Keep channel
  authority, drafts, retry and context navigation in their existing owners.
  Frontend 918 tests, production/desktop builds and structure/artifact gates pass.
  Actual desktop creation, send, mention, reply, pin and source navigation pass;
  evidence: `docs/verification/2026-09-22-conversation-context.md` section 6.
  User-caught two-message false history notice corrected: loaded reply/pin sources
  now focus in place, preserving the current transcript. Both exact two-message
  desktop cases and affected navigation regressions pass.
  Shared participant dot mapping now uses the existing active-presence owner;
  joined humans show green in both general and custom channels. Rebuilt desktop
  verification and all 68 affected tests pass. Commits `9f417d8`, `8c77b6d` and
  `f006759` are pushed to `codex/recovery-and-sonnet`.

- 2026-09-22: implement user-requested conversation improvements: optional message
  replies, external MCP attachment read/upload, and public conversation/poll state.
  Commit each feature and finish with real desktop/MCP execution. Contract and
  acceptance: `docs/specs/conversation-context-slice.md`. Base `6f7707e`; push only
  `codex/recovery-and-sonnet`. All three feature commits are pushed. Actual Windows
  desktop, external MCP, configured Codex status/reply, poll and restart flows pass.
  Windows lifecycle/fixture corrections are committed as `037b142`. Rust 843,
  frontend 918 and desktop 30 tests pass; artifact/format/Clippy/structure gates
  pass. Policy 19 passes on WSL; Windows lacks the symlink-fixture privilege;
  evidence: `docs/verification/2026-09-22-conversation-context.md`.

- 2026-09-21 user-directed guest avatar correction: selecting/cropping a photo now
  keeps it in the browser until the guest confirms Join. The admission request
  commits the canonical image with the profile and invite in one transaction;
  invite-only prejoin attachment upload is closed. Affected server and frontend
  regressions pass. Signed isolated package starts, retains its test room after
  restart, and rejects unauthenticated attachment upload with 401. In the exact
  packaged guest flow, selecting and confirming a photo left prejoin/profile
  asset counts at 0/0 and invite use at 0; Join changed them to 0/1 and 1,
  and the browser rendered the guest photo. The temporary public tunnel was
  closed and the isolated app/data removed after verification. Full
  `make verify` passes with build-only `CARGO_PROFILE_DEV_DEBUG=line-tables-only`,
  `CARGO_PROFILE_TEST_DEBUG=0`, and `CARGO_PROFILE_TEST_BUILD_OVERRIDE_STRIP=none`
  to stay within the artifact gate and avoid the macOS 27 LINKEDIT macro issue.

- 2026-09-20 whole-repository Pro answer for dcaa42eb is complete: REVISE H1/M1.
  H1 Claude's approval showed only a tool name despite returning the original input;
  M1 an install check that recovered `completed=true` reopened the offer and retained
  the parent updating guard. Both are confirmed in the current owners. Corrections
  preserve the existing request/installation layout. Affected bridge, Rust and
  frontend tests plus architecture, build and Clippy checks pass. A provider full
  test run passed 281/282; its one HTTP-cancellation timing failure passed alone
  on rerun. Signed isolated 0.1.60 starts and its room/add-agent path was operated;
  the exact approval/install-recovery UI paths were not exercised in the package.
  Final bridge resource is rebuilt and matches source in the signed package;
  inspect and push correction, submit the next
  unrestricted whole-repository review, then stop after visible submission.

- At 22:25 KST 2026-09-20, unrestricted whole-repository Pro re-review is posted
  in the existing in-app conversation for dcaa42ebc236513dc1aca3935d10080501b05092.
  Both complete source snapshots (2708 files), manifest, full prompt and active
  Pro generation are visible. Corrections and affected verification precede this
  submission. Stop after confirming submission per the user; do not wait for this
  next answer or claim final approval. Branch: codex/recovery-and-sonnet.

- 2026-09-20 acceptance: signed 0.1.59 verifies actual Claude Sonnet 5 Low,
  two completed turns across Stop/Resume with the same native session, then Stop.
  The original Attachment Fixture generation 4 is recovered through the real
  previous-boot boundary, not a database reset. Requested header icon/tab changes
  pass packaged verification. Pro H1 deleted-vote rollback is reproduced in Rust
  and corrected; current Homebrew Node loader dependency loss is reproduced and
  corrected at SDK host binding. Affected tests and mandatory gates pass. Freeze
  and submit the whole repository for Pro re-review, then stop per user instruction.

- 2026-09-20: completed whole-repository Pro answer for 722b506e is read
  (46m10s, REVISE H1). A deleted vote tombstone fails historical context
  validation and rolls back the next assigned message. Correct the shared visible
  payload owner, preserving deletion privacy and non-deleted vote validation;
  verify actual API-context persistence before requesting unrestricted re-review.

- 2026-09-20: integrate retained recovery correction onto windows-runtime-fixes
  f2f08703. User authorizes Claude Sonnet verification in addition to the earlier
  Opus low request. Only requested frontend edits: always use the people icon for
  the member-panel toggle and remove the redundant room-connection-info tab button.
  Other frontend changes are prohibited; alternate API harness work is deferred.
  Finish affected verification and retained-session recovery before full re-review.

- Latest user correction2026-09-16 supersedes the submission stop below: resolve
  the retained Attachment Fixture continuation/stop recovery failure, complete
  affected verification, and only then submit the corrected whole-repository review.
  Investigation is active; neither recovery nor completion is claimed. Preserve
  the exact failed session and its custody evidence; do not reset its database state.

- At00:33 KST2026-09-16, unrestricted whole-repository Pro re-review is visibly
  submitted in the existing in-app conversation at722b506ef7c0df1507a86e6efe6912872ff647e8,
  with both full source snapshots (2690 files). Complete prompt, attachment and
  active Pro generation are confirmed. The earlier fixture recovery case is
  explicitly disclosed as unresolved. Stop after this submission per user scope;
  do not wait for the next review answer or claim final approval.

- Follow-up verification2026-09-16: instrumented native fixture completes the actual
  answer -> MCP initialize/read_discussion/publish_message -> turn completion ->
  normal Stop path twice, including Stop/Resume with the same native session.
  Session Request Continuation Check is stopped with2 turns, no error/recovery.
  The earlier Attachment Fixture failure is not explained by these passes: its
  original stderr was not retained, and its stop intent remains unconfirmed after
  restart (operation_in_progress). Keep this recovery case explicitly open for the
  full repository review; do not claim a root-cause fix or discard its evidence.

- 2026-09-16 corrections verified: custom-channel reconnect18/whole frontend890,
  actual Rust workspace file-owner6 including cross-process exclusion, unchanged
  architecture/source gates, policy19 and workspace all-feature Clippy pass.
  Signed0.1.55 held-server reconnect preserves one receipt, displays one message,
  clears draft and restores focus. Signed0.1.56 removes the standalone request
  row/modal: actual agent-attributed question, selection, response and same-message
  resolved result are verified visually. The controlled fixture receives the answer;
  its subsequent MCP continuation does not finish, so no completed provider turn
  is claimed. Stop retains recovery uncertainty; normal Quit leaves no owned
  processes. Isolated data retained in Trash. Freeze and submit full-source review.

- 2026-09-16 user correction: remove the standalone provider-request button/modal;
  put response controls and results inside the requesting agent message. Preserve
  existing header and panels. Include verified correction in current re-review.

- Current user request: read c2d1a117 review, correct supported findings, verify,
  submit unrestricted whole-repository re-review, then stop after visible submission.
  Completed Pro answer43m12s: REVISE Medium2. Channel send ACK lifetime incorrectly
  follows history reconnect lifetime; concurrent approved workspace replacements
  compare and rename without shared exclusion. Both are confirmed in source.
  Current implementation/verification remains in progress; no approval claimed.

- At22:57 KST2026-09-15, the next unrestricted whole-source Pro request is visibly
  posted with both complete source snapshots at c2d1a1172354a06e9a3e10b98baeddc9e05df4a8.
  Attachment, complete request and active generation are confirmed in the existing
  in-app conversation. Current review corrections and affected verification are
  complete. Stop now per the latest user instruction; do not await this next answer.
  No final review approval is claimed.

- Latest user stop condition (2026-09-15): receive the current e60a8e76 review
  completely, correct its supported findings and complete affected verification,
  submit and visibly confirm the next unrestricted whole-repository review, then
  stop without waiting for that next review answer. This supersedes the earlier
  continue-until-approved instruction for this run.

- Current e60a8e76 Pro review is fully read (45m34s): REVISE Medium3.
  R1 whole-message attachment prefix passes persistence339, server attachment2 and
  signed0.1.53 real picker/queued5+4/read/publication/Stop/Quit verification. R2/R3
  explicit API media/size tool failures now reach the model without aborting the
  turn. Actual portal/API integration, provider264, workspace Clippy and signed0.1.54
  real upload/read/publication/normal Stop/Quit pass. This preserves text-only API
  and128KiB limits; it does not claim media or partial-attachment support. Freeze,
  push and visibly submit the next unrestricted full-source review, then stop.

- At21:58 KST2026-09-15, submitted unrestricted whole-source Pro re-review at
  e60a8e761ff3fe5108a8126285bf184ae26646b6 with both full source snapshots.
  Complete request, attachment and active generation are visibly confirmed in
  the existing in-app conversation. Await/read the completed answer and continue
  supported corrections. Packaged R1 and actual Rust R2 verification now pass.

- Whole-source6b5b19dd Pro completes26m58s: REVISE Medium2; full final answer read.
  R1: independently valid pending questions overflow required owner reconnect
  metadata. R2: builtin file results satisfy character caps but overflow the final
  JSON byte cap and become generic failures. Correct bounded request admission
  frames and file-result byte budgeting; reproduce at real Rust owners, verify
  packaged behavior and obtain unrestricted whole-source re-review. Signed0.1.52
  now passes four concurrent long requests, re-entry, all answers and normal Stops.
  Server275/provider262/native36 and frontend883 covered pass. Freeze the corrected
  source and obtain the completed unrestricted whole-source Pro answer.

- Whole-source b09d3de4 completed46m25s, REVISE H1/M1/L1; fully read.
  All three owner corrections now pass frontend877, persistence338, provider259,
  full all-feature server274, native36 and unchanged source gates. Signed0.1.48
  proves both exact manual creation retries and failed-start Stop;0.1.49 proves
  catalog refresh/re-entry/restart. The independent failed-turn result-loss fix
  now preserves its typed result atomically through process exit.
  Existing empty Codex native threads cannot resume: actual CLI returns no rollout
  found after idle Stop. An explicit native legacy history mode plus thread/name/set checkpoint preserves
  the same empty thread across process restart; default paginated mode does not.
  Also reproduced catalog-only refresh invalidating M1's retained creation intent;
  exclude discovery revision from the comparison while preserving the exact payload.
  Signed0.1.51 now proves catalog-refresh exact retry and idle Stop/Quit/restart/Resume
  with the identical native ID and zero turns. Provider261 covered, server20,
  native36, creation29/build and unchanged gates pass. At02:36 KST2026-09-15,
  sent the unrestricted whole-source Pro review at6b5b19dd with both repositories'
  full source. Attachment, complete request and active generation are visible in
  the existing in-app conversation. Await/read its final answer and continue
  supported corrections; current old archives are superseded.

- Cursor Auto native calls were rejected because runtime spawn reused the catalog's
  default Reject policy. Exact qualified tool identity was present. Pass RoomTools
  explicitly for real sessions and Reject explicitly for catalog discovery. The
  regression now uses the actual Cursor configuration: baseline rejects an allowed
  read. Provider coverage259 (258 full-run passes plus corrected stale size fixture1),
  Clippy and unchanged gates pass. Signed0.1.47 resumes the exact old session, reads
  and publishes52 twice, then stops with all exact children absent. Claude old
  failure Stop now also passes; normal Quit and artifact checks pass. Latest review
  remains pending.

- At00:16 KST2026-09-15, posted complete Rust/original source atb09d3de4 for
  independent whole-repository Pro review in the existing in-app conversation.
  Attachment, full request and active generation are visible. Await/read final.
  The post-failure Stop dead end is corrected and signed0.1.47 verified: the
  terminal failed execution already confirms runtime exit, so explicit Stop can
  finalize its durable state. Persistence338 and unchanged gates pass.

- Claude OAuth is confirmed logged in despite Safari's final localhost callback
  error. Direct SDK Opus5 returns68; native Stop confirms applied low. App.45 rejects
  absent init effort, a field the SDK only advertises on other host types. Correct
  the receipt owner. Signed0.1.46 now passes two actual Opus5/Low room reads and
  replies, normal Stop and exact provider-process exit. Normal Stop also exposes
  missing runtime-handle ownership after recovery; corrected and verified above.

- User authorization added: verify real Claude SDK with the currently advertised
  Opus model and Low reasoning (2026-09-14). Confirm catalog selection, actual room
  read/response and normal stop; retain the ongoing review corrections and Cursor
  diagnosis. This overrides the earlier real-provider exclusion for this run only.

- Whole-source Pro at frozen3d91736d completed48m43s: REVISE High1/Medium1;
  full answer read. R1: imported persona names can exceed mandatory snapshot bytes
  after session creation/configuration. R2: vote/custom-channel UI discard exact
  retry handles and can resubmit uncertain intent with a new ID. Reproduce through
  actual owners, correct, verify packaged flows and obtain whole-source re-review.
  R1 bed0ed50 and R2 now pass affected checks plus signed0.1.46 import/retry/restart
  verification; both retry cases have one event and one receipt. Latest whole-source
  review remains. Earlier roster and generic/lobby corrections are confirmed. Cursor.45
  package verification continues; prepared3abb4c15 archive is not yet submitted.

- Signed0.1.44 finds the official Cursor package exceeds the old512MiB staging
  bound (584,651,262 bytes). Correct the runtime bound to640MiB while preserving
  complete package identity and verified copying; repeat actual Auto verification.

- Cursor's official2026.09.10 package now emits qualified same-call MCP identity
  before permission; a real isolated Auto diagnostic confirms it. Correct the ACP
  contract selection to consume Cursor's native notification identity while keeping
  Grok's existing contract, exact room scope, conflict rejection and AllowOnce.
  Baseline regression fails; full provider259, workspace Clippy and unchanged gates
  now pass. Signed app Auto response and latest whole-source review remain pending.
  [Acceptance](docs/specs/final-parity-slice.md#cursor-qualified-mcp-permission-contract-2026-09-14).

- At22:37 KST, continued the unrestricted whole-repository Pro review at frozen
  `3d91736da0d5a5dccf850b0a9857077c61ade026`, including the later query correction
  and real Antigravity evidence. The complete attachment/request and active response
  are visibly posted in the same in-app conversation. The prior6935beb1 answer
  remained progress-only after reload, with no stop control or final verdict; it is
  not a completed review. Await/read this final answer and continue corrections.

- At 21:50 KST, submitted independent whole-repository re-review at frozen
  `6935beb11317d0586285928dea15fbafb0d0066f` in the same in-app Pro conversation.
  The complete Rust/original archive and unrestricted request are visibly posted;
  Pro is generating. Await and read the completed verdict; do not end merely for
  waiting. Code and packaged corrections are complete locally, not yet approved.
  A later measured query correction computes inactive human membership once per room:
  identical 10,000-row query results improve from1.6883s to6.7ms; actual socket4,
  persistence336 and Clippy pass. Signed0.1.43 loads retained rooms and exits with
  all exact owned processes absent; review this later source after the frozen answer.
  User-approved temporary external access and current-AI invitation creation/copy
  pass. Terminal UI access is denied by Computer Use, so Antigravity participation
  was initially unverified. Direct interactive CLI verification subsequently passes
  join/read/publication/leave/receipt release with individual tool approvals. The
  app and database confirm one message, Left and revoked membership. External access
  is closed, temporary MCP removed and all owned processes exit; see verification.

- Whole-source Pro at `7df6bb09` completed (43m1s): REVISE H1/M2; full answer read.
  The other seven prior corrections are confirmed. Current H1 covers terminal and
  expired participant accumulation; M1 covers manual retry identity loss; M2 is the
  post-send recovery stall already corrected and packaged-verified at `12f556b4`.
  H1 now passes canonical 1000-departure/expiry actual WebSocket4, persistence336
  and workspace Clippy. M1 now passes transport/composer28, frontend867, production
  build/CSS and signed 0.1.42 exact retry with one stored message/receipt plus restart.
  Submit the latest whole repository without restrictions and read its full answer.
  The prepared `12f556b4` archive is superseded by these active corrections and has
  not been sent. [Current evidence](docs/VERIFICATION.md#completed-whole-repository-pro-review-2026-09-14).

- Latest user instruction (2026-09-14) supersedes the split review and continuation
  directions below: obtain one independent whole-repository review, without phase,
  file, known-finding or implementer-prescribed review-method restrictions.
  Sent GitHub repository and current frozen commit
  `0ee154c8423beb1283e13c649685738064da35c8` to GPT-6 Pro in a new in-app conversation:
  https://chatgpt.com/c/6aa7aa6c-8608-83ee-b6b9-0e7833fcaf68 .
  The prior conversation and observed evidence remain preserved. No old 182-patch
  bundle or phase-specific review request was sent to the new conversation.
  Await and read the completed answer, then correct, verify and re-review supported
  findings. An in-progress request does not constitute review or phase completion.
  The first whole-repository answer (76m22s) is explicitly incomplete, with one
  unconfirmed ahead-of-history reconnect-cursor candidate, not a confirmed severity
  count or failed build/test result. Its full answer has been read. At 18:24 KST,
  supplied both exact Git snapshots in `aa-whole-repository-0ee154c8.zip` and requested
  completion of the same whole-repository review. Attachment and request persist
  after reload; Pro reports reading the archive. Code review and unperformed runtime
  execution must remain separate. The client's repeated rejected cursor is now
  reproduced and corrected. Frontend861, production build/CSS, actual WebSocket2,
  workspace Clippy and mandatory gates pass. Signed 0.1.36 confirms ordinary
  reconnect, history and post-restart delivery; packaged same-name recreation is
  not claimed. The corrected source requires review after the frozen review.
  The second whole-source answer is now complete (54m54s): REVISE H2/M5/L1.
  All eight findings and execution limits were read; supported corrections and
  latest-source whole-repository review remain. See the current disposition in
  [verification](docs/VERIFICATION.md#completed-whole-repository-pro-review-2026-09-14).
  Unperformed reviewer execution is not a failed product test. R1 is now reproduced
  and corrected across queue restoration owners (persistence334 / workspace Clippy);
  R8 joined reader cleanup also passes. R6 admitted leave now passes actual
  Connector13 and workspace Clippy. R2 now reproduces 1,000 canonical departures
  and fixes new-socket metadata while retaining author history. These corrections
  require latest-source review. R3 deleted-edit public reads now pass persistence335,
  frontend862, production build/CSS and workspace Clippy; packaged checks remain.
  R7 now passes actual 31-second wait/message delivery, Connector14 and transport2
  while preserving ordinary HTTP limits. R4 builtin API/Local workspace tools are now locally restored with provider258,
  controlled approval/custody and API tool-round checks passing. Signed 0.1.37
  verifies configured DeepSeek read/search, denied write, approved write/replace
  and actual file read; 0.1.38 verifies the corrected hint and restart persistence.
  Corrected-source whole-repository review is still required.
  At 20:44 KST, submitted the complete corrected snapshots at Rust
  `7df6bb092e00c63534d58bc25e3732b60e186142` and original `d5046473` in the same
  in-app Pro conversation. The posted request explicitly retains independent
  whole-repository scope without previous-finding or file restrictions; attachment
  and active response are visible. Await the completed answer; this is not approval.
  While waiting, signed 0.1.38 verifies R3 edit/delete/restart display and search.
  It also exposes stale compact-panel stacking hiding search results; the shared
  panel owner now resets the overlay z-index. Signed 0.1.39 verifies unobscured
  search, result selection/dismissal, side-chat switching and composer/top-rail
  preservation. Header/side-chat7, production build/CSS and mandatory gates pass.
  Normal Quit confirms exact owned-process cleanup. This later correction requires
  review after the current frozen answer.
  During the ongoing review, independently reproduced three post-send recovery
  stalls (ticket failure/hang and unfinished handshake). The existing command timer
  now bounds replay preparation and settles outcome_unknown without new intent or
  false rejection. Retry9, full frontend865, production build/CSS and mandatory
  gates pass. Signed 0.1.40/0.1.41 verify held-server deadline, editable preserved
  draft, Korean uncertainty guidance, actual reconnect and a distinct fresh send.
  Held servers are resumed and exact processes exit normally. The completed
  whole-source review and re-review of these later corrections remain pending.

- Review evidence retention correction (2026-09-14): the 15:24 continuation later
  left only progress text and no final verdict; after reload the stop control was
  absent. Continued from the checkpoint and verified the new full-scope request
  survived reload. Preserve locally observed reviewer outputs in
  `/tmp/aa-pro-review-evidence-2135d511-20260914/manifest.json`: initially five raw output
  captures, eight explicit per-SHA records, and observed saved counts through 22.
  Subsequent captures append without replacing prior observations.
  File hashes were checked. These are reviewer evidence claims, not whole coverage
  or implementation approval. Supply this retained record with the next review
  continuation rather than relying only on the web conversation's memory.

- Latest completed Pro continuation (2026-09-14, 72m41s) is still partial.
  M3 is formally withdrawn; whole Phase 7–9 is not approved. Final prose reports
  9/182 full patches, but the same run's actual `save_116.py` output shows
  `FULL_PATCHES_SAVED 22` with per-SHA source, judgment and saved-tail evidence.
  Preserve both facts until the reviewer reconciles its actual files; neither
  count establishes complete coverage. The full-scope continuation at 15:24 KST
  requests SHA-level reconciliation and remaining review from the already-recovered
  immutable source/182-patch archive. Do not reset actual reading to the smaller
  prose count, discard prior evidence, or treat this as phase closure.

- Connector admission/read custody correction (2026-09-14): actual remote MCP at
  `b2acf04d` reproduced a read-budget rejection during an admitted rejoin deleting
  its private handle and preventing leave. Removal now requires Pending admission,
  checked and closed under the client's existing state lock. Actual Connector12
  and all-target/all-feature workspace Clippy pass; affected-source review remains.
  Pro's next completed 95m50s answer remained partial (no additional full patches).
  Its G3-M3 `event` claim contradicts frozen `2135d511` context lines 225–237 and
  ancestor `4bf1c154`, both already using `event_id`. The full-scope continuation
  and source rebuttal were saved and verified after reload at 14:07 KST. During
  that ongoing review Pro explicitly withdrew M3 after checking both revisions.
  This is a withdrawn finding, not a new code fix or a whole-group approval.

- Active frontend correction (2026-09-14): preserve existing UI except required
  behavior additions or demonstrated usability fixes. Keep the top action rail
  stable while the right panel changes; restore side chat beside the main chat.
  The current compact fixed panel covers the transcript/composer. Replace that
  overlap with shared-width layout, verify against the running Discord reference
  and signed packaged interaction, and reconcile each change in
  `docs/FRONTEND_BACKEND_GAPS.md`. Signed 0.1.34/0.1.35 now confirms shared-width
  main/side chat, stable top actions, panel close/switch, drafts, search and custom
  channel layout. Frontend859 and unchanged mandatory gates pass. The gap map
  also reconciles omitted post-September-10 exposure changes. Final source review
  and remaining acceptance are still open; this is not phase completion.

- Current closeout handoff (2026-09-14): review source is GitHub repository
  `kwd421/agentsassemble-rust` at frozen `2135d5112d00f80996a5f6ce98762515049cfcb8`.
  Completed Pro Phase 1–3 and Phase 4–6 source verdicts are APPROVE
  C0/H0/M0/L0 in their respective scopes; P13-M7 and G2-M1–M6 are closed.
  Independent Phase 7–9 review remains incomplete in
  [the current Pro conversation](https://chatgpt.com/c/6aa7434d-c86c-83e8-b01d-c4a118677623).
  Use the Codex in-app browser only for this review; the user explicitly rejected
  Chrome. The reviewer returned an incomplete answer, then retracted its claim
  that no source had been returned. Resume the same full request at the frozen
  revision with GPT-6 Pro; the retraction is not a completed review or approval.
  While that immutable review runs, a local correction is being verified for
  committed Connector leave receipt recovery after room archive/close. Its regression
  failed with `room_inactive` before the correction, then passed; persistence333
  and Connector HTTP/MCP8 pass. Full workspace Clippy and affected gates pass;
  corrected-source re-review and remaining real-flow acceptance remain.
  Signed 0.1.32 now confirms desktop archive/restore, narrow-layout restart/message
  restoration and room close, plus normal owned-process cleanup. Packaged Connector
  admission/recovery was not completed; retain the separate HTTP/MCP evidence.
  This correction is not part of the frozen Pro review revision.
  The saved resumed Pro answer is still partial: it identifies G3-M1-R1 at the
  MCP caller response boundary (Hub removes the private leave handle before result
  delivery), with only 2 full patches and 1 partial patch read out of 182. That
  finding now has a local correction; it is not whole-group coverage. The complete answer
  was read and a continuation retaining the full independent scope was submitted.
  Explicit Connector wait resynchronization now passes actual HTTP/MCP9 and
  all-target/all-feature Clippy plus unchanged architecture/format/diff gates.
  The MCP terminal response now remains replayable until explicit caller receipt
  release, within the existing 128-entry registry; actual MCP-response-loss,
  concurrent replay, authority denial and capacity recovery pass. Connector10,
  workspace Clippy and unchanged architecture/format/diff gates pass.
  All new corrections require affected-source re-review.
  The ordinary MCP publication response-loss candidate is also independently
  reproduced: the original path writes two messages for one retried intent.
  Caller-owned required request UUIDs now reach the existing canonical receipt
  owner; exact replay, conflicting reuse and intentional distinct-ID publication
  pass in actual HTTP/MCP. Connector11 and workspace Clippy pass; no approval is
  inferred for this later source from the frozen review.
  Follow-up correction: retained receipts made implicit stdio leave ambiguous
  after a new join. The actual regression incorrectly left the new participant;
  leave/release now requires an explicit ID when multiple mappings exist.
  Exact old replay and current-room read/new explicit leave pass, alongside
  workspace Clippy and unchanged gates. This later fix also requires re-review.
  User correction: implementer-selected findings, files and paths must not limit
  or steer the independent review. Derive coverage from user requirements and
  actual source, including omissions and new problems outside those examples.
  Review the implementer's claims, documents and tests critically as well.
  Continue through completed evidence, supported corrections, affected checks and
  re-review. The user deleted the recurring review schedule and requires this
  active session to continue until the reviews are received; do not recreate the
  schedule or end the turn merely because a review is still running.
  A displayed outgoing message is not proof of saved delivery; the
  corrected request was verified to persist after reload. Do not treat progress
  labels as finished source review. Preserve the frozen review revision and the
  user-owned untracked `.agents/` and `scripts/__pycache__/` directories.
  [Completed verdicts and remaining boundaries](docs/VERIFICATION.md#completed-phase-1-6-source-review-disposition-2026-09-14).

- Active: correct the user-rejected agent-creation login/update flow and catalog
  refresh presentation (2026-09-10). Login must follow confirmed authentication need
  during creation and return to the preserved draft; version inspection must offer
  Update/Later only when a newer version exists, with an actual supported update
  action. Remove permanent login/version/help controls from the creation form and
  Automatically inspect only the selected provider, using its own 24-hour cache;
  one manual refresh requests that provider regardless of cache age. Remove the
  whole-catalog refresh endpoint and its replaced callers and tests. Do not query
  authenticated model APIs before required authentication is available; successful
  login refreshes only that provider and preserves the creation draft.
  Automatic and manual refresh target only the requesting user's own computer's
  CLI installations and connected API accounts, never another user's or the remote
  room host's catalog merely because the user is connected to that room.
  The previous setup
  implementation and passing checks do not meet this user-flow acceptance.
  [Corrected acceptance](docs/specs/operational-surfaces-slice.md#agent-creation-setup-flow-correction-2026-09-10).
  Selected local discovery is implemented at `e980eef7`; cache/authentication boundary
  tests and signed packaged creation, refresh, existing settings and restart pass.
  Confirmed authentication-need handling now passes scoped tests and packaged
  OpenCode/Cursor creation checks; Codex's installed CLI rejects the current personal
  configuration, which remains unchanged. Optional update execution and automatic
  offers pass controlled checks and signed 0.1.19 login/update cancellation, failure,
  retry, completion and draft-preservation flows; single-click Later also passes.
  Pro's completed `bfa62144` Phase 1–3 supplemental/current integration review
  returns REVISE C0/H0/M3/L2. Correct own-PC setup-to-creation continuity,
  update transport-loss custody, setup readiness after update and the obsolete
  updater contract. Catalog cleanup uncertainty now retains terminal ownership and
  passes controlled lifecycle and affected workspace verification. Update transport
  loss and readiness corrections pass signed packaged 0.1.20/0.1.22 checks, including
  the held installer, lost response and catalog-only recovery. Own-PC continuation
  now passes signed 0.1.23/0.1.24 local selection, rejected-draft preservation, real
  remote admission, add-only/start/cancel and restart-guard checks with controlled
  CLI execution. Full workspace tests and affected mandatory gates pass. Direct
  browser interaction/OS dispatch is blocked by browser policy and remains unverified,
  as does real two-machine proof. Pro's completed `7a3d6359` re-review closes
  M2/M3/L1/L2 and returns REVISE C0/H0/M3/L0: remaining paired-operator invite
  issuance (M1), cancellation during pending local start (M4), and self-targeted
  local-attendee cleanup before ingress shutdown (M5). The complete answer, separate
  verdict and E01–E51 source evidence have been read. Correct supported findings,
  verify and re-review before continuing the split groups. Paired invitation,
  pending cancel and self-targeted shutdown corrections now pass affected checks
  and signed packaged verification. Two further packaged cleanup defects
  (unassigned external-report race and early factory cancellation) are corrected;
  final `144f6050` workspace 912 tests and mandatory gates pass, and signed
  0.1.28 confirms both early and held-initialize cancellation plus exact local
  lease release. Isolated apps/data are cleaned and artifact check passes.
  Pro's completed `10308cae` re-review closes M1/M4/M5 and approves both additional
  cleanup corrections and all six new changes, but returns REVISE C0/H0/M1/L0 for
  new P13-M6: desktop's three-second stop kills the runtime supervisor before its
  own sixteen-second grace and the attendee's thirty-second remote cleanup finish.
  The complete answer, separate verdict and E01–E45 source evidence have been read.
  The outer shutdown lifetime correction passes affected provider/HTTP checks and
  32 native tests, including delayed EOF cleanup and emergency process-tree stop.
  Signed 0.1.29 normal quit keeps app/supervisor/server alive beyond seventeen
  seconds while real remote cleanup responses are held, then finishes both add-only
  and Running cleanup with exact lease release before exit. Full workspace 914,
  frontend 859 and native 32 tests plus mandatory gates pass; isolated artifacts
  are cleaned. Pro's completed `a41b4f0c` review leaves P13-M6 partially open as
  M6-R1: pre-Ready recovery misses parent EOF and can start further targets.
  Full verdict and N01–N31/R01–R04 evidence are read. Recovery cancellation and
  joined cleanup now pass 917 workspace tests, mandatory source gates, actual
  queued-control replacement and signed 0.1.30 early cancellation plus controlled
  held-factory normal Quit. The app scheduling control and existing five-second
  ticket-timeout behavior are disclosed in the evidence. Isolated artifacts are
  cleaned and artifact check passes. Pro's completed `a9d37b05` review approves
  M6/M6-R1 and the correction delta, but current Phase 1–3 remains REVISE
  C0/H0/M1/L0 for P13-M7: a normal status ticket's five-second wait closes the
  parent pipe and cancels legitimate recovery. The full answer, separate verdict
  and A00–A21/R01–R05 evidence are read. Correct the native waiter/pending-response
  lifetime. The correction passes 36 native tests and mandatory source gates;
  signed 0.1.31 natural restart survives delayed thread resume, visibly completes,
  restores both Idle sessions and permits fresh result/status queries. Normal Quit
  cleans all owned processes and exact leases. Isolated artifacts are removed and
  artifact check passes. The affected-head Pro review now closes M7 and approves
  current Phase 1–3; the later Phase 4–6 source review also approves its scope.
  Independent Phase 7–9 and final real-flow acceptance remain open.
  [Recovery evidence](docs/VERIFICATION.md#parent-loss-during-replacement-recovery-2026-09-11).
  Full flow acceptance remains open.
  [Latest evidence](docs/VERIFICATION.md#outer-shutdown-lifetime-correction-2026-09-10).
  Real installer/OAuth
  outcomes remain unverified.
  [Review disposition](docs/VERIFICATION.md#completed-setup-supplemental-pro-review-2026-09-10).
  [Update evidence](docs/VERIFICATION.md#optional-update-execution-during-creation-2026-09-10).
  [Scoped evidence](docs/VERIFICATION.md#selected-local-provider-discovery-2026-09-10).
  [Authentication evidence](docs/VERIFICATION.md#authentication-need-during-creation-2026-09-10).

- Active: complete the final split Pro reviews without pausing between results,
  correct supported findings, and verify/re-review the final changes, as requested
  by the user. The immutable `a41b12a` review snapshot remains available while
  corrections continue. Signed app/helper/server startup and stored-key access after
  a different signed server build pass. The 2/64px visible-read mismatch and modal
  dismissal re-evaluation are corrected; background automation was not proof of a
  foreground WebView failure. Final affected-head reviews remain pending.
  [Signing contract](docs/specs/operational-surfaces-slice.md#signed-desktop-supervisor-and-keychain-continuity).
  Pro Group 2 completed at `7e10252`: REVISE C0/H0/M2/L0. Correct the
  external-session action mismatch and room-menu read persistence, then obtain
  affected verification and Pro re-review before continuing the remaining groups.
  Both Pro corrections pass packaged verification through `5c6199a`; Daybreak then
  identified unsupported external resident pause/resume advertising, now corrected
  with affected tests passing. User decision (2026-09-10): reuse prior Daybreak
  reviews for already-reviewed areas and continue with Pro only; its model-access
  failure does not block this scope. Pro's completed `ea10659` review closes those
  findings and identifies two further corrections: expose actual external interrupt
  capability and guard channel read actions until preferences are loaded. Both
  corrections now pass affected tests, gates and the scoped packaged checks;
  Pro `80483c2` closes M3/M4 but finds one new profile ACK/state-event projection
  mismatch (G2-M5). The persistence owner now reuses the committed event projection;
  external pre-ready/false/true outcomes, exact replay, strict ACK/socket consumption
  and signed packaged rename/restart/restoration pass. Pro `ac94b92` closes M5
  and approves that correction, but identifies a pre-existing deadline interleaving
  in profile ACK event sequencing (G2-M6). The event owner now reconciles once
  before its contiguous profile pair. Controlled open/resolving expiry, atomic
  failure/retry, public ACK/socket consumption, server integration and signed
  packaged save/restart/restoration pass. Pro's completed `e1dfd235` review closes
  M6 and approves the correction, cumulative range, whole Phase 4–6 integration
  and affected Phase 1–3 supplement at C0/H0/M0/L0; M1–M5 remain closed.
  Independent Phase 7–9 Pro review at that revision returns REVISE C0/H1/M2/L0:
  remote Connector retry custody, terminal leave receipt recovery and unconfirmed
  login cleanup retention. Remote private custody and exact terminal leave recovery
  now pass affected persistence, actual HTTP MCP and stdio verification. Login
  cleanup retention also passes controlled task-loss, retry/cancel/shutdown and
  affected transport checks. Final packaged verification and Pro re-review continue.
  The reviewer also reports incomplete historical patch reading; all individual
  Phase 7–9 commits and whole-group completion still require finished coverage.
  [Correction contract](docs/specs/final-parity-slice.md#external-session-action-ownership-correction-2026-09-10).

- Active: close out user-requested Harness/API freshness, inline OS credential
  approval, and the observed unread-state correction, then obtain Daybreak review.
  Broader mobile UI/UX is explicitly deferred by the user and remains incomplete;
  agent-selected desktop stress dimensions do not constitute mobile design acceptance.
  [Contract](docs/specs/operational-surfaces-slice.md#provider-model-freshness-2026-09-09).
  Pro sequence: supplement changed Phase 1–3 at the frozen HEAD, then 4–6, then
  7–9; brief each completed result and continue through corrections and re-review.
  [Owner](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).

- Earlier browser-to-local provider setup mechanics were locally verified:
  login/installation guidance, local setup links and optional version updates
  preserve provider/device authority. Daybreak approved all three commits,
  cumulative changes and exact `9bfc3a9` integration at C0/H0/M0/L0.
  [Verification](docs/VERIFICATION.md#browser-to-local-provider-setup-and-optional-updates-2026-09-09). [Contract](docs/specs/operational-surfaces-slice.md#browser-to-local-provider-setup-user-request-2026-09-09).
  Final split Pro review uses immutable `a41b12a` snapshots; supported corrections
  continue under the latest user instruction.

- Test/security duplication follow-up is locally complete: same-transaction reads,
  redundant codec checks, and fake test crypto removed; [evidence and retained boundaries](docs/VERIFICATION.md#test-necessity-and-security-duplication-audit-2026-09-07).
- User-requested repository-wide optimization is locally complete: measured duplicate
  work removed and creation cancellation fixed; scope, verification, and remaining finding:
  [optimization audit](docs/VERIFICATION.md#repository-wide-optimization-audit-2026-09-07).
- Custom API response-model correction is locally complete:
  [adapter-path verification](docs/VERIFICATION.md#custom-api-resolved-model-contract-2026-09-07).
- User scope revision: Freebuff is excluded; Antigravity is external CLI plus
  Room Connector invite only (Phase 7). Both managed registrations and Antigravity's
  resident path are retired; the fourteen-provider Phase 1 passes local verification
  and Daybreak's whole-phase review through `cae1f90`.
- Codex native terminal-status correction passes affected local verification;
  [failure/publication and interrupt evidence](docs/VERIFICATION.md#codex-native-terminal-status-correction-2026-09-07).
- Phase 1 is closed through `2a49599`: Pro's three supported findings are resolved;
  Daybreak approved all 136 commits, cumulative phase, final HEAD and complete local
  contract. [Disposition and verification](docs/VERIFICATION.md#completed-pro-review-corrections-2026-09-07).
- Phase 2 is closed through `5c8d17b`: Daybreak approved every individual commit,
  cumulative phase, final HEAD and complete local contract at C0/H0/M0/L0.
  [Evidence](docs/VERIFICATION.md#phase-2-exact-agent-session-controls-2026-09-07).
- Phase 3 is closed through `b8fd15b`: Agent identity, avatar custody and editor
  pass local acceptance and whole-phase Daybreak review at C0/H0/M0/L0.
  [Contract and acceptance](docs/specs/agent-profile-slice.md).
- Phase 4 is closed through `53a82f1`: Daybreak approved every final phase correction,
  cumulative phase, final HEAD and whole local contract at C0/H0/M0/L0. The one
  late-moderation terminal-export finding is closed; direct packaged desktop/mobile
  settings, profile, controls, moderation and room lifecycle are verified.
  [Contract](docs/specs/room-lifecycle-slice.md) and [review correction](docs/VERIFICATION.md#phase-4-whole-phase-review-correction-terminal-export-2026-09-08).
- Phase 5 is closed through `1e24adf`: Daybreak approved both Google corrections,
  the cumulative 44-commit phase, exact HEAD and whole local contract at C0/H0/M0/L0.
  [Review disposition](docs/VERIFICATION.md#phase-5-whole-phase-review-corrections-2026-09-08)
  and [account/social/human contract](docs/specs/identity-accounts-friends-slice.md).
- Phase 6 is closed through `25c7961`: Daybreak approved all 17 commits,
  cumulative phase, exact HEAD and complete local contract at C0/H0/M0/L0.
  [Correction and approval](docs/VERIFICATION.md#phase-6-whole-phase-review-correction-http-incarnation-2026-09-08).
- Phase 7 is closed through `d70224b`: Daybreak approved the correction, cumulative
  95-commit phase, exact HEAD and whole local contract at C0/H0/M0/L0. All three
  custody paths, packaged desktop/mobile and Windows process/IPC verification pass.
  [Correction](docs/VERIFICATION.md#phase-7-whole-phase-review-correction-muted-owner-2026-09-09).
  [Contract and evidence](docs/specs/external-ai-admission-slice.md).
  New Pro review and authorized real-provider proof remain at full closeout.
- Phase 8 is closed through `f776e2e`: Daybreak approved its usage correction,
  cumulative phase, exact HEAD and whole local contract at C0/H0/M0/L0.
  [Contract](docs/specs/operational-surfaces-slice.md) and [correction](docs/VERIFICATION.md#phase-8-whole-phase-review-correction-usage-freshness-2026-09-09).
- Active: Phase 9 final exposure, integration, resource measurement and authorized
  real-provider verification. [Acceptance](docs/specs/final-parity-slice.md).
- Scope, acceptance, dependency order, and finding placement:
  [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#phase-1--provider-contract-and-process-correctness).
- Execution cadence and reviewer settings:
  [per-slice execution gate](docs/PRODUCT_REIMPLEMENTATION_PLAN.md#per-slice-execution-gate).
- Current local exit evidence:
  [packaged provider catalog and real-turn matrix](docs/VERIFICATION.md#packaged-provider-catalog-and-real-turn-matrix-2026-09-03).
- Final real verification, after reimplementation: configured DeepSeek, Codex,
  OpenCode, external Antigravity, Grok, and Cursor `auto` only. Other providers use
  original-contract implementation and local verification, without real execution.
  Packaged frontend flows are verified by direct app manipulation in every phase;
  final real-provider flows add integrated coverage. Local tests, mandatory gates,
  and phase code reviews continue during implementation.

## Read routes

Read the relevant sections and owning code, not entire documents or historical logs.

- Implementation constraints: [Rule.md](Rule.md). Substantial design: [SDD.md](SDD.md).
- Scope and sequencing: [product plan](docs/PRODUCT_REIMPLEMENTATION_PLAN.md).
  Historical finding labels do not override its approved phase order.
- Architecture, protocol, persistence, auth, lifecycle, or cutover:
  [architecture](docs/ARCHITECTURE.md) and the corresponding contract in `docs/specs/`.
- Frontend changes: read [frontend UX guide](docs/FRONTEND_UX_GUIDE.md) before editing.
- Frontend or real-client verification: the affected entry in
  [frontend gaps](docs/FRONTEND_BACKEND_GAPS.md) and [verification](docs/VERIFICATION.md).
- Finding history: [repository audit](docs/architecture/REPOSITORY_AUDIT_2026-09-01.md).
- Artifact maintenance: existing `make artifact-check` / `make artifact-prune` owner;
  preserve the current limits and do not clean while Cargo/Tauri work is active.
- Board creation or restructuring: [WORKBOARD_GUIDE.md](WORKBOARD_GUIDE.md).

## Historical evidence

Earlier board records remain in Git at `be63b1e4e6031853a3a666bb7deddd82781ce43d`:
`git show be63b1e4e6031853a3a666bb7deddd82781ce43d:WORKBOARD.md`.
Consult them only for a specific historical finding or review; current contracts,
execution evidence, and active state remain with the owners linked above.
