# Operational surfaces

Status: Phase 8 contract established after Phase 7 approval at `d70224b`.
Operational entries, affected local checks and direct packaged acceptance pass through
`60f4621`. Daybreak approved the usage correction, cumulative phase, exact `f776e2e`
and whole local contract at C0/H0/M0/L0. [Packaged evidence and limits](../VERIFICATION.md#phase-8-packaged-operations-and-update-2026-09-09)
supersede the pending execution notes below. Real-provider execution and both final
reviewers remain governed by the product plan's final closeout gate.

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
- Grok terminal-screen extraction and OpenCode Go HTML extraction were original
  usage entry points. Current structured native/API owners have since been found;
  the correction below restores those operations without reproducing their parsers.
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

### Agent-creation setup flow correction (2026-09-10)

The user rejects the permanent login, manual version inspection and duplicate help
controls added to agent creation. The requested flow identifies a login requirement
during creation, opens the provider-owned authentication flow and resumes the same
draft after confirmed completion. Version inspection belongs to that flow; a newer
release offers Update/Later without forcing an update. Update must reach the actual
supported updater, not an instruction link presented as an update action.

Keep the existing native provider, process custody and local-operator owners. A
browser user's setup must act on that user's computer; remote room authority must
not become authority to log in or update another computer. Provider selection,
creation input and pending operations remain scoped to the same draft. Cancellation,
failure and unconfirmed completion preserve the draft and cannot become success or
launch a different selected provider. No background update polling is required.

Latest user decision (2026-09-10) narrows discovery to the selected provider only.
Opening creation without a selection must not execute all provider probes or model
requests. Selection automatically reuses that provider's 24-hour cache or discovers
it when absent or expired. Retain one manual refresh action beside the provider
selection heading; it refreshes only the selected provider regardless of cache age.
Concurrent requests for the same provider share the in-progress discovery.
It does not replace automatic inspection. Reuse the surrounding visual style and
a compact icon with the existing 44px interaction area. Remove the wide button and
permanent explanatory paragraph. Display concise pending, success and failure
results in the existing small status style. Latest user scope decision (2026-09-10):
automatic and manual refresh target only the requesting user's own computer's CLI
installations and that user's connected API accounts. Joining a room must not route
refresh to the remote room host's or another user's catalog. The user's local runtime
owns discovery and cache freshness for those providers; room membership grants no
authority to refresh someone else's providers. Preserve the existing local-operator
authorization boundary.

Remove the whole-catalog HTTP refresh endpoint and its replaced callers, unused
helpers and obsolete tests, rather than leaving disabled or compatibility paths.
Authentication-required providers must not issue authenticated model requests before
authentication is available. Confirmed authentication need offers the provider-owned
login action; successful login refreshes only that provider. Cancellation and failure
preserve the selected provider and creation inputs. Providers supporting unauthenticated
local discovery remain usable without login. A generic discovery failure is not proof
that authentication is required.

Native process failure messages do not establish authentication state. The process
owner bounds and privately returns exit status and streams; the provider owner
interprets its documented authentication command. Codex checks `login status` with
the same executable and CODEX_HOME used for discovery before `debug models`.
Only its explicit no-login result requests authentication. Configuration, credential
store and transport failures remain failures and must not trigger a replacement login.
Cursor's `status --format json` must consistently report its credential state before
ACP model discovery; Claude's `auth status` JSON and exit code must agree before SDK
model inspection. These checks do not claim token freshness beyond the provider's
own status contract. OpenCode's public unauthenticated model discovery stays usable.
Creation starts login only for a local, new session with an explicit authentication
requirement; an unchanged requirement does not repeatedly open authentication.
Failure/cancellation leave a retry action and the draft in place. The existing local
login service owns the operation, and its successful refresh remains provider-scoped.

Native contract references: [Codex login implementation](https://github.com/openai/codex/blob/main/codex-rs/cli/src/login.rs),
[Claude CLI reference](https://code.claude.com/docs/en/cli-reference).
Cursor's installed `2026.08.11-e8db854` `src/commands/status.ts` implementation and
`status --help` establish its JSON field and exit semantics. Claude is verified with
controlled fixtures only under the approved real-provider scope.

A confirmed login's provider-specific catalog refresh belongs to the login service's
owned task, not its HTTP caller. Dropping the request cannot drop the completion
refresh. Native success followed by failed catalog discovery keeps the existing
`login_completed_catalog_unavailable` result; it must not be reported as a usable
login/catalog state. Cancellation still joins the owned operation and preserves
unconfirmed process custody. Interactive terminal launch remains Started, not
Authenticated, and cannot substitute for a completed authentication result.

Optional updates use the existing local update service. Selection reads versions
automatically for a registered update-capable installed provider, after discovery
and required login. No-newer-version results leave no permanent control. A newer
release offers Update/Later; Later has no installation effect. Unsupported local
installation methods cannot advertise an executable Update action.

Update rechecks the displayed offer before starting the provider's supported updater.
The local service owns the bounded updater process through completion/cancellation;
a terminal-launch receipt is no longer the completion contract. Fresh executable and
version observation must confirm the offered version (or a provably newer version)
before reporting completion. A check may join an already running update without
starting another. Failure, lost transport and unconfirmed cleanup are distinct from
success; retained custody cannot disappear on a repeated shutdown. A completed
update refreshes only the affected local provider's catalog. The creation draft stays
in place and cannot submit through an update it is currently awaiting.

Native update commands retain each CLI's installation owner. Codex's npm installation
is update-capable only when the globally configured npm prefix contains the exact
resolved Codex launcher; the command pins that prefix and the offered package version.
It does not install into another prefix or silently switch installation methods.
No real package update is part of agent verification without a separate user choice;
controlled updater fixtures cover installed-version, failure and concurrency outcomes.

Acceptance must exercise the complete packaged creation flow: already authenticated,
authentication required/completed/cancelled/failed, no update, Update/Later, updater
completion/failure, and draft preservation. Confirm desktop rendering and disclose
the separate deferred mobile acceptance. The earlier setup tests and manual review
cover their recorded mechanics only and do not establish this corrected acceptance.

#### Completed Pro review corrections (2026-09-10)

The completed `bfa62144` review identifies five owner/consumer gaps. Own-PC setup
must continue through selection and creation to that same computer's execution
owner using the existing external-admission/runtime contract. It must not mark
another room server's catalog ready, transfer credentials, or restore remote host
discovery authority. The current provider-only help link does not complete that flow.
Acceptance distinguishes a cold room host from a ready host whose catalog differs
from the requesting computer, and preserves the browser creation draft.

The creation destination is explicit. The existing room-host managed-session path
retains its room authority and stored-session re-add behavior. Own-computer creation
uses a companion attendee invitation issued by the current room-session authority,
including the paired operator's device- and origin-bound session. The HTTP boundary
selects the existing human or paired credential owner; issuance revalidates that
exact owner within its transaction. The invite retains the issuing session's
fingerprint, bounded lifetime and eight-companion limit. Admission and ongoing
attendee authorization resolve that stored fingerprint to exactly one durable
human-session or operator-pairing record, then revalidate its current room and
posting authority. Missing, ambiguous, corrupt, expired or revoked parents fail
closed; a rejected credential is never retried against another authority. This
uses the existing parent relationship and session owners without converting human
credentials, copying device secrets into packets or changing the storage schema.
Verification starts at real paired redemption, covers its actual browser API and
invite HTTP, wrong device/origin, exact replay and parent revocation before/after
admission, and preserves the existing human companion and native friend boundaries.
Its browser draft carries the selected provider and display name into a
bundled local creation window; it does not carry a host model, executable, workspace,
API credential or human session bearer. Only the invitation and expected room
incarnation cross the app handoff. Opening the window has no provider-launch effect.
The local window selects from its own canonical catalog and persona library, keeps
the normal authentication/update flow, and requires explicit creation with the
local workspace. The browser retains its draft until canonical room admission is
observed or the user dismisses it. A link-opening receipt is never creation success.

Reuse `RoomAttendeeClient`, `AttendeeRuntime` and the existing external socket and
cleanup protocol. The local server owns each creation task independently of an HTTP
waiter or window. One invitation retains one admission identity, chosen draft and
fresh dedicated adapter; repeated requests join that operation. Expected room ID,
room UID and provider must match admission before any local provider is launched.
Validate local catalog, workspace and persona selection before consuming the
invitation, under the authenticated local operator's selection authority. A rejected
local draft remains editable with its unused invitation. The attendee runtime binds
the actual server-issued participant ID after admission; the local draft's provisional
selection identity is never presented as remote membership authority.
Add without start retains admitted local custody until an explicit local start or
cancel. Running is reported only after the room acknowledges actual local readiness.
Remote room events remain authoritative for membership and external controls.
The pending create/start HTTP waiter must not disable the separate cancel action.
Cancel uses the same operation identity and waits for its real cleanup; it does not
abort the UI promise and infer stop. A later response from a superseded waiter cannot
replace the cancellation result. A missing/failed cancel remains unconfirmed with
explicit status and cancel access, never permission to submit a replacement draft.

Admission uncertainty retains the same client secret for explicit retry; a read
does not dispatch another admission or provider start. Cancellation joins admission
resolution and positive local stop before reporting remote cleanup. Unconfirmed
cleanup retains the client, runtime and terminal failure, including on repeated
shutdown; it cannot become a fresh operation. Normal application shutdown first
closes local creation and joins these owners while the listener, public ingress and
room authority remain available for exact cleanup, including self-targeted attendees.
Only then does it close ingress/connections and stop room owners. A real remote
cleanup failure still fails shutdown; no local shortcut or inferred receipt replaces
that protocol. Verification covers add-only and running self-targets without prior
cancel, fresh cleanup connections, and retained failure on a real rejected cleanup.
An attendee that never published a runtime identity already has host-owned canonical
removal completion. Its cleanup read must not issue an empty external stop delivery
that can become stale when that same host completes removal. The existing local
runtime owner still positively stops its actual process before leaving. Only an
assigned runtime identity requires the exact external stop report; ordinary leave
receipts and read failures remain authoritative, with no retry-on-rejection or
new completion receipt. After confirmed leave and a successful cleanup read with no
stop delivery, the positively stopped local owner releases its own lease artifacts.
A failed leave/read retains those artifacts. Verify the unassigned read before and
after host completion and preserve assigned-runtime report replay, rejection and
unresolved cleanup.
A normal attendee cancellation must not drop the provider factory's in-flight
launch future before it returns process custody or an exact safe launch failure.
Pass explicit cancellation through the existing adapter start and attachment owner:
reject a cancelled request before new launch effects, join an already-dispatched
factory launch, and cancel attachment/readiness after the driver is owned. This
adds no detached task or periodic retry. Unexpected task loss still retains the
existing unconfirmed launch custody; cancellation never invents a gone receipt.
Verify cancellation during the factory handoff and during held initialization,
then confirm real stop, departure and local lease release.
Process restart never automatically launches a replacement attendee
or treats an old invitation as an unused draft. Existing provider lease/guardian
recovery and server expiry remain authoritative after process loss. Before dispatch,
reserve the request ID and invitation fingerprint atomically in the local runtime's
existing metadata store, using the same store as local persona selection. The
receipt holds only room/request identity and an unresolved process-loss state; it
contains no bearer, client secret, credentials, paths or executable selection. A
new process reads this receipt as unresolved and cannot reserve the old request or
invitation again. This is a restart guard, not reconstructed live custody. Even a
previously completed operation remains unknown after losing its live owner; users
check the room and use a new invitation. Keep receipts with the runtime's data;
there is no polling, automatic deletion or schema conversion. No new room
database replica, remote catalog refresh, credential forwarding, polling bridge or
automatic installation is part of this correction.

Verification covers two independent catalogs, mismatched room incarnation, duplicate
handoff/create, dropped HTTP waiters, exact admission retry, add-only/start/cancel,
readiness acknowledgment and retained cleanup failure. Packaged checks exercise the
browser draft and bundled local creation with controlled providers and actual room
membership. Any unavailable OS/browser dispatch or real two-machine proof remains
explicitly unverified rather than inferred from a component test.

An update request losing its HTTP response does not establish terminal completion.
Retain the selected provider's creation guard through uncertainty and a read-only
check joining the existing task; release it only on confirmed terminal observation.
Do not relaunch the installer to resolve transport uncertainty or add polling.
Confirmed installation and failed selected-provider discovery are separate outcomes;
the independent setup window must display current readiness and permit a catalog-only
retry without repeating installation.

The provider discovery owner must retain cleanup-unconfirmed results across force
requests, cancellation and repeated shutdown. A subsequent successful probe cannot
erase unresolved process custody. Ordinary failures with confirmed cleanup remain
retryable. The public failed provider projection is evidence of the failure, not
a substitute for retaining the owner's shutdown result. Controlled failure-boundary
verification must establish no additional probe and persistent shutdown failure.

Remove superseded terminal-handoff/guide-only update descriptions from the current
contract when reconciling the updater implementation. Review findings and execution
status are recorded in the verification owner rather than treated as prior approval
of later changes.

### Browser-to-local provider setup (user request, 2026-09-09)

The agent creation dialog must keep unavailable providers selectable for diagnosis
without making them startable. It shows the catalog's actual failure and offers a
local setup link for providers with a supported native installation. The link opens
AgentsAssemble on the browser user's computer, not an operation on the room server.
It carries only a known provider ID: no room authority, credential, arbitrary URL,
path, shell command, or automatic login/update instruction.

The desktop deep-link owner validates the exact route and opens/reuses one bounded
setup window per provider. Cold and already-running launches use Tauri's deep-link
and single-instance owners. The existing startup identity boundary and local-operator
tickets still govern the bundled setup UI. Login and cancellation reuse the current
provider operation owner; catalog reads/refresh remain local to that runtime.
Version checks are read-only and on demand. An available release shows installed
and offered versions with Update and Later choices; Later preserves use of the
installed version. Only Update may hand off to a supported native updater, and its
launch receipt is not installation success. Unsupported installation methods expose
the fixed official instructions explicitly. Checks never execute an installer, and
ordinary discovery failure is not evidence of an available update. Provider-owned
update preferences are not rewritten. Native reads reuse bounded process custody;
public release requests have fixed endpoints, bounded bodies/time and cancellation.
Concurrent identical reads coalesce; shutdown joins owned work. No update polling,
compatibility fallback or version-based creation gate is introduced.

Opening the app/help is not authentication or update completion. The setup window
reports actual local catalog state; browser readiness remains its serving runtime's
canonical catalog. A different computer's setup cannot mark that server ready.
The browser explains that an installed desktop app is required and does not infer
launch success from a timer. Acceptance covers missing/auth-required selection,
strict deep-link rejection, local-only operation authority, cold/warm packaged
opening, login failure/cancellation, official-help opening, and desktop/390px layout.
Existing provider installations/accounts must not be updated merely to test this UI.

Version inspection covers Codex and Claude public npm releases, OpenCode's official
GitHub release, Cursor's native download service (using its configured channel),
and Grok's `update --check --json` contract. Grok, Cursor, OpenCode and Claude use
registration-owned native updater commands with bounded process custody and
post-install version confirmation. Codex supports an in-app update only when the
installed executable is verified to belong to the supported global npm installation;
other installation methods keep explicit official instructions. The app does not
guess a package manager or replace the configured release channel. Ollama and LM Studio expose
official app setup instructions and explicit unsupported version inspection.
Cursor's `get-channel` empty output means its native default `prod`; static/unknown
channels remain unsupported. No endpoint supplied by a browser or upstream response
is opened or executed; upstream version strings only populate validated display data.

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

## Historical whole-catalog refresh implementation (superseded 2026-09-10)

The following records the earlier implementation and its limited verification.
The agent-creation correction above replaces this whole-catalog HTTP mutation with
selected-provider discovery requested through the user's native private control pipe.
Its HTTP catalog endpoint only reads a snapshot or waits for an already requested
provider generation; it cannot initiate discovery. The private control response is
immediate so model discovery does not block other native control requests.
The selected-provider implementation at `e980eef7` passes the scoped packaged checks
recorded in VERIFICATION.md. Independent review and the remaining login/update flow
acceptance are pending. Its cache is process-local and uses a monotonic 24-hour age;
opening the selected provider's creation or execution settings requests that provider.
A forced request bypasses age and joins existing work for the same provider. There is
no periodic refresh task. Registration metadata is published without startup probes.
API catalog discovery checks required credential availability before external requests.
The first completed catalog initializes defaults; subsequent updates reconcile the
existing draft. Remote-room views cannot invoke discovery for the host's catalog.

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

### Final Pro correction: unconfirmed login cleanup ownership (2026-09-10)

Pro G3-M2 confirms that observing `CleanupUnconfirmed` made a login run replaceable:
the next login could overwrite that provider's record and make shutdown forget the
unresolved process custody. Keep the existing per-provider run for both a pending
result and an observed unconfirmed cleanup. Retry and cancel return that same
uncertainty until the owning service has actual cleanup evidence; the current owner
has no operation that can supply a later positive observation. Shutdown cancels and
joins retained results without discarding this evidence, including repeated calls.
Ordinary authentication failure and confirmed cancellation remain retryable.

Reuse the bounded registration/run map and shared task result, following the existing
usage/update owners' refusal to replace unconfirmed custody. No new state store,
process, timer, probe loop or success inference is introduced. Acceptance controls
a real login task failure mapped by its owner to unconfirmed cleanup, then verifies
no second launch, unchanged retry/cancel/shutdown outcomes, and ordinary failure
retry. The fixture does not imply observation of an actual orphaned provider process.

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
Completion follows the native task even when every response consumer disconnects.
A subsequent read joins completed custody before starting a new observation;
unconfirmed cleanup is retained as a failure and prevents replacement.

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

## Provider model freshness (2026-09-09)

The user requests current Harness/API model discovery while retaining OS-keyring
credential storage and optional software updates. The current discovery service
runs once at startup and on explicit local-operator refresh, publishes a revision
through the existing catalog subscription, and joins concurrent refresh requests.
Keep this owner and cadence; do not add timers or silently replace an agent's model.

Observed gaps: DeepSeek hardcodes two model IDs, obsolete price metadata and only
high/max effort despite the official low/high/max contract. OpenRouter's bounded
popular-only page can omit new models. Refresh UI reports success even when an
individual provider fails discovery. Native Codex/Claude/OpenCode/Cursor/Grok and
Ollama/LM Studio already discover from their actual installed provider; OpenCode
must additionally request its documented `models --refresh` to refresh its native
model cache during discovery. Other API
gateways fetch their public catalogs. Custom API retains explicit user model IDs.

DeepSeek must use its fixed authenticated HTTPS `/models` endpoint through the
existing bounded client and OS credential owner. Credential absence, denial,
malformed data, network failure and cancellation remain failures, with no static
or environment fallback. The model list supplies IDs, not pricing or capability
metadata: do not manufacture current prices/context limits for newly listed IDs.
The provider-owned Chat Completions request dialect supplies low/high/max and
thinking controls; discovery is not runtime certification of unreleased models.
Preserve the existing response-size/model-count/public-catalog limits.

OpenRouter combines its required bounded popular and newest tool-capable pages,
deduplicated by model ID; either failed read fails discovery. It remains a bounded
selection, not a claim to expose every historic upstream model. Other provider
model IDs remain discovered, without date/name allowlists. Keep per-provider
protocol differences with their owner and shared HTTP/projection/lifecycle common.

Acceptance: fixture catalogs containing new IDs are selectable after refresh;
removed IDs cannot be newly selected; failed reads never publish old lists as fresh.
DeepSeek's low effort reaches the request payload; unknown metadata is not shown
as current. Credential save and subsequent discovery outcomes remain distinct.
Run affected provider/server/frontend tests and mandatory gates, direct packaged
desktop/390px refresh flows, then the plan-owned manual review. Real provider runs
remain within the existing approved matrix; public documentation/catalog research
does not authorize new model turns or installation/account changes.

## User-requested secure-store interaction (2026-09-09)

The user authorizes keychain access and explicitly rejects a separate authorization
button. Existing native reads suppress authentication UI, so ordinary retries
cannot request approval. Keep the existing user operations: explicit catalog
refresh (including completion of a user-requested provider login), credential save,
and deletion may request the native OS dialog. Startup/status/runtime reads remain
noninteractive. The catalog generation owner scopes interactive credential access
to a user-requested discovery and retains the same credential semaphore/backend;
no secret or permission is cached. OS denial stays a typed failure. The blocking
native call retains its permit until the user completes/cancels the OS dialog even
if the requesting HTTP connection closes; no automatic retry or timeout reports
success. A one-time OS grant is not advertised as persistent access.
Acceptance covers unchanged operator authorization, interactive refresh versus
noninteractive startup/runtime, and the real packaged user-approved flow.

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


The Unix control pipe now needs a handoff-safe input owner: no blocking stdin worker
and no read-ahead across frames. Tokio `AsyncFd` owns readiness for pipes, sockets
and terminals; bounded regular-file/device reads preserve ordinary redirected stdin.
Restore the original descriptor flags before handoff and on drop, including a failed
readiness registration. Cancellation is accepted between frames. After the first
byte, the existing 4 KiB frame must finish within ten seconds and its response must
flush before exit; incomplete/oversized/stalled frames return errors and cannot
silently permit exec. Verify cancellation before input, cancellation mid-frame,
unconsumed next-frame bytes, deadline failure and restored descriptor flags.

Both controlled input cases pass (0.00s): a cancelled pending Unix read leaves the
next bytes untouched and restores flags; mid-frame cancellation finishes exactly
one frame, preserves the next frame and reports a stalled-frame deadline. All ten
real control-pipe process/TCP boundary cases pass (2.03s), including parent EOF and
writer-lease reacquisition. Server Clippy and mandatory gates pass. Unix input adds
one readiness registration and removes Tokio stdin's blocking worker/read-ahead;
one-byte syscalls are bounded by the existing 4 KiB control limit. Full packaged
resource/latency measurement remains pending.


The fixed `--runtime-preflight` entry reports the current handoff, protocol and
schema versions without opening runtime storage or providers. `RuntimeImage`
snapshots the selected executable, verifies source/copy content identity, retains
it under that identity and runs that exact image's bounded preflight. Invalid,
changing, corrupt or incompatible images return errors. Startup now explicitly owns
the Tokio runtime lifetime, while preserving both existing provider helper modes.

The actual server binary preflight creates no files in its isolated working directory;
its immutable copy runs successfully, a corrupted retained image and a non-executable
candidate are rejected. This case passes (3.51s); all ten real control-pipe cases pass
(2.09s), with server Clippy and mandatory gates. The measured debug image is
162,872,200 bytes; snapshots retain one copy per content identity and hashing uses
a bounded 64 KiB buffer. Candidate preparation occurs only on explicit restart,
not a periodic scan. Handoff triggering/listener inheritance and full packaged
acceptance remain pending.

Restart HTTP uses the existing one-use local-operator ticket. The private CLI socket
uses OS-user custody and a bounded one-command EOF frame; admitted responses finish
before shutdown. Its short per-user path supports long database paths, rejects active
or unresolved listeners, and removes only the exact owned socket inode. Terminal
receipts are retained under their operation IDs; only the latest record can own
admission. A repeated old request returns its immutable receipt without replacing
that owner. Candidate image/frontend preparation precedes durable quiescence; the
main process receives the prepared effect and owns drain, cleanup and replacement.
A runtime without that executor reports restart unsupported. Reconstruction uses
the captured session authority and existing provider supervisor, with floor wake-up
only after all targets are confirmed. Main-process handoff is the next integration.

The retained-receipt test and 20 affected restart cases pass (0.40s). Real HTTP
operator-ticket/replay/unsupported checks pass (0.07s), and the actual private socket
verifies status, active-listener refusal, inheritance identity and preservation of an
unrelated replacement file (0.04s). Server/persistence Clippy and mandatory gates
pass. These checks launch no provider; executable replacement is not yet exercised.

The executable now owns actual POSIX replacement. It consumes and validates inherited
listeners before constructing Tokio, checks the exact address and image identity,
reconciles old custody, reconstructs captured sessions and publishes readiness. Source
shutdown joins HTTP/room/provider owners, both control transports and the signal task;
only then records drain and drops the entire async runtime before exec. The desktop
supervisor passes the original executable source before its immutable launch binding.
The replacement loads the prepared frontend snapshot while retaining its original
source for future updates. Windows explicitly rejects inherited handoff arguments.
Apple does not implement SO_ACCEPTCONN: listener validation there uses socket type,
unconnected endpoint, exact address and durable operation/image identity; other Unix
platforms additionally check SO_ACCEPTCONN. No alternate bind path handles bad input.

Normal startup can terminate an abandoned older-generation restart only after positive
custody reconciliation. Startup notification failure can similarly record failure in
the replacement generation only after captured sessions are positively stopped; an
ordinary successful runtime shutdown does not rewrite a completed restart receipt.
The startup owner separates construction and handoff from readiness/control task
ownership, and boxes the combined serving future (measured 48,368 bytes by Clippy).

All twelve real control-pipe/process/TCP cases pass (5.52s), including two actual
same-PID replacements, original-source frontend updates, HTTP and private-CLI receipts,
retired receipt lookup, continued parent control and final writer-lease reacquisition.
Incomplete handoffs create no database or readiness output. Six persistence restart
cases pass (0.13s), including abandoned startup and positive cleanup after readiness
failure. Seven desktop supervisor cases pass (0.47s). Server/persistence Clippy and
mandatory gates pass. No provider was launched; packaged UI acceptance remains pending.

## Signed desktop supervisor and Keychain continuity

Final packaged verification found that the ad-hoc app/server signatures do not
retain a stable Keychain access identity across rebuilds. The stored provider key
still exists at the existing service/account; repeated approval is not key loss.
Developer ID signing exposed a second defect: copying the signed app's Mach-O
alone into a standalone supervisor path loses its signed Info.plist/bundle context.
The signed app passes strict verification, its standalone copy fails, and a signed
standalone server copy passes. A test executable is insufficient bundle evidence.

Live process signing inspection also found that debug packages selected the
repository's ad-hoc `target/debug` server before their signed bundled server.
The runtime must select its sibling external binary, which Tauri stages in both
development and packaged builds. A missing bundled binary is an error; source-tree,
environment override and resource-directory searches do not substitute authority.
Verify the running server's signing identity, not only the bundle on disk.

Package the existing desktop supervisor code as a separate standalone helper,
using the same authority and implementation. Bind that helper and the server from
opened files into private immutable stages. Preserve leases, exact child readiness,
process-group/Job custody, private control pipes, original restart source, and
positive cleanup semantics. The app bundle executable is not a standalone helper.
No unsigned retry, mutable-source launch, signature removal, Keychain ACL widening,
secret cache, or alternative credential source is permitted.

The macOS distribution entry `npm --prefix desktop run build:signed:macos` requires
`APPLE_SIGNING_IDENTITY` set to the publisher's Developer ID. Tauri signs helper and
server with their stable executable-name identifiers before signing the app. Keep
the same identifiers and signing team across updates. This distribution entry
fails explicitly without a signing identity. Ordinary source tests remain independent of a
developer certificate. Acceptance includes a signed-bundle regression, direct
packaged app-to-helper-to-server startup, stored-key access after initial approval,
and another signed build with the same requirements without key re-entry or renewed
access approval. Passing codesign alone is not startup or Keychain continuity proof.

## Operator restart entry points

`assemble rolling-restart --database PATH [--wait SECONDS] [--json]` preserves the
original command and busy-wait behavior using the same-user control endpoint instead
of the retired host-token transport. The wait also observes the accepted operation's
terminal receipt within its remaining budget. `--status [--operation-id UUID]` reads
latest or retained results; explicit `--operation-id` reuses an uncertain request.
Missing responses and elapsed waits remain unknown, with the operation ID printed
before dispatch. Failed/aborted receipts never produce a successful completion exit.
No provider turn or public credential transport is introduced by the CLI.

The desktop's Server Status panel now exposes restart and result lookup. Generated
Rust receipt types drive the UI. An accepted or lost response starts observation of
that exact operation: one request at a time, one second between observations, at
most sixty seconds, cancelled on unmount. Initial idle status is read once, and
terminal results stop observation. Missing outcomes remain explicit; a manual retry
reuses the same request ID. Request dispatch has a thirty-second deadline. There is
no permanent restart poller or optimistic completion state. Button sizes and wrapping
reuse the approved CSS cascade. React review retains event-owned mutation, bounded
cancelled effects and derived phase presentation without a second restart authority.

The actual binary CLI `--wait --json` case passes through in-place replacement and
terminal receipt (6.65s). Three frontend cases pass (0.73s), including lost response,
exact-ID recovery, deadline stop and same-ID retry. Production frontend build and
unchanged CSS, generated bindings, server Clippy and mandatory gates pass. Direct
packaged desktop/mobile operation remains the next whole-phase acceptance step.

## Remaining usage contract correction

The complete original registry proves Grok and OpenCode usage are independent retained
entry points, so their earlier provisional unsupported classification is insufficient.
Current official sources now expose structured owners: Grok's `x.ai/billing` ACP
extension (`xai-org/grok-build`, `extensions/billing.rs`) and OpenCode's
`GET https://opencode.ai/zen/go/v1/usage` (`anomalyco/opencode`, console route).
Restore both through the existing on-demand usage service, not terminal/HTML parsing.

Grok account inspection initializes the existing ACP client, requests billing without
creating a session/turn/tools, and positively cleans up its bounded native process.
Authentication remains inside the CLI through the existing auth-path owner; a private
inspection home avoids loading unrelated room configuration. Project only native
usage percentage and period reset, keeping absent measurements unknown. OpenCode
uses its explicitly configured native Go API credential and fixed HTTPS endpoint,
without browser-cookie import or account/payment mutation. Missing native auth,
unsupported server response and malformed data remain explicit errors. No real
provider/account execution occurs until the authorized final matrix.


Grok now uses the [official billing extension](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-shell/src/extensions/billing.rs),
including its current percentage/period and documented monthly-budget projections.
Malformed current fields never switch to the other shape. Native missing measurements
stay unknown; zero cents follows the native proto3 empty-object contract, and only a
ratio is projected. Subscription/account metadata, credit purchases and automatic
payment rules are excluded. The existing ACP client owns framing, initialization,
permission rejection and shutdown; the native inspection owner bounds the process to
ten seconds and confirms cleanup. Two local cases pass (0.01s): exact sessionless
billing exchange and native schema/unknown/error/privacy projections. Provider Clippy
and mandatory gates pass. Actual Grok billing remains at the final authorized stage.

OpenCode Go now uses the [official usage route](https://github.com/anomalyco/opencode/blob/dev/packages/console/app/src/routes/zen/go/v1/usage.ts)
with its native `rolling`, `weekly`, and `monthly` percentages and absolute reset
timestamps. Monthly duration remains unknown rather than assuming thirty days.
The [native authentication owner](https://github.com/anomalyco/opencode/blob/dev/packages/opencode/src/auth/index.ts)
and [Go provider definition](https://github.com/anomalyco/models.dev/blob/dev/providers/opencode-go/provider.toml)
identify `OPENCODE_API_KEY`, then `OPENCODE_AUTH_CONTENT` or XDG
`opencode/auth.json`'s exact `opencode-go` API key. This is the existing Go `/connect`
authentication flow; old browser cookies are not automatically imported or converted.
Invalid explicit content fails instead of switching credential sources. The file
read is bounded to 1 MiB and eight seconds; fixed HTTPS is bounded to 16 KiB/eight
seconds with redirects disabled and cancellation observed. HTTP account rejection,
missing credentials and malformed measurements remain typed failures. No auth
content or response body reaches diagnostics, persistence or frontend state.
Two local projection/auth-file cases pass (0.01s), provider Clippy passes (4.37s),
and unchanged mandatory gates pass. Real Go account execution remains pending.

## Complete retained provider operations matrix

Original `d504647` owners are `providers/launch_specs.py` for login commands and
`providers/provider_usage.py::default_provider_usage_registry` for account readers.
Current `registration.rs` plus `ollama.rs`/`lm_studio.rs` own all fourteen capability
records; `ProviderCatalogService` owns explicit refresh for every discovery entry.
Unsupported means no original retained account operation, not missing installation.

| Provider | Local login | Account usage owner |
| --- | --- | --- |
| Codex | `codex login`, browser OAuth | `codex_usage.rs`, native app-server rate limits |
| Claude | `claude auth login`, browser OAuth | `claude_usage.rs`, native SDK usage |
| OpenCode | `opencode auth login`, operator terminal | `opencode_usage.rs`, Go quota HTTPS |
| Cursor | `cursor-agent login`, browser OAuth | Unsupported |
| Grok | `grok login`, browser OAuth | `grok_usage.rs`, native ACP billing |
| DeepSeek | Existing API credential settings | `deepseek_usage.rs`, balance HTTPS |
| Cerebras | Existing API credential settings | Unsupported |
| OpenRouter | Existing API credential settings | Unsupported |
| Vercel AI Gateway | Existing API credential settings | Unsupported |
| LLM Gateway | Existing API credential settings | Unsupported |
| TokenRouter | Existing API credential settings | Unsupported |
| Custom API | Existing API credential settings | Unsupported |
| Ollama | No account login | Unsupported |
| LM Studio | No account login | Unsupported |

Freebuff and app-managed Antigravity remain excluded by user scope. External
Antigravity's Room Connector admission is independent of this account matrix.
Whole-frontend verification found four failures from old fixed test-surface digests
after the Phase 8 product revision changed. Both fixture digests now match revision
13; production integrity checking is unchanged. All thirteen affected tests pass
(0.60s); the other 797 tests passed during the full run.

Whole Rust verification also exposed an old static-route fixture missing the now
explicit private `/app/index.html` entry, and a recovery-publication fixture that
did not join the existing reconciliation-test lock while sharing the bounded
filesystem authority pool. Both fixtures are corrected; the production route and
four-worker admission gate are unchanged. The 105 server unit tests and all 866
workspace unit/integration tests pass, as do 28 desktop tests and workspace Clippy.
Windows compiled the full affected targets but rejected an unnecessary mutable
binding used only by POSIX restart. That binding is now POSIX-scoped; CI recheck is
pending. Direct packaged login cancellation exposed the English cancelled response
overwriting its Korean receipt; presentation now maps typed login errors, and both
reply orders pass with the same confirmed cancellation text (four component tests).
Frontend build and unchanged CSS plus mandatory gates pass after the correction.

Direct 390×480 packaged inspection found that mobile member detail omitted the
shared usage component mounted by desktop. The existing mobile detail owner now
passes its one selected provider to both session controls and `MemberUsage`;
there is no separate quota state, request policy, timer or authorization path.
Eight affected mobile/usage tests, frontend build/CSS and mandatory gates pass.
Packaged re-verification of this connection is pending.

The second Windows lint pass reached the CLI's unsupported-platform branch and
rejected an async function without awaits. It now returns an already-ready error
future, preserving the CLI's common call contract and explicit unsupported result.
Server Clippy and mandatory gates pass; the next Windows run verifies that branch.

## Claude catalog inspection workspace (2026-09-10)

Packaged catalog refresh was observed to launch installed Claude Code, which macOS
attributed as accessing MediaLibrary. The SDK's metadata calls initialize that CLI
even with an empty input stream. The old inspection omitted `cwd` and inherited
the desktop/server launch directory; metadata inspection has no selected user
workspace. Catalog and usage inspection must explicitly use the existing private
SDK stage as cwd. That stage's current Rust owner retains and removes it after the
probe. Managed sessions retain their explicitly selected workspace. No new temp
owner, provider execution mode, permission suppression or alternate catalog is
introduced. Acceptance is deterministic SDK-option verification from a different
caller directory; whether this eliminates the native music prompt remains unknown
without independent CLI access evidence.
