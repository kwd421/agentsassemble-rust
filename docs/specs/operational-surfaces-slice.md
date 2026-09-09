# Operational surfaces

Status: Phase 8 contract established after Phase 7 approval at `d70224b`.
Implementation and local acceptance are pending. Real-provider execution and both
final reviewers remain governed by the product plan's final closeout gate.

## Definition and current reachable contracts

Preserve supported provider login, account usage and explicit catalog refresh.
Preserve confirmed runtime diagnostics and update/restart operations. Operational
metadata is not permission to start a model turn or expose host credentials.

The original checkout `d504647` supplies these reachable entry points:

- `web/routes/providers.py` mounts local-operator provider login and catalog refresh,
  plus credential-authorized account-usage reads. `providers/login.py` runs only
  provider-defined login commands: Codex, Claude, Grok and Cursor use browser OAuth;
  OpenCode opens its interactive authentication UI and reports started, not authenticated.
- Agent creation and member usage are ordinary frontend controls. Original
  `provider_usage.py` dispatches provider-specific readers. Codex reads app-server
  rate limits; DeepSeek reads its authenticated balance endpoint. Claude's original
  terminal reader can be replaced by the already-pinned SDK's structured usage API.
- Grok terminal-screen extraction and OpenCode Go HTML extraction are original
  implementations, not permission to introduce scraping under the current Rust
  contract. Their operational capabilities require a supported current native/API
  owner; absent evidence must remain explicitly unsupported, never fabricated usage.
- `web/routes/observability.py` mounts resource and release-health projections.
  `assemble release-health list/run` selects fixed local checks, bounds their
  execution and persists a latest report. Retain this CLI behavior using current
  Rust/frontend verification owners instead of old Python check implementations.
- `assemble frontend-info` inspects the actual served build. The mounted original
  `FrontendUpdateNotice` compares the current served build and protocol identity.
  `assemble rolling-restart` and `web/routes/runtime.py` expose status/request/wait;
  the original POSIX owner hands off the listener after quiescence and reconstructs
  eligible Agent Sessions. It does not transfer live provider process handles.
- Copied `AdminPanel` has no opener in either original or current production graph.
  It must be connected to confirmed diagnostic owners or removed in this phase;
  its lazy import, dead state and pollers are not a retained user flow by themselves.

The fourteen managed providers remain canonical. Freebuff and managed Antigravity
are excluded. External Antigravity remains a Room Connector participant and has no
app-managed login/usage/runtime operation. Voice, Mafia, RimWorld and the scripted
meeting runner are outside this phase. No new account, purchase, usage reset,
credential import, browser-cookie extraction or provider simulation is introduced.

## Authority, state and lifecycle

Provider registration owns advertised login and usage capabilities. Runtime
availability and operation support are separate facts. Existing catalog discovery
owns explicit refresh and publication; concurrent refreshes join one operation,
and shutdown cancels and waits for discovery. A failed refresh is visible rather
than silently publishing an old catalog as newly discovered. Selected attendee
discovery must remain scoped to its one explicit provider.

Local operator HTTP tickets and existing request-origin/body bounds protect
host operations. Remote admitted users cannot initiate host login, refresh or
account usage. Credentials remain in their existing provider store/environment;
only normalized quota/balance results reach the local operator. Provider-native
account IDs, tokens, raw diagnostics and authentication URLs containing secrets
must not enter room events or logs. Room participants retain ordinary room authority.

Browser OAuth login has one bounded process owner, explicit cancellation and
cleanup, and success only after the provider command completes successfully.
Interactive authentication reports launch separately from completion. Catalog
refresh follows successful authentication or an explicit operator action. Failure,
unsupported operation, missing executable, unavailable account and cancellation
remain distinct. No command is selected from caller-provided shell text.

Usage reads run on demand without a model turn. Native transport parsing and
cleanup reuse their existing provider owners where possible. Bound execution,
output and cancellation; coalesce concurrent identical reads rather than spawning
unowned work. Any retained cache must expose its observation time and stale/error
state. Missing or null quota values never mean zero usage or unlimited remaining.
Codex's multiple limit buckets and Claude's non-subscription case remain distinct;
DeepSeek monetary decimal strings retain their exact meaning and currency.

Runtime build identity and update state belong to the serving runtime, not a
frontend timer or a client-created release flag. Preserve assets used by admitted
clients while a replacement build becomes current. Restart requires quiescence,
preserves durable room/account/session configuration and bound address, and waits
for positive old-provider cleanup before recovery. Desktop parent/sidecar custody
must survive the transition; listener handoff cannot create an unowned replacement.
Busy, unsupported platform, missing build, readiness failure and uncertain cleanup
must be observable. Runtime stop is not a claimed successful rolling restart.

Resource reads use the existing maintained process library and bounded typed
projections, with observation time and unavailable metrics. Do not expose process
arguments, environment or user paths. Release health selects only fixed current
verification commands at their owning repository; no arbitrary remote shell or
new automated security scan. A report stores actual outcomes, including timeout,
failed and not-run, and failures to save are not reported as durable success.

## Phase dependency order and acceptance

1. Add operation capabilities to provider registration and explicit refresh to the
   catalog lifecycle. Wire one real local operator refresh path through the API and
   agent editor. Verify single-owner concurrency, cancellation and revision updates.
2. Connect supported login and normalized usage through the same authority boundary
   and visible creation/member controls. Cover every retained provider with static
   capability evidence; unavailable runtimes remain truthfully unavailable.
3. Complete runtime build/update/restart and resources/release-health owners and
   their retained CLI/HTTP/frontend entries. Resolve the copied Admin surface using
   these actual owners. Do not harden one provider while these targets are absent.
4. Verify the phase as a whole through direct packaged desktop/mobile manipulation
   with explicitly labeled local protocol fixtures. Exercise denied remote access,
   operation failure/cancellation, catalog refresh, usage rendering, diagnostics,
   restart/reconnect and update notice. Confirm exact app/child cleanup and remove
   only run-owned data/regenerable artifacts after work ends.
5. Run affected tests, builds and mandatory unchanged gates. Measure CPU, memory,
   process/task count, disk and latency at affected owners; optimize only concrete
   costs. Obtain Daybreak Blue `xhigh` approval for each commit, cumulative Phase 8,
   exact final HEAD and complete resulting local contract before Phase 9.

Final real runs remain configured DeepSeek, Codex, OpenCode, external Antigravity,
Grok and Cursor `auto` only. Claude and other providers receive static/native
contract and local fixture verification without actual account/provider execution.
Local Phase 8 acceptance does not claim the final authorized real-client matrix.

## Explicit catalog refresh implementation

Catalog discovery now retains one cancellable worker. A watch request/completion
generation coalesces simultaneous refreshes and publishes the actual new catalog
before acknowledging callers. Waiting has no timer or periodic work. Selected
attendee discovery still visits only its chosen registration; fixed fixture catalogs
explicitly reject refresh. Cancelled/failed workers return unavailable.

The local operator's one-use HTTP ticket protects the private refresh route, whose
response uses the existing catalog contract and no-store/Tauri-origin policy. The
agent creation dialog exposes a desktop refresh button; room catalog subscriptions
remain authoritative for updated choices. UI success follows a ready server result.

The existing catalog test verifies concurrent requests share one generation, selected
scope stays unchanged, and shutdown/fixed-catalog rejection. A real local TCP case
verifies authorization before body handling, crossed/consumed tickets, bounded body,
private CORS/cache behavior and published catalog equality. Both pass (0.01/0.04s).
Provider/server all-target/all-feature Clippy, 24 existing agent-editor tests,
frontend production build and unchanged CSS/architecture/growth/19-policy/format/diff
gates pass. The only new resident task is the existing discovery worker retained on
an event wait; no provider runtime is launched by the local fixture verification.
Packaged operation proof and whole-phase cost measurement remain pending.

## Browser OAuth login implementation

Codex, Claude, Grok and Cursor registrations now own their original login arguments;
the public optional `login_supported` flag is derived from those registrations.
API/local providers, Freebuff and managed Antigravity do not gain login authority.
OpenCode's separate interactive authentication entry is implemented below.

One login owner coalesces repeated requests for the same provider and retains the
bounded native command across response loss. Explicit cancellation and runtime
shutdown cancel and join it through the existing process-group/Job probe owner.
Its original ten-minute OAuth deadline remains; there is no periodic work. The
registry bounds retained login records, and no process starts at service construction.
Codex retains its existing configured home. No raw command output is published.

The local-operator login/cancel routes reuse the refresh transport boundary. Login
reports authenticated only after successful command exit and catalog refresh. If
authentication succeeds but refresh fails, the response explicitly reports that
distinction. The creation dialog displays pending state and an explicit cancel action.

A native local-command fixture verifies successful/failed exit, private diagnostic
suppression, unsupported providers and shutdown rejection (0.02s). The existing
three process cases verify sanitized environment, cancellation with positive
descendant cleanup, and explicit unconfirmed cleanup (0.03s). The TCP operation
boundary additionally rejects unauthenticated/unsupported login and distinguishes
no-active-login cancellation (0.05s). No real provider/account runs. Affected
Clippy, 34 existing frontend tests, generated types, frontend build, unchanged CSS
and mandatory gates pass; direct packaged login proof remains part of phase acceptance.

## On-demand usage and DeepSeek balance implementation

Provider registration now owns usage readers and the generated `usage_supported`
capability. The shared usage owner starts no resident task or periodic request:
explicit reads coalesce while running, preserve their observation timestamp, and
runtime shutdown cancels and joins them. Completed results are not a freshness cache.
A failed refresh clears the visible observation instead of relabeling old data.

The first reader uses DeepSeek's fixed `/user/balance` endpoint and the existing
credential store. The existing bounded HTTPS JSON read mechanism is shared with
catalog discovery; it retains HTTPS-only/no-redirect policy, eight-second deadline,
cancellation and bounded body handling. Balance responses have a 16 KiB cap and
retain exact decimal strings, currency and the provider's `is_available` result.
No model turn or credential/public diagnostic output is involved. Codex and Claude
structured usage readers remain subsequent targets; other providers stay explicit.

The private POST usage route consumes the same one-use local operator authority
before body decoding. The member detail resolves its provider from the current
catalog and offers an explicit desktop account-usage action; admitted browsers
receive an explanatory state and no host account operation. Responses and UI use
the generated domain projection, separate from room history and participant state.

Local verification passes: controlled concurrent read/shutdown and exact-decimal
projection cases (2 tests, 0.01s), the extended real-TCP operator boundary (0.03s),
27 frontend cases, existing catalog projection regression, generated bindings,
frontend build/CSS, provider/server all-target/all-feature Clippy and mandatory
gates. Native account/API execution and direct packaged observation remain pending.

## Claude structured quota implementation

Claude uses the already-pinned Agent SDK `0.3.258` public experimental usage method
(`sdk.d.ts`, `SDKControlGetUsageResponse`). The existing catalog inspection owner
now also runs the usage mode: an empty prompt queue, no tools/settings/MCP, bounded
ten-second process probe, private SDK staging and confirmed process cleanup. This
uses the supported typed method directly; an SDK failure is not replaced by terminal
scraping. No dependency version or actual Claude account/provider execution changes.

The bridge projects quota fields only, excluding session costs and transcript/behavior
metadata. Rust maps five-hour, weekly, model-specific and OAuth-app quota windows
into one generated rate-window projection. Null measurements and reset times remain
unknown; an account for which subscription limits are unavailable has a separate
state. This phase retains plan quota usage, not session-cost/behavior analysis or
purchase/extra-usage account management. The same operator member control renders
quota windows and exact DeepSeek balances without periodic refresh.

Eight existing/expanded SDK bridge fixtures, three Rust usage-owner/projection tests,
five frontend response/rendering cases, generated bindings, frontend build/CSS,
provider/server all-target/all-feature Clippy and unchanged mandatory gates pass.
The SDK fixture proves the actual bridge selects quota fields and exits; it does not
prove live Claude authentication or availability. Direct packaged phase acceptance
is pending; the Codex reader is implemented below.

## Codex account rate limits implementation

Codex now reads `account/rateLimits/read` through a temporary app-server, preserving
its configured home and process-local MCP isolation. The exchange only initializes
and reads account limits; it never starts/resumes a thread or sends a model turn.
The [native account contract](https://learn.chatgpt.com/docs/app-server#6-rate-limits-chatgpt)
selects the multi-bucket projection when present and the documented single-bucket
projection otherwise. Malformed multi-bucket data fails instead of recovering from
the other field. Missing measurements/reset times remain unknown, and private
account notifications and reset-credit metadata never enter the quota projection.

Room sessions and account inspection now share Codex's existing bounded JSON-line
wire owner. The existing process owner also supports a bounded private-pipe exchange;
it drains bounded stderr, cancels on shutdown and confirms the process tree has
ended before returning any outcome. There is no room portal, resident account
process, provider retry or periodic usage work. The existing room-session request,
turn, correlation and pending-notification authority remains with the session.

Five usage cases pass, including the real duplex native exchange and multi-bucket
projection; all 32 existing/affected Codex cases pass (77.83s), including turn
cancellation, durable resume and descendant cleanup. Four process cases pass (0.02s),
including exact inspection process absence after success and cancellation. Existing
provider/server Clippy and mandatory gates pass. No real Codex/account is invoked;
packaged phase and final real-provider evidence remain pending.

## Interactive login custody decision

OpenCode's original interactive login hands a graphical terminal to the operator;
its receipt means the native launch was accepted, not authentication or visible UI
completion. macOS uses Terminal's native scripting entry, Windows uses documented
`Start-Process` new-window launch, and Linux selects the first installed terminal
from the original four command definitions. A present terminal's failed launch is
not retried through another terminal. Shell command text is never supplied by HTTP.

Only the short-lived native launcher belongs to the app. It is bounded, cancelled
and reaped separately; the new terminal belongs to the operator and is not placed
inside an app-owned provider process group/Job. Linux transfers the terminal child
after the successful native spawn receipt, using Tokio's child-reaping support.
Closing the app or cancelling after handoff must not terminate that user terminal.
A lost/failed launcher receipt reports an unconfirmed handoff, never authenticated;
completion is observed by the operator's subsequent explicit catalog refresh.

OpenCode's terminal handoff is now wired through the registration, login owner,
operator HTTP response and creation dialog. macOS uses a constant AppleScript with
separately passed shell-quoted arguments; Windows uses a constant PowerShell script
with executable/argument data in the sanitized launch environment, following
[Start-Process's native new-window contract](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.management/start-process).
Linux retains the original installed-terminal selection and required GUI session
environment. No terminal or real provider was opened during local verification.

Local helper fixtures distinguish successful native receipt, unconfirmed receipt
and exact helper cleanup on cancellation (2 login tests, 0.01s). The actual frontend
API decoder/rendering path distinguishes started from authenticated (26 affected
frontend tests); HTTP operation regression (0.04s), frontend build/CSS, Clippy and
mandatory gates pass. Packaged terminal launch and cross-platform runtime evidence
remain phase acceptance work.

## Resource observation contract

The runtime owns one on-demand sysinfo sampler. It reads memory and process CPU,
never arguments, environment, executable paths or task lists. The original scope
remains the runtime, direct children and fixed related CLI/tool names; all displayed
names come from that fixed vocabulary. Rows are capped at thirty after sorting;
counts describe all matching processes. This is not a whole-machine process viewer.

CPU needs two native samples at least the library minimum interval apart. First
reads, new/reused PIDs and reads too close together expose unknown CPU, not zero.
Memory/load unavailable on the platform stays null. A single async mutex is acquired
before dispatching the blocking OS read, so there is no resident sampler, timer or
queue of blocking workers. A disconnected reader does not release the sampler while
its OS read still runs. Private GET authority is consumed before body validation.

Connect the desktop-only server-status opener, an initial resource read and explicit
refresh with observation time, unknown/error states and related-process rows. Remove
the copied unimplemented health pollers; the subsequent release-health owner will
restore that section with actual results. Verify native sampling privacy and first
sample semantics, operator HTTP denial, frontend refresh/error behavior, affected
builds and unchanged gates. Direct packaged proof remains whole-phase acceptance.

The resource sampler, private GET route and desktop server-status rail entry are
connected. Native first-sample/privacy verification passes (0.02s); the shared
real-TCP operator boundary verifies denial, one-use consumption and private
CORS/cache headers (0.04s). The frontend case verifies unknown CPU and removal of
stale observations after an explicit failed refresh. Generated types, production
build with the unchanged approved CSS cascade, all-target/all-feature Clippy and
mandatory architecture/growth/19-policy/format/diff gates pass. No resident resource
task or timer is added; the native initial sample took under 0.02s in the local test.
Full repeated-sample cost and packaged desktop/mobile evidence remain phase acceptance.

## Release-health execution contract

A fixed Rust-owned catalog replaces the old Python check implementations: frontend
build/tests, Rust check/tests, mandatory architecture/format gates and Git diff.
The explicit local `assemble release-health list/run` entry selects catalog IDs;
HTTP only reads catalog/latest report and cannot launch repository commands. The CLI
accepts an explicit repository and output root (default current checkout and the
server's default state directory). Packaged runtimes need no source checkout to
read a report. A missing latest report means not run; malformed/unreadable reports
fail visibly, and the report timestamp is not a claim about current source.

Reuse the existing bounded local-command owner with a working-directory argument:
fixed executable/arguments, sanitized build environment, null streams, owned native
process group/Job, per-check timeout and explicit cancellation/cleanup outcome. Do
not collect raw build/test output into the report or web API. Sequential checks
stop on cancellation or unconfirmed cleanup; remaining checks are not run.
Atomic latest-report replacement follows actual completion and sync; failure to
persist is a CLI error. A repository-local advisory lock prevents overlapping
release-health invocations from racing frontend artifacts. No polling or resident
worker is introduced. Verify a real CLI Git check/report read, invalid selection,
missing/corrupt report distinctions, process outcomes and private read-only UI.

The CLI now lists six current checks and runs exact selected IDs. A native built
`assemble release-health run --check git_diff` passed in 0.013s, wrote a matching
report and preserved it after rejected invalid selection. Missing/corrupt/failed
report projection passes its local case (0.01s). Seven existing stable-entry cases
pass (0.44s) after extracting their unchanged native launch/environment/stop owner;
release-health adds only an explicit working directory and pre-cancel rejection.
Generated types, all-target/all-feature Clippy, CLI build and unchanged mandatory
gates pass. No provider, account or remote publication ran; stable-entry tests use
local protocol fixtures. The private HTTP/report UI connection is the next slice.

The runtime's existing state-root constructor now binds report reads to the same
directory as its database/provider state. Private catalog and queue GETs consume
operator authority before body handling; reads use the bounded typed report owner
on the blocking pool. A runtime without a configured state root explicitly rejects
report reads. The server-status panel reads on opening and explicit refresh only,
shows last completion time, and distinguishes missing, failed/timed-out and unreadable
reports. Directly replaced Python-era label/schema helpers and their self-tests are
removed; the generated current contract owns status labels.

The actual TCP route verifies unauthenticated denial, private cache headers, missing
report null and corrupt-report 503 (0.05s). Two frontend cases cover resource refresh
and health missing/timeout/error rendering; production build with unchanged CSS,
Clippy and mandatory gates pass. No automatic command execution or polling is mounted.
Direct packaged whole-phase proof remains pending.

## Served frontend release custody

The executable currently serves mutable build output directly. Startup will instead
materialize a private, content-addressed copy beneath the runtime state directory,
then serve only that copy. Hash every relative file name and byte, reject links and
special files, and verify the source has not changed while copying. Reusing a release
requires verifying its actual contents; corruption fails startup. Publication uses
an atomic same-filesystem rename under a release-store lock. No background watcher,
new provider process or automatic release deletion is introduced. Retaining releases
is necessary for the following update/restart asset handoff; routing old build assets
and the runtime identity query remain separate integration work, not implied by a
snapshot alone. Startup readiness identifies the actual snapshot. Verify source
replacement leaves the served copy unchanged, repeated startup reuses the same
identity, and changed content or corrupted retained files are distinguished.

Startup now materializes that snapshot before listening or launching runtime
operations, and readiness includes its actual full-content build ID. The existing
TCP static-frontend case replaces the original HTML and deletes its asset after
server startup: all browser entrances still serve the retained build with unchanged
security/cache policy (0.04s). Identity reuse/change and retained corruption pass
(0.00s); all-target/all-feature server Clippy and unchanged mandatory gates pass.
Hashing uses an 8 KiB buffer and no resident task. Snapshot disk cost is one full
copy per distinct build, retained explicitly for the pending asset handoff. Whole
phase packaged/cost verification and the identity/update/restart connection remain
pending.

## Runtime version projection

Store the selected release as one immutable object in AppState, deriving both static
paths and build identity from it. The same-origin version read exposes only the
optional build ID and the protocol owner's version, with no state-directory paths or
provider data. A runtime without a frontend returns an explicit null build identity.
The retained `assemble frontend-info --server URL` entry queries this actual runtime
using the shared bounded HTTP JSON reader, without redirects, rather than guessing
which local source directory a process serves. Missing/unreachable/malformed server
responses fail visibly. This read adds no resident task. Browser update notice and
old-build asset routing follow this owner; the public version projection grants no
host operation authority.

AppState now accepts the verified release object, so serving paths and version
projection cannot be assigned independently. The private-no-store same-origin
version endpoint and generated schema are connected. The actual built CLI queries
the TCP fixture after source replacement and reports exactly the retained build and
protocol version (1.38s including CLI launch). Six ingress cases (0.19s) and the
existing guest recovery boundary pass with snapshot custody. Server Clippy and
unchanged mandatory gates pass. No provider/account runs or background observations
are introduced; browser baseline/update behavior remains pending.

## Browser build binding and retained assets

A served HTML page must carry its own build/protocol baseline, rather than treating
the first later version fetch as proof of the page already loaded. Rewrite the
snapshot's HTML in memory once using Cloudflare's maintained `lol_html` parser;
retain original release bytes for identity verification. Bind module/style asset
URLs to an explicit build-ID route and validate referenced files before serving.
The route addresses only that retained release's assets, never a search through
other versions or a mutable build directory. Existing static entrances retain their
ingress and cache policy. Old namespaced assets remain available after a new release
is selected, while missing/invalid build IDs fail. No periodic filesystem reads or
release deletion is introduced. Snapshot HTML transformation uses the same build ID
and protocol owner as the version API, with no inline script or CSP relaxation.

Browser update observation will compare against this embedded baseline, using the
original bounded version-check cadence and visibility/reconnection triggers. It
must expose unavailable checks rather than imply that an unknown server matches.
The native Tauri UI is packaged in the desktop executable and is not the server's
HTML page; its current protocol compatibility owner remains authoritative. The
browser reload notice applies to the actual HTTP-served page.

The served HTML now embeds its build/protocol identity and references only its
namespaced release assets. `lol_html` 3.0.1 handles attribute parsing/rewriting;
referenced missing assets fail snapshot preparation. The existing TCP static case
verifies the embedded baseline, unchanged security/cache headers and CLI identity,
then starts a replacement runtime/build and reads the prior build's exact asset
through its retained URL. An unknown release returns 404 (1.31s total). Missing
assets/corrupt releases pass the expanded snapshot case (0.01s); existing ingress,
guest recovery, surface registry, Clippy and mandatory gates pass. This proves asset
retention across runtime replacement, not rolling listener or provider handoff.
The browser notice and full packaged/rolling evidence remain pending.

The HTTP-served app now mounts the update notice after identity admission. Its
baseline comes from the loaded document; its expected protocol is generated from
the protocol crate at frontend build time. A changed first response is an update,
not a new baseline. Reads coalesce, have a five-second abort deadline, run at the
original fifteen-second cadence only while visible, and recheck on focus, visibility
and room connection changes. Unmount aborts the current read and clears observers;
a confirmed update stops them until the user's explicit reload. Failed/missing reads
show an unavailable notice with retry, never a fabricated matching version.

Three actual-fetch/React cases pass (57ms): first-read mismatch and stopped polling,
failed observation followed by explicit successful retry, compiled-protocol mismatch
and abort on unmount. Production frontend build and the unchanged CSS digest pass;
protocol exporter Clippy and mandatory gates pass. Operational type exports now
share their existing generator's scoped helper, without changing previous output.
No native Tauri polling is added. Direct desktop/mobile browser observation and
whole-phase cost measurement remain pending.

## Restart transition prerequisites

The current HTTP shutdown drops an in-flight connection immediately, which can lose
a durably accepted restart response. Preserve the existing absolute connection
lifetime while asking Hyper to finish admitted HTTP responses during shutdown; the
runtime's existing six-second connection-drain bound still owns the outer deadline.
Do not wait longer or weaken admission limits. Verify cancellation while a handler
is deterministically held, then release it and observe its actual response.

Login, usage and the room/catalog shutdown owners are independent after connection
drain. Start their cancellation/join paths together so one native cleanup does not
delay cancellation of the others. Continue checking every result and retain the
required reconciliation-before-room-cleanup order. No new retry, poller or provider
execution is introduced by this transition preparation.

The controlled TCP response case fails against the previous immediate-drop path
and passes with graceful shutdown. Five affected shutdown cases pass (0.88s),
including reconciliation failure and positive turn/lease cleanup. Server Clippy and
unchanged architecture, growth, policy, format and diff gates pass. Independent
cleanup futures now run concurrently after connection drain; their errors and
reconciliation order are preserved. This establishes response-drain behavior,
not rolling-restart completion or a measured whole-runtime latency improvement.

The desktop supervisor validates every readiness record against its original child
PID and first listener address. It forwards the initial record, consumes subsequent
matching records, and continues forwarding ordinary control responses. A changed
PID/address fails custody instead of entering the parent's control-response queue.
The existing stream test now covers repeated readiness plus the next control reply
and both identity changes (0.00s); desktop Clippy and mandatory gates pass. This is
control-stream verification; actual same-PID executable replacement remains pending.

## Atomic restart admission

After candidate preparation, the restart owner must check durable busy/lifecycle
authority and record quiescence in one SQLite transaction. Existing room command
admission and turn assignment must observe that same record in their own mutation
transactions. A concurrent launch/turn either precedes the restart and blocks it,
or follows quiescence and cannot cross the provider boundary. Committed command
replays remain readable. New room commands receive an unresolved, retryable result;
queued inputs remain durable and are not assigned during quiescence.

The existing private `runtime_metadata` map owns the latest restart operation,
source runtime generation and eligible managed session keys. No table migration or
second in-memory admission flag is needed. Missing metadata means no restart;
malformed metadata is an error. Retrying the same operation reads its receipt and
never starts a second transition. Only its source runtime may abort preparation;
an aborted operation is terminal and a new attempt requires a new operation ID.
Abort releases admission; the coordinator must wake retained room-floor work once.
External participants are not reconstructed as app-owned provider processes.

Acceptance uses real store/lifecycle transactions to verify busy refusal, receipt
replay, quiescent command/turn refusal, abort and subsequent retry, including a
reopened store. This admission slice does not yet expose restart success or launch
a replacement; executable handoff, recovery and readiness remain the next owners.

The shared non-lifecycle inspection, lifecycle reservation and floor scheduler now
read that transaction-owned record. Idle and paused managed session keys are retained;
active lifecycle/turn/recovery authority blocks preparation. Three restart cases pass
(0.05s), including a real queued room input that remains unassigned until abort,
committed launch replay, source-generation refusal after reopen and malformed state.
All 324 persistence tests pass (4.42s); persistence all-target/all-feature Clippy and
mandatory gates pass. No process, background task, timer or provider call is added.
Each affected admission reads one metadata row; full phase resource cost and the
coordinator's abort wakeup remain pending.


## Reconciled startup and control ownership

The server's readiness notification must run after durable ownership reconciliation
and before network admission. The serving owner accepts the notification future;
notification failure follows the same owned shutdown as listener failure. Tests
must observe recovered durable state at notification and verify that a failed
notification closes the listener. The main executable starts its control pipe only
after that notification succeeds and joins the control task after serving ends.
This removes the current premature `ready` record and detached control task. It
does not yet establish POSIX descriptor inheritance or restart session recovery.

The serving owner now awaits readiness after reconciliation and routes notification
failure through owned room/catalog/login/usage/ingress cleanup. The executable joins
its control task. A real attendee admission/connection case observes its old
connection invalidated inside readiness, then verifies notification failure and a
closed TCP listener. All 23 affected attendee/control-pipe/runtime boundary cases
pass (0.05s, 1.95s, 7.19s); server all-target/all-feature Clippy and mandatory gates
pass. The managed-ingress test's old plain-text HTML fixture now contains the real
asset entry required by the existing frontend-release owner. No added background
work or polling; readiness latency now includes the required startup reconciliation.
POSIX control-frame/descriptor handoff and full restart completion remain pending.


## Restart handoff and reconstruction ownership

The accepted operation progresses from quiescing to draining, then recovering and
completed. Only the source generation may begin draining, bound to the prepared
executable's content identity. A replacement must present that operation and image
identity after ordinary positive custody reconciliation. The replacement generation
then owns reconstruction; an older generation cannot release its barrier. Failed
reconstruction can become terminal only after positive cleanup has removed all
owned runtime authority. Unconfirmed cleanup retains the nonterminal barrier.

The durable restart target list grants only reconstruction of those exact managed
session keys. It does not manufacture human membership or a room command. Load the
positively stopped target, reserve through the existing provider supervisor, then
atomically persist the exact handles and `starting` state under the restart operation
before any provider launch. The existing provider confirmation validator and session
transition apply the actual result. Keep paused targets paused and keep floor
assignment blocked until every target is confirmed. Existing runtime reconciliation
already owns complete handles without a pending human lifecycle command; use that
path for interrupted reconstruction, without introducing a second custody store or
changing ordinary command reservation rules.

POSIX replacement will preserve the desktop-owned PID, process group, control pipe,
loopback listener and private CLI socket. Validate the staged executable's fixed
protocol/schema preflight before quiescence, finish admitted responses and owned
cleanup before exec, and report completed only after replacement reconstruction.
The original executable/frontend source paths remain the candidate sources for later
updates. Malformed inheritance, incompatible images, failed exec and uncertain
response loss must remain explicit errors or unknown outcomes; none may bind a new
address and claim a successful handoff. Local lifecycle tests precede real packaged
same-PID listener/reconnect proof. Real providers remain at the authorized final stage.

The persistence owner now binds drain to a candidate identity and reconstruction to
the replacement generation. It validates positively stopped targets before reserving
new custody, applies exact provider confirmations, preserves paused state, and keeps
admission blocked through recovery. Failure can release admission only after captured
custody is positively stopped. The interrupted-start case uses ordinary runtime
reconciliation with no fabricated human command reservation. Five restart scenarios
pass (0.11s), including wrong image/generation/target, pre-authorization confirmation,
incomplete completion, paused/idle restoration and cleanup before retry. All 326
persistence tests pass (4.42s); persistence Clippy and mandatory gates pass. These
are explicitly local store observations; executable replacement and provider startup
remain unverified until their runtime owner and packaged flow are connected.
