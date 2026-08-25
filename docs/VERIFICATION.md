# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

The active comparison baseline is original
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust
`6de2671848b951fb16dc13bb2dd2dfeb25c1e88f`. Local uncommitted behavior
is never described as public completion. Every completed-slice evidence entry must
name the tested Rust commit, original provenance commit, platform/build, exact
entry point and command flow, viewer identity, provider/model where applicable,
owned process/session identity, restart step, cleanup result, and failure if any.
Historical real-provider observations without that complete provenance remain
historical observations and must be rerun on the completed public slice commit.

`make verify` regenerates TypeScript protocol bindings from the Rust owner, builds the React production bundle, runs the socket-client tests, verifies the isolated Tauri shell and bundled sidecar input, and then runs the Rust architecture, source-growth, formatting, check, Clippy, and test gates.

The sidecar boundary tests close the parent control pipe and prove the process exits and releases its SQLite writer lease for a restart. A watchdog regression test suspends an owned process before closing the independent parent control pipe and proves the stopped process group is force-killed. Desktop real-flow verification separately kills the owning Tauri process while its live sidecar is suspended to prove the packaged watchdog cleanup, then suspends a live sidecar with its parent active to prove unhealthy-child replacement; only the exact processes created by that verification may be signalled.

Server boundary tests also prove that pre-authentication HTTP admission is bounded, incomplete headers expire, standalone static assets carry CSP and browser hardening headers, binary WebSocket ingress is rejected, and command-line help exposes no host-secret argument or environment path.

## Agent Session contract evidence

The active slice is not complete until one public commit reproduces all of these
boundaries:

- owner and non-owner snapshot, live, catch-up, reconnect, and resync apply one
  viewer policy, including contiguous `event_hidden` sequence;
- public ACK/result/error payloads exclude private runtime and provider data;
- `agent.create(start=false)` creates one stopped session, while
  `agent.create(start=true)` sends one client command and one command reservation
  owns creation plus optional start without client start/resync orchestration;
- same-payload replay, conflicting-payload rejection, ACK-loss replay, crash between
  create reservation and lifecycle preparation, launch ambiguity/adoption, exact
  generation-bound stop, confirmed cleanup, and restart reconciliation behave as
  specified;
- the observable create result retains the original created-session, participant,
  and optional-start fields, and its success/partial/uncertain meanings match the
  protocol contract;
- tests permit only the documented post-commit ACK/event orderings and do not make
  client optimistic state authoritative;
- provider conversation reuse is distinct from process reuse.

## Frontend provenance and parity evidence

The frontend reference is original commit `d504647…`. Each verified Rust commit
records a Rust-only change allowlist. Allowed differences are runtime bootstrap,
ticket/transport, the Tauri native boundary, and behavior-preserving source splits;
controller command decomposition or client-owned product state is a parity failure.

At fixed desktop and responsive viewports, compare asset identity, selector/class,
component and rendered DOM order, responsive breakpoints, left/right panel widths,
central chat bounds, composer bounds, and left-bottom profile-card position and
overlap. Screenshots support, but do not replace, geometry assertions. Exercise
create stopped, create-and-start, re-add, stop, resume/restart, reconnect, and one
provider reply through the copied controls. A hidden fake, no-op, or fallback is a
failed run.

## Frontend real-flow cleanup

When Computer Use is used for frontend verification, every resource created solely for that verification is shut down after its evidence is collected:

- controlled browser tabs and windows;
- test-only desktop application instances;
- local runtime/server processes started by the verification;
- test-only Agent Sessions and provider processes.

Cleanup resolves exact owned process and session identities before stopping them. It never closes user-owned tabs, applications, providers, or unrelated processes. Cleanup failure is reported and is not treated as a clean run.

## Admission, public ingress, and destructive-flow evidence

The future admission/public-ingress slice is not complete from HTTP route tests or
an opened screen. Its published Rust commit must exercise the copied production
frontend against a disposable authority and prove:

- separate normal-human and read-only invites, including server-derived scope,
  token removal from browser history, canonical roster projection, an initial
  authenticated WebSocket snapshot, permitted normal posting, and visibly denied
  read-only posting;
- one external-AI invite consumed by an actual supported RoomConnector app or CLI
  session, one acknowledged WebSocket publication, host-timeline projection, and
  exact leave/revocation cleanup; a mock connector or an unavailable plugin remains
  failed or `unknown`;
- operator pairing at the exact public origin, canonical operator projection,
  secret removal from history, first-use success, and isolated-browser replay
  rejection;
- owned public-tunnel startup, public React load, WebSocket operation, stop, exact
  cloudflared cleanup, retired-origin failure, and stable-entry health reporting a
  null target before the owning runtime exits. Cleanup is failed if the stable
  entry still redirects to a dead quick-tunnel origin;
- explicit failure classification: HTTP admission followed by a permanently
  unready room socket is a realtime defect, never a read-only pass or degraded
  success.

The later moderation/destructive slice must use disposable messages, participants,
and rooms to prove the visible confirmation gate and the server result: message
tombstone, kicked roster/session revocation, exact-name room deletion, removal from
the host directory, and immediate failure of already connected public sessions.
Only verification-owned data may be destroyed.

## Real Agent matrix

Frontend flows that require real Agent Sessions use exactly this matrix:

- Codex: Terra;
- Antigravity: Flash;
- OpenCode: the free Hy3 model.

The verification records the exact provider/model identifiers exposed by the installed runtime at execution time. Missing login, unavailable capability, unsupported model, or provider failure remains visible as failed or `unknown`; it never triggers model substitution, a mock pass, or a fallback provider.

Provider credentials, private conversation state, hidden reasoning, and provider-private identifiers are excluded from screenshots, logs, fixtures, public events, and committed artifacts.

## Published macOS human-profile evidence: `b0c55f6`

On 2026-08-24, public Rust commit
`b0c55f6fde01d004954458fa54178bd06fce4aab` was compared with original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` for the reachable local-operator
profile flow. The exact packaged debug application at
`desktop/src-tauri/target/debug/bundle/macos/AgentsAssemble.app` was driven with
Computer Use as the local operator. Saving `Profile Head E2E` through the copied
settings UI changed the left-bottom profile, the room roster, and existing human
message attribution from the same canonical participant projection. Room role,
join state, mute, permissions, and every Agent Session profile remained separate.

Opening the left-bottom profile card and then Agent Add closed the card and left
the whole profile bar below the modal backdrop; no profile surface painted above
the dialog. A normal application quit removed the exact Tauri and Rust sidecar
processes. Relaunching the same bundle restored `Profile Head E2E` after the
authenticated runtime profile synchronization, proving that the value came from
SQLite rather than the initial React default. The profile was then restored to
`SeiNel`, the application was quit normally, and final app/process inspection
found no verification-owned AgentsAssemble or `agentsassemble-server` process.

The exact public code passed `make verify`: mandatory architecture and source-
growth gates, generated bindings, the production frontend build, original CSS
and cascade verification, 66 Vitest files with 334 tests, 13 Tauri tests, every
Rust workspace test, warning-denied Clippy, and `git diff --check`. A separate
warning-denied `x86_64-pc-windows-gnu` workspace all-target/all-feature check also
passed. The authenticated HTTP boundary test exercised fresh one-use tickets,
atomic multi-room profile projection, safe raster avatar upload/read, CORS and
body limits, event delivery, restart persistence, and preservation of room-owned
fields. No real provider was started because this profile flow does not create or
run an Agent Session.

## Published profile hardening evidence: `a9c9630`

Public Rust commit `33ee7dcc8e3c5301b5bb487299cc170d432d57ff`
routes every room event producer through one room-owned durable sequence publisher.
The deterministic boundary test commits a withheld profile event N+1 and then a
normal room command N+2, drains one durable cursor, and observes N+1 then N+2 with
the same snapshot cursor. WebSocket delivery suppresses snapshot/publication
overlap duplicates and closes for resynchronization on any sequence gap.

The same commit validates declared avatar types against bytes, decodes under
allocation, dimension, pixel, and concurrency limits, and re-encodes one static
PNG. New blobs remain non-public pending capabilities until the profile transaction
binds them; replacement deletes the previous bound blob atomically, expired
pending uploads are collected and excluded from quota, and served avatars use
`private, no-store`. The active runtime now accepts only the complete current
schema and does not convert or expose older attachment records.

Commit `a9c9630ae3db197594b654b7c789ace13618dcb2` removes the copied React default
as an initial profile authority. The exact packaged debug application first showed
`프로필 불러오는 중`, then displayed the durable server value `SeiNel`; it never
presented a copied identity while the authenticated read was pending. The Agent Add
dialog remained above the left-bottom profile surface, and the room participant,
existing human message attribution, and left-bottom profile all showed `SeiNel`.
The prior temporary verification name was absent from both the final UI and SQLite.
Normal quit removed the exact packaged Tauri process and both owned runtime
processes; final process inspection found no verification-owned resource.

The exact public code passed `make verify`: mandatory architecture/source-growth
gates, generated bindings, production frontend and original CSS/cascade checks,
66 Vitest files with 335 tests, 13 Tauri tests, all Rust workspace tests,
warning-denied Clippy, and `git diff --check`. The warning-denied
`x86_64-pc-windows-gnu` workspace all-target/all-feature check passed with the
rustup stable compiler and target libraries explicitly paired. No provider was
started because this profile and publication verification does not require an
Agent Session.

## Published room-directory and creation evidence: `6624e51`

On 2026-08-24, public Rust commit
`6624e51edbd71c450497c41812eab23bb0e74770` was compared with original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` for the packaged local-operator room
directory and create-room flow. The exact release application at
`desktop/src-tauri/target/release/bundle/macos/AgentsAssemble.app` was driven with
Computer Use. Its first paint marked the cached directory as awaiting server
confirmation; the authenticated Rust response then supplied the real durable
`general` room and human profile instead of a client-fabricated room.

The copied plus control created one canonical room through `POST /api/rooms`,
entered it through a separately scoped WebSocket ticket, and published
`Rust room directory packaged-flow verification` through the room composer. A
normal quit removed the exact desktop, supervisor, and sidecar processes. The
same release bundle was relaunched, confirmed the same stable server and room
identities from SQLite, and displayed both the created room and its durable
message through a new WebSocket connection. Opening Agent Add again showed the
left-bottom profile surface below the modal backdrop. Final normal quit left no
AgentsAssemble process or SQLite handle owned by the verification.

The exact public code passed `make verify`: mandatory architecture and 800-line
source-growth gates, generated TypeScript bindings, the production frontend build
and original CSS/cascade verification, 66 Vitest files with 339 tests, 14 Tauri
tests, every Rust workspace test, warning-denied Clippy, and `git diff --check`.
Purpose-separated ticket, authentication-before-body, CORS, atomic creation,
idempotent room UID/event behavior, stable database identity, corruption rollback,
and immediate WebSocket admission were exercised at their public boundaries. The
full workspace and Tauri shell also passed warning-denied all-target/all-feature
`x86_64-pc-windows-gnu` source checks with the installed rustup compiler and target
libraries explicitly paired. No provider was started because room directory and
creation do not create or run an Agent Session. The later correction and review
evidence below closes the critical-review condition; it does not retroactively
make this first public revision crash-consistent.

The subsequent critical web review returned `REVISE` with one reproducible
crash-consistency blocker in public commit `6624e51`: schema/`server_id` and the
initial room/profile were committed in separate transactions gated by the
process-local file-creation boolean. Death after file creation could leave an
unowned empty file, while death after schema commit could leave a valid v9
authority with no room/profile that every restart permanently skipped. The
normal shutdown/restart evidence above did not cover either crash window.

Public correction `6568810` moves fresh schema, server identity, room/settings,
publication cursor, participant, and profile into one SQLite transaction. It
also treats an existing SQLite file with no user schema as an interrupted empty
authority only after the normal exclusive path and file-identity checks, and
repairs the older valid schema-only state only when both room and profile
authority are empty, preserving the committed `server_id`. Four deterministic
tests prove retry from an interrupted empty file, the fresh all-in-one state,
prior schema-only recovery, and rollback of every product row after an injected
profile insert failure before a successful retry. The actual server control-pipe
boundary additionally starts from a fresh path, shuts down normally, verifies
the durable `general` directory and
`server_id`, restarts through the same production bootstrap entry point, and
proves both identities remain unchanged.

The corrected source passed `make verify`: all mandatory architecture and
800-line source-growth gates, generated bindings, production frontend and
original CSS/cascade checks, 66 Vitest files with 339 tests, 14 Tauri tests,
every Rust workspace test (including 82 persistence tests), warning-denied
Clippy, and `git diff --check`. Both the workspace and Tauri shell passed their
warning-denied all-target/all-feature `x86_64-pc-windows-gnu` source checks with
the installed stable compiler and target libraries explicitly paired. The exact
release bundle was then rebuilt and driven with Computer Use: it recovered the
durable `general` room, profile `SeiNel`, historical messages, and Agent Session
projections; opening Agent Add showed the left-bottom profile surface below the
modal backdrop. Normal quit removed the exact app and its owned Rust sidecar.
No provider was started because bootstrap recovery does not require an Agent
Session. The same GPT-5 Pro critical-review session re-read public correction
`6568810`, the original `d504647` baseline, and the earlier `6624e51` slice at
very-high reasoning. Its manual parity and security cross-check examined the
fresh transaction boundary, empty-file authority classification, schema-only
repair scope and `server_id` preservation, completed-authority protection,
pre-mutation input validation, cancellation/crash retry, and the production
entry point. It returned `APPROVE` with no remaining blocker. This web review
also served as the user-authorized manual-security cross-check because the same
Daybreak review task twice terminated without producing a review body; no
additional Deep Scan or automated scanner was run.

## Published macOS evidence: `99165dd`

On 2026-08-24, the packaged release candidate that became Rust commit
`99165dd621c6cde81e62324d0c418df9b40fc3ea` was compared with original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f` on macOS through the copied React UI
inside the bundled Tauri application. The production source was unchanged between
that real-client run and publication; the only later pre-commit change was a
test-build-only serialization guard for the process-global filesystem-authority
pool plus documentation. `make verify` then passed again on the exact published
commit: architecture/source-growth policy gates, generated protocol bindings,
frontend production build, original CSS/cascade verification, 65 Vitest files with
332 tests, Tauri checks with 13 tests, all Rust workspace tests, Clippy with warnings
denied, and `git diff --check`.

The real-client flow used the local owner identity and the native directory picker
for the repository workspace. Each provider was added with the original
"추가하고 바로 실행" control, then addressed through the room composer:

| Provider/model | Observed result |
| --- | --- |
| Codex `gpt-5.6-terra` | One persistent `app-server --stdio` session returned `CODEX_TERRA_CREATE_START_OK`; the durable room order was source message, turn start, provider final, turn finish, then idle session state. |
| Antigravity `gemini-3.6-flash` | One persistent native PTY session returned `ANTIGRAVITY_FLASH_CREATE_START_OK`; no print/one-shot path was used. |
| OpenCode `opencode/hy3-free` | One persistent loopback HTTP/SSE session returned `OPENCODE_HY3_CREATE_START_OK`. After normal application shutdown and relaunch, the UI `재개` control retained the same private provider-session identity, set `provider_session_reused=true`, and returned `OPENCODE_HY3_REUSE_OK` on turn two. The UI `중지` control then reached detached/stopped with no provider process left. |

Application shutdown removed the exact verification-owned desktop, sidecar,
provider supervisor, and provider processes. The final process inspection found no
verification-owned AgentsAssemble, Antigravity, or OpenCode process; existing
ChatGPT/Codex application processes were identified as user-owned and left alone.
Provider-private session identifiers remain in the local evidence database and are
intentionally not copied into this document.

The copied frontend CSS chunks remained byte-identical under the provenance gate.
The global overlay root was moved to a body portal with an explicit stacking
context, without changing the original CSS cascade. Opening the left-bottom human
profile and then the Agent Add dialog confirmed that the profile card closes and
the profile bar, left rail, right member panel, and central chat remain below the
modal rather than painting over it.

One explicit provider edge remains visible: a newly created Codex thread with no
completed turn was shut down before its first turn, and the installed app-server
later rejected `thread/resume` because no rollout existed. The Rust runtime kept
the original thread identity, surfaced `provider_request_rejected` with recovery
required, and did not create a replacement thread or invoke a fallback. A Codex
session with one completed turn passed the normal create-and-start flow. This
zero-turn provider limitation is not reported as successful restart parity.

## Published Windows transport and helper-binding evidence: `11e9b85`

Rust commit `6bfe73ed2c080eb36d942674fa1de3f04a2584a1` replaces the previously
unimplemented Windows Antigravity branch with one managed, resident system-ConPTY
session. The selected executable remains byte-bound and open without write/delete
sharing; process creation is suspended until the root is assigned to a
kill-on-close Job Object. The same common Antigravity transcript, session identity,
permission, room-portal, turn, and failure logic is used on Unix and Windows. The
Windows driver does not invoke print, exec, Python, another provider, or automatic
ConPTY backend substitution.

The exact public commit passed the repository's complete `make verify` on macOS,
including the 800-line source gate, copied frontend production build and all 332
frontend tests, 13 Tauri tests, all Rust tests, and warning-denied Clippy. It also
passed Windows GNU source and test cross-checks for the provider/server crates.
GitHub Actions run
[`32650454003`](https://github.com/kwd421/agentsassemble-rust/actions/runs/32650454003)
then executed on `windows-latest`: Windows workspace-hook registration/removal
passed, and one actual ConPTY test process accepted two sequential inputs through
the same bidirectional terminal, returned both responses, remained alive between
turns, and exited under managed custody.

Web review then found that the original bare `agentsassemble-room` hook and prompt
could let a workspace executable win Windows current-directory search before the
private helper on `PATH`. Public commits `7698afb` through `3d02575` bind the hook
document, provider prompt, terminal permission policy, and hook policy to the same
quoted absolute private-helper invocation and reject the bare basename. Windows
run [`32652867488`](https://github.com/kwd421/agentsassemble-rust/actions/runs/32652867488)
placed a decoy helper in the selected workspace, executed both the installed hook
command and an auto-approved room command under that cwd and environment, proved
that the decoy was never executed, and then reran the resident ConPTY test. Earlier
runs `32652173385`, `32652401445`, and `32652641770` failed while making the new
fixture represent the runner's DOS-short temp path and raw `cmd` quoting correctly;
none is counted as passing evidence and the security assertions were not removed.

A follow-up web review found that per-session absolute hook paths made the second
concurrent Antigravity session in one workspace conflict with the first workspace
registration. Commit `11e9b8547580c3da8b2f32ed40ff5034d7683ec2`
keeps the first verified absolute hook executable alive under the workspace
reference count, while each provider process receives its own canonical helper
prefix and retains its own portal authority. Windows run
[`32653761776`](https://github.com/kwd421/agentsassemble-rust/actions/runs/32653761776)
registered two different private helpers in the same workspace, retained the
shared hook after the first registration dropped, executed the second helper,
removed `hooks.json` only after the final drop, proved the workspace decoy was
never run, and then passed the resident ConPTY fixture again.

This is Windows OS transport evidence, not a claim that an authenticated
Antigravity Flash account or the copied desktop UI was exercised on Windows. The
published macOS real-client matrix above remains the provider/model evidence; a
future Windows real-provider run must be recorded separately rather than inferred
from the ConPTY fixture.

## Published OpenCode control-plane evidence: `5c31ccf`

The separate Daybreak manual security review found that the OpenCode loopback
control plane had no authentication and that dropping the port-reservation
listener before child bind could let another local process receive the first
health and RoomPortal-registration requests. Commit
`bcff0b55a8f082f77d69623528e4711882121b20` gives each OpenCode runtime a fresh
64-hex-character Basic-auth password, requires credentials in the shared strict
loopback client, and applies them to both bounded JSON requests and SSE streams.
The password enters only the exact provider environment; on Unix it crosses the
guardian boundary inside the anonymous inherited launch manifest and never enters
argv or the guardian environment.

Before any authenticated HTTP request, startup now requires the exact bounded
`opencode server listening on http://127.0.0.1:<selected-port>` line from the
byte-bound child's stdout. Only that proof permits the authenticated health check,
and only a successful health check permits RoomPortal registration. A process that
merely wins the released reservation cannot make the child emit that line before
initial health.

The first manual re-review correctly found that this still proved only historical
readiness: after the child died, a replacement listener could receive credentials
from a later transparent client reconnection. Commit
`3929e31f3407c60d829a030a527266daacaf9197` removes that client behavior. Every
JSON or SSE operation first creates a raw TCP connection without request bytes,
then revalidates the exact owned guardian/child, and only then constructs and sends
authenticated HTTP on that already connected socket using Hyper. A replacement
peer connected after child death receives EOF and no credentials; if the child
dies after verification, the established TCP socket cannot be redirected to a new
listener. Initial health, RoomPortal registration, session operations, turn POST,
SSE, abort, and disconnect all use this same custody-bound path.

The second manual re-review found a macOS-specific gap in that revalidation: an
exited provider can remain as a zombie under the guardian, retaining its PID and
process group until the guardian reaps it. Commit
`642f27250e966ffaf6070a8a3e7503dc961c0999` makes provider health an asynchronous
common-driver contract. On every Unix target the server asks the guardian over
its bounded private control protocol, and the guardian calls `try_wait` on the
exact `Child` handle before answering. A dead or zombie child therefore fails
before any HTTP byte is sent even when its PID and PGID still look live. The
existing group and Linux `/proc` checks remain additional custody evidence after
the exact-child proof. The guardian retains cleanup ownership after reporting an
exit, so existing stop and macOS fork-history failure semantics are unchanged.
The protocol was split into its own owner rather than weakening the 800-line gate.

The third manual re-review found that a timed-out or cancelled health probe could
leave its uncorrelated response buffered for the next post-connect check. Commit
`5c31ccf1cf33146a4e91431df7400b8508aca82d` adds a strictly increasing nonzero
request identity to every guardian health command and requires the guardian to
echo both that identity and the exact provider PID. A malformed, mismatched,
timed-out, or cancelled exchange permanently poisons that custody channel, so no
later request can consume a stale response. The poison guard is armed before the
first asynchronous write and disarmed only after one fully correlated response.
Normal stop no longer performs a preliminary health probe: it closes the guardian
input and executes the exact-owner cleanup directly, so a poisoned observation
channel cannot prevent guardian EOF cleanup or its existing receipt proof.

The exact code passed the complete `make verify` on macOS: architecture and
800-line source gates, generated protocol bindings, copied frontend build and CSS
verification, 65 frontend files with 332 tests, 13 Tauri tests, all Rust workspace
tests, warning-denied Clippy, and diff checks. The workspace also passed the
warning-denied `x86_64-pc-windows-gnu` all-target cross-check with the installed
rustup compiler. Focused tests prove credential validation, fresh password shape,
exact child-endpoint readiness, Basic authentication on both JSON and SSE, and
zero transmitted bytes when the connected peer fails the post-connect child-
custody check. The six real WebSocket Agent Session boundary tests passed three
consecutive runs after their existing receive deadline was increased from two to
five seconds; the frame-count and semantic assertions were unchanged.

Computer Use then launched the exact debug application bundle built from this
workspace, resumed the durable OpenCode `opencode/hy3-free` Agent Session, and
sent one room message through the copied composer. Direct unauthenticated requests
to that run's `/global/health` and `/event` returned HTTP 401. The real Hy3 agent
published `OPENCODE_PEER_CUSTODY_OK` through RoomPortal and returned to idle; the
copied stop control reached stopped state. The exact debug app, Rust sidecar,
guardian, anchor, and OpenCode process were then shut down, their absence was
confirmed, and the verification-only temporary directory was moved to Trash.
Pre-existing original-project and unrelated processes were not signalled.

The exact debug bundle was rebuilt from `642f272` source and the copied UI resumed
that durable Hy3 session again. The observed provider port returned HTTP 401 for
unauthenticated `/global/health` and `/event`, and the real agent published
`OPENCODE_MAC_CHILD_HANDLE_OK` through RoomPortal before returning to idle. The
copied stop control reached stopped; the app, sidecar, guardian, provider, and
observed listener were all confirmed absent afterward.

The exact debug bundle was rebuilt again from `5c31ccf` source. The copied UI
resumed the durable Hy3 session, the same observed provider port returned HTTP
401 for unauthenticated `/global/health` and `/event`, and the real agent
published `OPENCODE_HEALTH_NONCE_OK` through RoomPortal. The copied stop control
reached stopped; the debug app, Rust sidecar, guardian, provider, and observed
listener were all confirmed absent afterward.

The same Daybreak Blue high task then manually re-reviewed code commit `5c31ccf`
and evidence commit `7509c94` read-only and returned `PASS / APPROVE`. It confirmed
the correlated response contract, cancellation/timeout poison, overflow failure,
mutex serialization, poison-independent guardian cleanup, post-connect zero-byte
boundary, and JSON/SSE single-socket behavior, with no remaining credential
disclosure, deadlock, custody-loss, cleanup, or authority-bypass finding. No
second Deep Scan or automated scanner was used.

## Stage A settings/scheduling/tabletop candidate: 2026-08-24

The complete candidate passed `make verify` after the legacy/compatibility cleanup
and the current Stage A implementation. The run included the mandatory
architecture, source-growth, logical-line, and 800-line gates; generated protocol
bindings; the copied frontend production build and original-CSS provenance check;
66 frontend files with 338 tests; 14 Tauri tests; 15 domain, 77 persistence, two
protocol, 100 provider, 12 server, and 19 Rust integration tests; warning-denied
workspace and desktop Clippy; documentation tests; and the final diff check.

The transition contract exercises ordered to ambient to ordered with an existing
active turn. It proves stale settings revisions write nothing, ambient may create
a second active turn, and queued `OrderedObservation`/`AmbientObservation` values
retain their original delivery semantics across both transitions. Separate
contracts cover strict old-schema rejection without conversion, human replay and
tabletop gating, the durable provider 32-result budget, same-turn read receipt,
reservation-first and terminal-first ordering, and close tombstone resolution.

The first identical verification attempt exhausted local disk during the duplicate
desktop link step after the architecture gates, frontend build, and all 338
frontend tests had passed. Only Cargo-generated build artifacts were cleaned. The
successful rerun used one shared Cargo target directory with incremental output
and debug symbols disabled; no source, gate, warning level, assertion, product
data, or runtime behavior was changed.

This is deterministic candidate evidence, not packaged real-client or provider
parity evidence. No real provider or Computer Use session was started for this
run. The required copied-UI flow with persistent Codex Terra, Antigravity Flash,
and OpenCode Hy3-free sessions, critical web review, and Daybreak Blue manual
security review remain pending after the feature commit is pushed.

The first post-push Daybreak Blue high manual review found that the existing
WebSocket ingress limiter was connection-local and provider random results did
not share the original durable room write budget. The correction under
verification keeps the transport frame limiter as a secondary guard, adds one
room-task principal command/byte window shared across sockets and RoomPortal,
and reserves one SQLite room-wide command/byte window atomically with every
budgeted command result, lifecycle intent, or provider random event. Exact replay
and lifecycle resume are checked before admission, failed transactions roll the
durable reservation back, and `agent.stop` remains available at saturation. No
Deep Scan or automated security scanner was used for this review.

The in-progress Pro critical review confirmed three Stage A integration defects in the
public candidate: settings replay re-entered floor progression, the copied
preference controller swallowed the absent HTTP owner's failures and left local
optimism looking successful, and a structured handoff incorrectly outranked a
later direct body mention. The correction under verification skips progression
for deduplicated settings outcomes, gives preferences an explicit
loading/ready/saving/stale/error authority state with confirmed-value rollback
across the modal, channel menu, and header actions,
and restores the original final-direct-mention routing order. Focused frontend
contracts and the production build pass; the routing regression proves a later
`@Flash` mention overrides an earlier structured Terra handoff.

The corrected worktree then passed the complete `make verify` gate: architecture,
source-growth, logical-line, and 800-line checks; formatting and generated
bindings; the copied frontend production build and CSS provenance check; 67
frontend files with 341 tests; 14 Tauri tests; 16 domain, 79 persistence, two
protocol, 100 provider, and 13 server unit tests; 19 Rust integration tests; all
documentation tests; warning-denied workspace and desktop Clippy; and final diff
validation. No provider or packaged application was started by this deterministic
run.

### Command-admission and provider-presentation correction: `865ad02`

The next correction adds the actor-owned raw transport budget required by the
manual security finding and narrows saturated `agent.stop` admission to sessions
that actually own runtime or lifecycle-intent cleanup. The complete `make verify`
passed with every mandatory architecture and 800-line gate unchanged, 67 frontend
files with 342 tests, 14 Tauri tests, 16 domain, 80 persistence, two protocol, 100
provider, and 14 server unit tests, the Rust integration boundary suites,
warning-denied Clippy, generated bindings, original-CSS provenance, and the final
diff check.

Computer Use first exposed an existing user-data boundary rather than hiding it:
the installed Rust data authority was schema version 9 and the current runtime
requires version 11, so startup failed visibly. No migration, compatibility path,
fallback, or user-data edit was introduced. A release application was rebuilt
under a verification-only bundle identifier, creating a fresh schema-11 Rust
authority while leaving the user's existing data untouched.

In that isolated packaged application, the copied Agent Add modal showed the
`Harness`, `API`, and `Local` groups and Codex, Antigravity, and OpenCode from the
real Rust catalog. Before a provider was selected, neither `표시 이름` nor its
text field existed in the accessibility tree or rendered modal. Selecting each
provider made the field appear with its catalog-derived name: Codex and
Antigravity used the shared `provider · model` presentation, while the already
provider-qualified OpenCode model label remained unchanged. The original icon
card geometry and the existing per-mark scales were preserved; the three live
catalog marks were visually checked in the packaged modal. Freebuff and
TokenRouter are not yet exposed by the Rust live catalog, so their official assets,
recorded hashes, alpha bounds, and central renderer tests are evidence only and
were not misreported as a reachable packaged flow.

No Agent Session was created and no provider was started. The modal was closed,
the application and Rust sidecar were shut down, and both verification-only data
directories and application bundles were deleted after process absence was
confirmed. The installed Python application, the existing Rust user database,
and unrelated processes were not modified or signalled.

## Restartable local bootstrap and zero-room product candidate: 2026-08-25

A debug macOS application was built under the verification-only bundle identifier
`app.agentsassemble.rust.bootstrapverify`. Its Application Support, WebKit, cache,
and application-bundle paths were separate from the installed Rust and original
Python products. The copied production central-directory URL was deliberately
unset for this run because the central identity/native OAuth owner is still an
explicitly incomplete later slice; this run is evidence for the complete local
authority path only and is not central-account evidence.

Computer Use started from a fresh empty data root. The Rust runtime created schema
and immutable bootstrap lineage without creating a room, the copied startup gate
accepted `Local Operator Verify`, and the resulting complete authority exposed a
real zero-room directory. A second application build restarted against the same
authority without showing bootstrap again. The left-bottom profile loaded the
stored name through a fresh one-use server-operator profile ticket even though no
room existed.

The copied room-rail plus control then created the first real SQLite room
(`새 회의실`) and entered its authenticated WebSocket flow. The copied composer
published `bootstrap runtime verification message`; the timeline projected that
committed message. The user settings UI changed the server-wide human profile to
`Canonical Operator`. One profile revision atomically updated the left-bottom
card, right member panel, and historical message author projection without
changing room role, join state, or mute authority. Read-only SQLite inspection
confirmed bootstrap `complete`, profile revision 2, one durable room, and one each
of `room_created`, `message_final`, and `participant_updated`.

The Agent Add dialog was opened while the left-bottom profile card was expanded.
Opening the dialog closed the card and the modal backdrop remained above all
room/profile surfaces. Before provider selection there was no display-name input;
the live Harness catalog showed Codex, Antigravity, and OpenCode with their shared
icon presentation, and selecting Codex introduced the catalog-derived name and
settings fields. No workspace was selected, no Agent Session was created, and no
provider was started.

A final restart restored `Canonical Operator`, the real room, and the committed
message. The run also truthfully exposed separate incomplete surfaces: the copied
friends view failed because Rust has no `/api/room-friends`, and public-account
settings returned 404 because the account routes are not yet cut over. They were
not replaced with client data or hidden success.

The exact candidate then passed the complete `make verify`: mandatory
architecture, source-growth, logical-line, and 800-line gates; formatting and
generated bindings; the copied production frontend build and original CSS/cascade
check; 67 frontend files with 344 tests; 14 Tauri tests; 17 domain, 82 persistence,
two protocol, 100 provider, and 14 server unit tests; 20 Rust integration tests;
all documentation tests; warning-denied workspace and desktop Clippy; and final
diff validation.

After evidence collection the exact verification app, supervisor, and sidecar
were closed and their absence was confirmed. The verification-only Application
Support, WebKit, cache, and app-bundle paths were moved to Trash. Two earlier
verification-only diagnostic roots that exposed the still-incomplete central
identity boundary were also left recoverable in Trash. Existing user product data,
original-project processes, and unrelated processes were not modified or
signalled.

### Manual-review correction candidate: 2026-08-25

The first Daybreaker Blue High manual review of public range `92e6bb4..6de2671`
returned six medium blockers: the immutable bootstrap digest did not bind its
complete contract, room creation had no request owner, desktop URL modes could
bypass native bootstrap, zero-room directory decoding was loose and detached
from lineage, profile projection included ended memberships, and profile HTTP
could wait indefinitely for room publication. The correction binds every initial
profile and marker field with a versioned length-delimited digest; makes room
creation a transactionally reserved UUID request with exact replay only; requires
strict native grant/directory lineage before desktop entry; updates only Active,
Joined human memberships; and leaves profile publication to the durable room
cursor without waiting on a room actor.

The same critical web session independently reviewed `92e6bb4..6de2671` without
Deep Scan or a provider run and returned two additional medium blockers. The
production React composition still mounted an unavailable HTTP roster reader,
swallowed its failure, and merged its cache beneath canonical WebSocket
participants. The `Empty` bootstrap check inspected only rooms, participants, and
profiles, allowing another current product table to contain rows before bootstrap.
The correction removes the HTTP roster/role-refresh APIs and every production
refresh, merge, cache, departed-member, and ignored-failure path. The active room
and its invite modal now project only the authenticated WebSocket snapshot and
sequenced participant events. The schema owner now inventories every current
product table with a static `EXISTS` statement; a gate proves that inventory is
equal to every non-infrastructure table and that each query names its declared
table. Any product row while the marker is Empty produces `RepairRequired`.

The complete candidate passed `make verify`: every mandatory architecture,
source-growth, logical-line, and 800-line gate; generated bindings; the production
frontend build and original CSS/cascade check; 67 frontend files with 343 tests;
14 Tauri tests; 17 domain, 85 persistence, two protocol, 100 provider, and 14
server unit tests; 20 Rust integration tests; documentation tests;
warning-denied workspace and desktop Clippy; and final diff validation.

Computer Use drove a fresh debug app under verification-only identifier
`app.agentsassemble.rust.reviewfixverify`. It completed a zero-room bootstrap as
`Roster Canonical`, created the first real room, and showed exactly that one
canonical human in the right roster. Normal quit removed the exact app,
supervisor, and sidecar. Relaunch restored the same profile, room, and one-member
roster through a new native bootstrap/directory/room connection. The app and all
bundle-ID-specific Application Support, WebKit, cache, and app-bundle paths were
then shut down and moved to Trash; no verification-owned process remained. No
Agent Session or provider was started because this correction changes bootstrap,
room creation, profile publication, and roster projection only.

This entry records candidate evidence, not post-push reviewer approval. The exact
published correction range must still receive same-session critical web approval
and Daybreaker Blue High manual-security approval before the blockers are closed.

### Cross-review correction candidate: 2026-08-25

Daybreaker Blue High's next manual pass over `6de2671..4e4a44b` found four
remaining medium boundaries: Core-mount directory refresh used a loose response
and did not retain lineage, the React room-create caller discarded its operation
ID, a ticket issued before bootstrap corruption could reach the room mutation
before Complete was rechecked, and the table-inventory test could silently skip
an alternate valid DDL spelling. The same critical web session independently
confirmed the missing client room-create operation owner and ongoing directory
binding. It also found a separate roster regression: after the HTTP roster reader
was removed, `participant_joined` did not contain or insert a new room participant,
so a started Agent could remain absent until reconnect.

The correction makes every room list and create response a strict closed-schema
contract bound to the startup/native server and authority lineage. A dedicated
React controller retains one request and room intent, performs at most one
immediate exact replay for an ambiguous result, retains that same intent for a
later user retry, and accepts the created room only after a strict canonical
directory refresh. Room creation revalidates the full Complete bootstrap contract
inside the same `BEGIN IMMEDIATE` transaction before replay, reservation, or
mutation; its response uses the authority observed by that transaction rather
than a later read.

Schema installation and product classification now come from one table descriptor
set, and the gate executes the declarations and compares the real SQLite table set
instead of parsing a preferred SQL prefix. Agent launch now commits the canonical
room-owned participant in its sequenced `participant_joined` event, whose strict
client contract upserts the roster without deriving role, status, mute, or owner
from Agent Session state. This persisted event-contract change advances the schema
to 14; schema 13 is rejected without migration, conversion, or compatibility code.

Focused contracts cover exact client replay across one response loss and a later
user retry, rejection of loose directory/create payloads, declarative schema
installation, bootstrap corruption before room mutation, complete joined-participant
event publication, and immediate WS roster insertion. The exact worktree passed
the full mandatory `make verify`: architecture, source-growth, logical-line, and
800-line gates; generated bindings; production frontend build and original CSS;
70 frontend files with 349 tests; 14 Tauri tests; 17 domain, 85 persistence, two
protocol, 100 provider, and 14 server unit tests; 20 Rust integration tests; doc
tests; warning-denied workspace and desktop Clippy; and final diff validation.

Computer Use then drove a freshly packaged debug app under the isolated identifier
`app.agentsassemble.rust.crossreviewverify`. It completed native startup with the
stored `Cross Review Canonical` profile, reopened the real SQLite room, and showed
the one canonical human participant. The Agent Add flow selected the allowed free
OpenCode `opencode/hy3-free` model and the real project workspace, then started the
actual provider session through the production UI. Without reconnecting or using
an HTTP roster reader, the right panel immediately changed to `Agent — 1` and
showed `opencode/hy3-free` in Waiting state. The member card exposed the running
session controls, and its Stop action transitioned the same session to Stopped.
No prompt or provider-private data was sent or displayed.

Normal quit removed the exact app, supervisor, and sidecar processes. The isolated
Application Support, WebKit, cache, and both verification-built app bundles were
moved to `/Users/seinel/.Trash/AgentsAssemble-CrossReview-Verify-20260825-0312`;
the original product data and unrelated processes were untouched. This entry
records candidate evidence only. The exact published correction range must still
receive same-session critical web approval and Daybreaker Blue High manual-security
approval before these blockers are closed.

### Stopped-creation live-projection correction candidate: 2026-08-25

Daybreaker Blue High manually reviewed public range `4e4a44b..7fa1694`
without Deep Scan and returned `APPROVE`, with no Critical, High, or Medium
finding. The same critical web session independently rejected that range for one
reachable Medium parity defect: `agent.create(start=false)` durably created a
detached Participant and stopped Agent Session but published only their IDs, so
the copied React client could not show either authority until reconnect. The web
review also retained one Low lifetime-binding issue: a desktop directory response
could replace the existing webview authority pin when a fresh native bootstrap
grant matched the replacement.

The correction makes the creation transaction append its exact complete public
Participant and Agent Session projections in `agent_session_created`. Snapshot and
live socket admission closed-schema validate both nested records, their room,
session, participant, provider, name, status, ownership, and public-field
relations. React upserts both records from that canonical event for every viewer;
the issuer ACK remains irrelevant to UI authority. A later start success or
failure updates the same visible session, while history pagination cannot replay
old participant or session state over the current snapshot. The webview directory
pin is now compared before native bootstrap is consulted and can never be
overwritten by a replacement grant. Because already persisted schema-14 creation
events do not satisfy the new complete event contract, the clean schema advances
to 15 and schema 14 is rejected without migration or compatibility decoding.

The complete `make verify` gate passed unchanged: architecture, source-growth,
logical-line, and 800-line gates; generated bindings; copied production frontend
build and original CSS/cascade verification; 71 frontend files with 353 tests;
14 Tauri tests; 17 domain, 85 persistence, two protocol, 100 provider, and 14
server unit tests; 20 Rust integration tests; documentation tests; warning-denied
workspace and desktop Clippy; and final diff validation. The new hook test was
split at its Agent-creation responsibility boundary when the mandatory 800-line
gate rejected growth in the general hook test; no exception or gate change was
made. A subsequent loaded-host run also exposed that an existing integration
fixture allowed only two seconds to observe a verified provider child before
testing shutdown. The focused failure reproduced; its test-only observation bound
is now ten seconds, while product launch, protocol, cancellation, and shutdown
deadlines are unchanged. The focused contract and the final full gate both passed.

Computer Use then drove a fresh debug application under isolated identifier
`app.agentsassemble.rust.stoppedcreateverify`. It initialized the real schema-15
local authority as `Stopped Create Verify`, created the first real SQLite room,
selected OpenCode `opencode/hy3-free` and the actual Rust project directory in the
copied Agent Add flow, and explicitly turned `추가하자마자 실행` off. Without
reconnect, resync, or an HTTP roster request, completing `추가` immediately changed
the same right panel to `에이전트 — 1` and rendered `opencode/hy3-free 중지됨`.
Its member card showed stopped state with only Start enabled. Read-only SQLite
inspection confirmed schema 15 and one creation event containing
`participant.status=detached`, `agent_session.runtime_status=stopped`, and
`agent_session.enabled=false`. Process inspection confirmed that no OpenCode
provider was launched.

Normal quit removed the exact app, supervisor, and sidecar processes. The isolated
Application Support, cache, WebKit, and application-bundle paths were moved to the
recoverable Trash directory
`/Users/seinel/.Trash/AgentsAssemble-Stopped-Create-Verify-20260825-0352`.
Original-product data and processes and unrelated existing processes were not
modified or signalled. This is pre-push candidate evidence; the exact published
correction still requires same-session critical web and Daybreaker Blue High
manual-security re-review.

### Create/start snapshot-authority correction candidate: 2026-08-25

The same critical web session and Daybreaker Blue High independently rejected
public range `7fa1694..fbc44c5` for the same reachable Medium authority race.
For `agent.create(start=true)`, the first transaction stored the Agent Session as
`starting/enabled=true` but put its pre-intent `stopped/enabled=false` projection
in the creation event. A live viewer therefore disagreed with a concurrent
snapshot viewer while provider launch was pending. A resume snapshot could also
install its current Agent Session array and then replay the older creation event
over it, exposing stopped controls for a currently starting or later session.

The correction makes every creation event contain the exact public Agent Session
stored by that same transaction: `stopped/disabled` for `start=false`, or
`starting/enabled` for `start=true`. The strict browser contract admits only those
two coherent creation-state pairs. Initial, resume, and resync snapshots now use
their Participant and Agent Session arrays as the sole current-state authority;
snapshot events remain timeline/history and only separately delivered live events
can update current roster/session state. Focused persistence and server tests
prove creation-event equality with the durable concurrent snapshot, while the
React regression test proves an old stopped creation event in a resume snapshot
cannot rewind an idle current session. Because schema-15 data can contain the
inconsistent create/start event contract, the clean schema advances to 16 and
schema 15 is rejected without migration, compatibility decoding, or fallback.

The corrected worktree passed the complete unchanged `make verify`: architecture,
source-growth, logical-line, and 800-line gates; generated bindings; production
frontend build and original CSS/cascade verification; 71 frontend files with 355
tests; 14 Tauri tests; 17 domain, 85 persistence, two protocol, 100 provider, and
14 server unit tests; 20 Rust integration tests; documentation tests;
warning-denied workspace and desktop Clippy; and final diff validation. Clippy
initially rejected a four-line overrun in the focused persistence test, so its
creation-projection assertion was extracted by responsibility; no exception or
gate change was added.

Computer Use drove a fresh debug application under isolated identifier
`app.agentsassemble.rust.startauthorityverify`. A first production-central build
made the configured external guest request and failed visibly with `Load failed`;
read-only inspection showed a clean schema-16 `empty` bootstrap and no partial
profile or room. After normal shutdown, its exact data was moved to recoverable
Trash. The verification build was then rebuilt with only the central URL unset,
matching the documented local-authority scope rather than adding a product
fallback. It initialized `Start Authority Verify`, created the first real room,
selected OpenCode `opencode/hy3-free` and the actual Rust project workspace, and
left `추가하자마자 실행` on.

The real provider launched through the production UI. Without reconnect or HTTP
roster refresh, the right panel showed `에이전트 — 1`, `opencode/hy3-free 대기`,
and enabled running-session controls. Read-only SQLite inspection confirmed
schema 16, `agent_session_created.agent_session=starting/enabled=true`, and the
same durable session at `idle/enabled=true`; the exact provider guardian, anchor,
and `serve --pure` child were present under the verification server. The UI Stop
control transitioned the same card to `중지됨`, persisted `stopped/disabled`, and
removed those exact provider processes. Normal app quit then removed the exact
app, supervisor, and sidecar. Application Support, cache, WebKit, app bundle, and
DMG were moved to
`/Users/seinel/.Trash/AgentsAssemble-Start-Authority-Verify-20260825-0422`;
the failed-attempt data remains separately recoverable at
`/Users/seinel/.Trash/AgentsAssemble-Start-Authority-Verify-Failed-20260825-0415`.
Original-product data and unrelated processes were untouched. The complete
correction was committed and pushed as
`745e8832059a893f73e798a10369c2d52c5d0903`. Daybreaker Blue High then manually
reviewed the exact public `fbc44c5..745e883` range and reported no new
Critical, High, or Medium finding: `APPROVE`. The replacement critical web
session independently reviewed the same range in GPT-5.6 Sol Pro, including
ACK/event publication ordering, safe and uncertain launch failure, exact-request
retry/replay, snapshot authority, strict decoding, schema-16 fail-closed, and
private-authority exposure. It withdrew its final safe-failure retry concern
after tracing the same-session resume and single-creation-event contract, found
no reachable blocker, and returned explicit `APPROVE`. That approved session was
then changed and visibly verified as GPT-5.6 Sol `매우 높음` (fourth of five) for
subsequent critical reviews.

### Derived product-surface candidate: 2026-08-25

The server now derives its advertised HTTP routes, canonical WebSocket stream,
and currently implemented room actions from the same registries that build the
real Axum routers and strict protocol enums. The Tauri host likewise derives its
advertised command surface from the intersection of one shared command registry
and the checked-in desktop capability. The published candidate structurally
validates and pins both surfaces for its lifetime, rejects unadvertised native commands and room actions,
and no longer mounts copied plugin UI unless the server advertises its stream.
`message.send` also uses its actual content-only server contract; aliases and
extra client-owned routing fields fail closed.

The unchanged full `make verify` gate passed: architecture, source-growth,
logical-line, and 800-line gates; generated bindings; production frontend build
and original CSS/cascade verification; 71 frontend files with 355 tests; 15
Tauri tests; 18 domain, 85 persistence, four protocol, 100 provider, and 15
server unit tests; 20 Rust integration tests; documentation tests;
warning-denied workspace and desktop Clippy; and final diff validation.

Computer Use then drove a fresh debug application under isolated identifier
`app.agentsassemble.rust.surfaceverify`, with the central URL intentionally unset
to exercise the documented local-authority entry point. The real startup flow
initialized `Surface Verify`, created the first SQLite-backed room, joined it as
the human host, opened the canonical `room_events` WebSocket, and sent
`SURFACE_CONTRACT_OK`. The message returned through the room timeline and the
composer cleared. The reachable tree exposed the canonical room, channel,
profile, and roster surfaces but no unadvertised RimWorld/plugin UI. No provider
was started for this provider-independent contract slice.

Normal quit removed the exact app and sidecar processes. Its Application Support,
cache, WebKit data, and app bundle were moved to the recoverable Trash directory
`/Users/seinel/.Trash/AgentsAssemble-Surface-Verify-bZkrfr`; original-product
data and unrelated processes were untouched. The published candidate still
requires cross-review by the critical web session and Daybreaker Blue High.

Daybreaker Blue High rejected public range `be3e895..ce77535` with one reachable
Medium downgrade. The first directory response's surface digest was only checked
for 64-hex shape, not recomputed, and native bootstrap bound only server and
lineage IDs. A loopback response modifier could therefore keep the genuine IDs
and digest while replacing streams or actions with a valid strict subset. That
self-asserted subset became the immutable webview pin and could keep the room
socket or controls unmounted for the webview lifetime.

The correction makes the private control bootstrap grant carry the server
surface revision and digest. Before any room list is persisted or composed, the
webview reproduces Rust's versioned, 64-bit length-delimited canonical transcript
and recomputes SHA-256 with Web Crypto, then requires both the response digest
and the private-control digest to match. Recomputing an attacker's downgraded
digest therefore still fails against native authority, while retaining the
original digest fails against the received registry bytes. The lifetime pin is
installed only after both checks pass.

The correction passed the complete unchanged `make verify`: all mandatory
architecture/source-growth/logical-line/800-line gates, generated bindings,
production frontend and original CSS verification, 71 frontend files with 356
tests, 15 Tauri tests, 18 domain, 85 persistence, four protocol, 100 provider,
15 server, and 20 Rust integration tests, documentation, warning-denied Clippy,
and final diff validation.

Computer Use then drove a new packaged debug application under isolated
identifier `app.agentsassemble.rust.surfaceauthverify`. Its private-control
surface grant and HTTP directory surface passed the new independent digest
checks, the real local identity `Surface Auth Verify` completed, and the genuine
zero-room UI appeared. Creating the first real room then mounted the canonical
channel, host roster, and chat composer, demonstrating that the correction does
not reject or disable the legitimate server registry. Normal quit left no exact
app or sidecar process. Application Support, cache, WebKit data, and the app
bundle were moved to the recoverable Trash directory
`/Users/seinel/.Trash/AgentsAssemble-Surface-Auth-Verify-1FMoFt`; original data
and unrelated processes were untouched. No provider was required or started.

The correction was committed and pushed as
`1bcd09c877161570e1a1705312579893e60687db`. Daybreaker Blue High manually
re-reviewed exact public range `ce77535..1bcd09c`, independently reproduced the
canonical normal and downgraded digests, confirmed that both the retained-digest
and attacker-recomputed-digest variants fail before persistence/composition, and
returned `APPROVE` with no new Critical, High, or Medium finding. The same GPT-5.6
Sol `매우 높음` critical web session independently traced the native control
pipe, immutable shared `AppState` surface, Rust/TypeScript byte transcript,
crypto-failure behavior, startup side-effect order, lifetime drift, runtime
replacement, and non-desktop reachability. It likewise returned explicit
`APPROVE` with no Critical, High, or Medium blocker.

### Proof-bound finite subscription candidate: 2026-08-25

The room socket now registers its live receiver before constructing the durable
snapshot, serializes the exact final Snapshot frame at cursor `C`, and fixes one
transactional bounded catch-up high-water `H`. A new strict `Subscribed` receipt
binds the one-use ticket-derived connection nonce, client challenge, exact room,
principal and participant, protocol and stream set, pinned product surface,
canonical permissions digest, `C`, `H`, and SHA-256 of the exact Snapshot UTF-8
bytes with a versioned 64-bit length-delimited HMAC transcript. The client
serializes asynchronous verification, accepts no command before the receipt and
Snapshot verify and `C+1..H` arrives contiguously, and exposes readiness only at
`H`. String-ticket and non-desktop ticket fallbacks were removed; central and
guest room admission remain explicitly incomplete instead of borrowing desktop
authority.

The complete unchanged `make verify` passed in one continuous run: mandatory
architecture, source-growth, logical-line, and 800-line gates; generated Rust to
TypeScript bindings; production frontend build and original CSS/cascade check;
71 frontend files with 348 tests; 15 Tauri tests; 18 domain, 86 persistence, four
protocol, 100 provider, and 16 server unit tests; 20 Rust integration tests;
documentation tests; warning-denied workspace and desktop Clippy; and final diff
validation.

Computer Use then drove a fresh debug package named
`AgentsAssemble Subscription Verify` under isolated identifier
`app.agentsassemble.rust.subscriptionverify`. The central URL was intentionally
unset so the documented local-authority entry point, rather than the separately
incomplete central admission surface, owned the run. The real UI initialized
`Subscription Verify`, created its first SQLite-backed room, mounted the
proof-bound `room_events` subscription and sent `SUBSCRIPTION_PROOF_READY_OK`.
Normal quit removed the exact app, supervisor, and sidecar. A second Computer Use
launch recovered the same room and message through a new subscription, then sent
`SUBSCRIPTION_RECONNECT_OK` through the live timeline.

Final normal quit left no exact verification app, supervisor, or sidecar process.
Its Application Support data, cache, WebKit data, and app bundle were moved to
the recoverable Trash directory
`/Users/seinel/.Trash/AgentsAssemble-Subscription-Verify-Zx7qIO`. The two
preflight packages whose build identity or central configuration was rejected
before evidence collection remain separately recoverable at
`/Users/seinel/.Trash/AgentsAssemble-Subscription-Preflight-AWq58f` and
`/Users/seinel/.Trash/AgentsAssemble-Subscription-Misconfigured-EH5dwE`.
Original-product data and unrelated processes were untouched. This candidate
still requires exact public-diff cross-review by the critical web session and
Daybreaker Blue High.

Daybreaker Blue High rejected public range `27181cd..0e7d75e` with one High
channel-integrity finding. The signed receipt bound `C`, `H`, and the exact
Snapshot but did not authenticate catch-up/live events, ACK/NACK frames, or
client command bytes. An active loopback modifier could relay the genuine
receipt and Snapshot, change an event body without changing its sequence shape,
or retain a command action/request ID while replacing `message.send` content.
Those frames could pass the former structural checks and the command variant
could create attacker-selected durable state.

The correction derives a connection-specific HMAC key from the private ticket
proof key and receipt-bound nonce. After the plain receipt-bound Snapshot, every
frame in both directions uses one strict authenticated envelope. Its versioned,
64-bit length-delimited transcript binds nonce, `client` or `server` direction,
an independently contiguous counter beginning at one, and the exact decoded
inner JSON UTF-8 bytes. Canonical base64 carries the inner frame. Product bytes
remain limited to 256 KiB and the envelope to 384 KiB. Proof, counter,
direction, canonical-encoding, or inner-schema failure is rejected before event
projection or room action execution and closes the connection. The browser also
serializes outbound signing, sends each pending request at most once per
connection, and retains the same request ID for a fresh authenticated retry
after reconnect.

The unchanged complete `make verify` passed after the correction: all mandatory
architecture/source-growth/logical-line/800-line gates, generated bindings,
production frontend and original CSS verification, 72 frontend files with 351
tests, 15 Tauri tests, 18 domain, 86 persistence, four protocol, 100 provider,
17 server unit tests, and 21 Rust integration tests, documentation,
warning-denied Clippy, and final diff validation. New regressions reject exact
catch-up content mutation, command payload mutation, replay, counter gaps, and
direction reflection. A real server boundary test proves a command whose
authenticated payload is replaced cannot execute and leaves no attacker content
in SQLite.

Computer Use then drove a new packaged debug application under isolated
identifier `app.agentsassemble.rust.frameauthverify`, with the central URL
explicitly unset. Fresh local identity `Frame Auth Verify` created the first
SQLite room and published `FRAME_AUTH_CHANNEL_OK` through the authenticated
command/event path. Normal quit removed the exact application and runtime. A
second launch recovered the same room and message through a fresh connection and
published `FRAME_AUTH_RECONNECT_OK`. Final normal quit again left no exact app or
sidecar process. Its Application Support data, cache, WebKit data, preflight
output, and app bundle were moved recoverably to
`/Users/seinel/.Trash/AgentsAssemble-Frame-Auth-Verify-uwlYKa`; original data and
unrelated processes were untouched. The correction still requires exact
public-diff re-review by both critical reviewers.

Daybreaker Blue High re-reviewed public range `0e7d75e..8870778`, confirmed that
the original frame-tampering High was closed, and rejected one remaining Medium
retry ambiguity. An active loopback modifier could suppress an authenticated ACK
and every later counter frame without closing TCP. The former 20-second browser
timer then deleted the committed command's request ID and rejected it as an
ordinary timeout, allowing a user retry under a new ID to commit a duplicate.
The asynchronous signing queue also needed to recheck pending ownership after
the timer could delete a not-yet-sent command.

The correction now distinguishes commands that never crossed `WebSocket.send`
from commands with an unknown outcome. A pre-send command may expire normally.
Once sent, an ACK deadline closes the socket without deleting the pending
request; the next proof-bound connection replays the exact request ID, action,
and payload and resolves from the durable deduplication record. Explicit handle
shutdown reports `outcome_unknown` for any unresolved sent command. Pending
ownership is checked again after asynchronous signing and before send. A
connection generation is also checked after every receipt, key-derivation,
snapshot, and authenticated-frame crypto await and before readiness or
projection, preventing an old socket from reviving successor state.

Focused browser regressions prove that ACK silence closes the first connection,
keeps the promise unresolved, replays byte-equivalent command authority on a
fresh ticket, and resolves a deduplicated ACK; that a command whose pre-send
deadline expires while signing is never transmitted; and that a snapshot whose
verification completes after its socket closes cannot project or mark the
transport ready. The complete unchanged `make verify` then passed: all mandatory
architecture/source-growth/logical-line/800-line gates, generated bindings,
production frontend and original CSS verification, 72 frontend files with 354
tests, 15 Tauri tests, 18 domain, 86 persistence, four protocol, 100 provider,
17 server unit tests, 21 Rust integration tests, documentation, warning-denied
Clippy, and final diff validation.

Computer Use drove a newly packaged debug app under isolated identifier
`app.agentsassemble.rust.ackrecoveryverify`, with the central URL explicitly
empty. Fresh local identity `Ack Recovery Verify` created a real SQLite room and
published `ACK_RECOVERY_UI_OK`. Normal quit removed the exact app and runtime. A
second launch recovered the same room and message over a fresh authenticated
connection and published `ACK_RECOVERY_RECONNECT_OK`. Final normal quit again
left no exact app or sidecar process. Application Support, cache, WebKit data,
and the app bundle were moved recoverably to
`/Users/seinel/.Trash/AgentsAssemble-Ack-Recovery-Verify-vsI6O6`; original data
and unrelated processes were untouched. Both exact public-diff re-reviews remain
required after this second correction is committed and pushed.

Daybreaker Blue High then rejected the ACK-loss correction with one remaining
Medium outcome ambiguity. A committed command whose ACK was lost could be replayed
onto a fresh connection, but a queue, principal-resolution, or persistence failure
could emit an authenticated NACK before the durable replay result was recovered.
The browser treated every NACK as definitive, deleted the private request ID, and
allowed a new-ID retry that could duplicate the original mutation.

The current correction makes command outcome resolution a required server-owned
protocol field. Successful ACKs are `committed`; NACKs are `rejected` only for a
definitive command-owner rejection; queue, transport, lost-owner-reply, principal,
persistence, and public-projection ambiguity is `unresolved`. The browser never
infers certainty from an error code. It settles only committed/rejected responses;
an unresolved, missing, malformed, or action-mismatched response closes the socket
while retaining the exact private request ID and serialized command bytes for the
next proof-bound connection. The room runtime preserves this distinction across
queue admission, room-owner execution, committed provider failures, and post-commit
public projection.

Focused browser regressions now cover the full hostile path: ACK silence closes the
first socket; a fresh connection replays byte-identical authority; an authenticated
`unresolved` persistence NACK closes that socket without settling; a third
connection replays the same bytes again and resolves only on a deduplicated
`committed` ACK. A separate regression proves that an explicit `rejected` NACK is
the only NACK that rejects the user promise without reconnecting. Server boundary
tests require committed ACK resolution and unresolved transport/authentication
NACK resolution. The first complete verification reached every test successfully
but warning-denied Clippy rejected an eight-argument helper; no exception was
added. Protocol error code/message were grouped into the existing `ProtocolError`
value, simplifying the helper. The subsequent complete unchanged `make verify`
passed every mandatory architecture/source-growth/logical-line/800-line gate,
generated bindings, production frontend and original CSS verification, 72
frontend files with 355 tests, 15 Tauri tests, 18 domain, 86 persistence, four
protocol, 100 provider, 17 server unit tests, 21 Rust integration tests,
documentation, warning-denied Clippy, and final diff validation.

Computer Use drove a fresh debug package under isolated identifier
`app.agentsassemble.rust.outcomeresolutionverify`. One preflight package exposed
that merely unsetting the shell variable still allowed Vite's production `.env`
central URL; it was quit normally and its exact Application Support, cache, WebKit,
and app bundle were moved recoverably to
`/Users/seinel/.Trash/AgentsAssemble-Outcome-Resolution-Preflight-sMIRZ3`. The
package was rebuilt with the central URL explicitly empty. Fresh local identity
`Outcome Resolution Verify` created a real SQLite room and visibly published
`OUTCOME_RESOLUTION_UI_OK`. Normal quit left no exact app or runtime. Relaunch
restored the same room and message over a fresh authenticated channel and visibly
published `OUTCOME_RESOLUTION_RECONNECT_OK`; read-only SQLite inspection found
those exact strings as `message_final` sequences 2 and 3. Final normal quit again
left no exact app or sidecar. Application Support, cache, WebKit data, and the app
bundle were moved recoverably to
`/Users/seinel/.Trash/AgentsAssemble-Outcome-Resolution-Verify-Pmf8t2`; original
data and unrelated processes were untouched. Exact public-diff re-review by both
critical reviewers remains required after commit and push.

The critical web reviewer approved public range `9553f97..90448b3`, but the
independent Daybreaker Blue High review rejected it with one Medium phase error.
`room_command_result` globally treated every execution-time `CommandRejected` as
definitive even though `agent.create(start=true)` can commit creation and a start
intent before publication/provider completion returns that generic error shape.
Start, resume, and stop likewise cross durable-prepare and external-effect
boundaries before some completion failures. A rejected NACK at those points could
retire the private request identity even though the operation was nonterminal.

The correction moves certainty classification into each action owner. Atomic
no-effect failures use the transactional classifier. A safely failed provider
launch becomes rejected only after its terminal failure state is committed.
Publication failure after create/prepare, uncertain provider effects, confirmed
effects whose completion checkpoint fails, and stop finalization after an applied
effect are unresolved. Recovery events committed for an uncertain failure are
still published without converting that failure to a definitive rejection. The
general room actor no longer infers certainty from the persistence error variant.
Random-command transaction ownership was moved intact into its existing runtime
module to keep the room actor under the mandatory 800-line gate; no compatibility
path or second implementation was added.

The browser now also fails closed when an authenticated ACK/NACK names a request
ID it does not own, instead of silently ignoring the frame. Its public ACK type
derives the shared request/action/deduplication fields and resolution union from
the generated Rust protocol binding while retaining action-specific validated
result projection. The focused regression confirms that an unknown response
closes the channel, and the existing exact-byte replay tests continue to prove
that only committed/rejected server resolution settles a sent intent.

The final unchanged `make verify` passed mandatory architecture, source-growth,
logical-line, and 800-line gates (`room_runtime.rs` is 790 lines), generated
bindings, production frontend/CSS verification, 72 frontend files with 356 tests,
15 Tauri tests, 18 domain, 86 persistence, four protocol, 100 provider, 17 server
unit tests, 21 Rust integration tests, documentation checks, warning-denied
Clippy, and final diff validation. Two earlier full runs stopped as intended: the
first exposed the generated-JSON/application-type mismatch during the optional
binding cleanup; the second exposed 105/100- and then 114/100-line action
functions. Both were fixed at their owner boundaries without an allow or gate
change before the clean full run.

Computer Use drove a fresh package with identifier
`app.agentsassemble.rust.commandphaseverify` and an explicitly empty central URL.
Fresh local identity `Command Phase Verify` created a real SQLite room and visibly
published `COMMAND_PHASE_UI_OK`. Normal quit left no exact app or server process.
Relaunch restored the same room and message over a fresh authenticated channel
and visibly published `COMMAND_PHASE_RECONNECT_OK`; read-only SQLite inspection
found the two exact strings as `message_final` sequences 2 and 3. Final normal
quit again left no exact app or sidecar. The isolated application data, cache,
WebKit data, app bundle, frontend distribution, generated Tauri schemas, copied
sidecar, and repository Cargo targets were then removed; the two Cargo target
trees accounted for 23.1 GiB of regenerable output. Dependency installations,
source files, original application data, and unrelated processes were untouched.
The corrected public diff still requires both reviewers to re-review after commit
and push.

The same critical web session then reviewed public range `90448b3..b2de648` and
returned `APPROVE` with no Critical, High, or Medium blocker. It recorded durable
safe-failure replay as a Lower hardening observation. Daybreaker Blue High
independently returned `REVISE` with three Medium paths after tracing reconnect
through persistence and provider ownership: an `unconfirmed` start/stop replay
reset the intent to `prepared` and could call the provider again; a safe terminal
launch failure deleted its reservation, so suppression of the rejected NACK could
re-admit the same request; and an exact recovery-locked replay was classified as
rejected, causing the browser to discard the only owning request ID. The stronger
Daybreaker result is accepted because each path is reachable directly from the
current reconnect contract; the web approval and its narrower severity are kept
as independent evidence rather than silently replaced.

The correction makes safe launch failure one atomic durable terminal outcome.
The lifecycle reservation enters `rejected` with only the same bounded redacted
public code/message returned by the initial NACK. Exact replay returns that stored
rejection without another budget debit, event, provider selection, or provider
effect. Corrupt stored rejection data fails unresolved. `unconfirmed` start,
resume, create/start, and stop replay now returns a typed unresolved outcome
without changing the intent or calling a provider; only a later authoritative
reconciliation observation may change the state. The browser retains the exact
serialized command and adds a per-request exponential retry delay capped at 30
seconds, which a successful socket handshake cannot reset. The clean schema
advances to 17; schema 16 is rejected without migration or compatibility code.

Focused persistence regressions prove identical durable rejection replay with no
new events, retained request reservation, no repeated ambiguous provider plan,
and restart-uncertain create/start ownership. The browser regression proves
byte-identical replay across repeated unresolved NACKs and observes the growing
per-request delay on an already authenticated replacement connection. The final
unchanged `make verify` passed every mandatory architecture, source-growth,
logical-line, and 800-line gate, generated bindings, production frontend/CSS
verification, 72 frontend files with 356 tests, 15 Tauri tests, 18 domain, 86
persistence, four protocol, 100 provider, 17 server unit tests, 21 Rust
integration tests, documentation tests, warning-denied workspace/desktop Clippy,
and final diff validation. An earlier run stopped at the 800-line gate after the
new contract pushed a mixed lifecycle test file to 803 lines; the start-failure
tests were split at their owning responsibility without an exception. A later
frontend run exposed a real asynchronous test race between socket readiness and
authenticated command encoding; the regression now waits for the causal second
wire frame rather than assuming queue timing.

Computer Use drove a fresh debug package under isolated identifier
`app.agentsassemble.rust.lifecyclerecoveryverify`. Static inspection proved the
production central origin was absent from the built assets, and the startup UI
visibly selected local identity because no central directory was configured.
Fresh identity `Lifecycle Recovery Verify` created a real schema-17 room and
published `LIFECYCLE_RECOVERY_UI_OK`. Normal quit removed the exact app,
supervisor, and server processes. Relaunch restored the same room/message over a
new authenticated socket and published `LIFECYCLE_RECOVERY_RECONNECT_OK`.
Read-only SQLite inspection found the two strings as `message_final` sequences 2
and 3. Final normal quit again removed the exact process tree. The isolated
Application Support, cache, WebKit data, temporary server copies, copied sidecar,
generated Tauri schemas, frontend distribution, app bundle, and both Cargo target
trees were removed; the Cargo trees accounted for 12.0 GiB of regenerable output.
Dependency installations, original app data, and unrelated legacy/provider
processes were untouched. Commit/push and both post-push reviews remain required
before this correction is closed.

The subsequent same-session critical web review rejected the startup-only recovery
boundary. Daybreaker Blue High independently rejected the same-sidecar room-switch or
webview-reload path: a reservation created after admission was never enrolled after its
browser request identity disappeared. The web review additionally found that the key-only
watcher could reload a newer live-recovered candidate, that an old request lacked a
server-generation provenance check after restart, and that OpenCode could repeat
`POST /session` after its response was lost. Those findings are retained as the active
correction requirement rather than being treated as approval evidence.

Schema 18 stores the exact private principal and payload beside each incomplete lifecycle
reservation. Exact live replay now asks the common provider adapter for one bounded
observation and applies it under a complete candidate CAS. `Gone` reopens only the same
request, exact adoption continues through the owned runtime, and uncertain or ambiguous
results remain unresolved without state mutation or another provider effect. Startup
reconciliation terminally rejects a proven-gone start/create-start while retaining the
same Agent Session, and it commits a proven-gone or already-confirmed stop result. One
cancellation-owned watcher observes only the finite reservations present at startup and
can publish a later proven-gone terminal transition; it never reissues an effect. Stored
terminal rejection messages must also equal the canonical bounded redacted form.

Focused persistence tests prove exact-request-only recovery, exact adopted-runtime
continuation, restart-safe start and create/start terminalization, same-session reuse by
a new request, and stop result completion. The production server helper is exercised with
the real common provider adapter, and the server boundary test interrupts a real
create/start initialization, shuts down, restarts from the same SQLite authority, observes
the old durable rejection, and starts that same Agent Session with a new request. The
complete `make verify` passed every mandatory architecture, source-growth, logical-line,
and 800-line gate, generated bindings, production frontend and original-CSS verification,
72 frontend files with 356 tests, 15 Tauri tests, 18 domain, 90 persistence, four protocol,
100 provider, and 18 server unit tests, 21 Rust integration tests, documentation tests,
warning-denied workspace/desktop Clippy, and final diff validation. The first full run
stopped on a 106-line stop executor and two noncanonical timeout branches; the executor's
recovery preparation became one owned helper and the branches became `let-else`. The
targeted follow-up then found the expanded restart boundary test at 111 lines, so its
post-restart verification was split by responsibility. No allow, exception, or gate change
was introduced before the clean full run.

Computer Use drove a fresh debug package under isolated identifier
`app.agentsassemble.rust.liverecoveryverify`, with the central URL explicitly empty and
absent from production assets. Fresh local identity `Lifecycle Live Recovery Verify`
created a real schema-18 room and visibly published `LIFECYCLE_LIVE_RECOVERY_UI_OK`.
Normal quit left no exact app or server process. Relaunch restored the same identity, room,
and message over a fresh authenticated socket and visibly published
`LIFECYCLE_LIVE_RECOVERY_RECONNECT_OK`; read-only SQLite inspection found those exact
`message_final` values at sequences 2 and 3. The packaged Agent Add flow also closed an
open left-bottom profile card beneath its modal, exposed no display-name input before
provider selection, and introduced the catalog-derived field after selecting Codex. No
Agent Session was created and no provider was started.

Final normal quit again left no exact app or sidecar. The isolated Application Support,
cache, WebKit and temporary WebKit data, app bundle, frontend distribution, generated
Tauri schemas, copied sidecar, and both Cargo target trees were permanently removed; the
two target trees accounted for 12.6 GiB of regenerable output. Dependency installations,
source files, original application data, and unrelated processes were untouched.
Commit/push and both exact-diff reviews remain required.

The current correction replaces the startup-only key watcher with one server-lifetime
reconciler. It scans at most 64 durable Agent Session keys per cursor page, observes at
most eight captured `unconfirmed` candidates concurrently with the existing two-second
bound, and applies only each captured candidate/CAS. A live recovery that changes
`unconfirmed -> prepared` therefore makes an already captured watcher candidate stale;
the watcher drops it and never adopts the new phase. The task continues discovering
reservations created after network admission, so closing a room socket or reloading the
webview cannot strand recovery until sidecar restart.

Clean schema 19 adds an immutable random `SqliteStore` runtime generation to every private
lifecycle reservation; schema 18 is rejected rather than migrated. Exact live replay must
match that current generation both when loading and when applying its candidate. Reopening
the same SQLite authority creates a different generation, so an old browser request can
only remain unresolved while the server-owned reconciler terminalizes a later proven
`Gone`; it can never revive its provider effect. The private generation joins the complete
candidate CAS and does not enter snapshots, events, results, or diagnostics.

OpenCode now has an explicit provider-session creation authority. It becomes uncertain
immediately before the first custody-verified `POST /session` and is cleared only after a
successful response yields a valid session identity. While uncertain, runtime observation
reports `LeaseUncertain` and direct reuse rejects before polling a second provider request.
The stable OpenCode API currently does not expose caller-chosen idempotent session identity,
so this fail-closed state is retained until runtime absence is proven instead of guessing a
session or repeating the effect.

Focused tests pass for: post-admission dynamic discovery plus stale-candidate drop after
live reentry; previous-generation exact replay rejection followed by server-owned
terminalization; a same-sidecar browser identity loss followed by dynamic recovery,
reconnect, and a real new lifecycle start; and a deterministic OpenCode guarded request
proving the second session-creation future is never polled. The complete unchanged
`make verify` then passed every mandatory architecture, source-growth, logical-line, and
800-line gate, generated bindings, production frontend and original-CSS verification,
72 frontend files with 356 tests, 15 Tauri tests, 18 domain, 91 persistence, four protocol,
101 provider, and 19 server unit tests, 22 Rust integration tests, documentation tests,
warning-denied workspace/desktop Clippy, and final diff validation. No allow, exception,
or gate change was introduced.

Computer Use drove a fresh debug package under isolated identifier
`app.agentsassemble.rust.generationverify`, with the central URL explicitly empty. Fresh
local identity `Lifecycle Generation Verify` created a real schema-19 room and visibly
published `LIFECYCLE_GENERATION_SCHEMA19_UI_OK`. Normal quit left no exact app or server
process. Relaunch restored the same identity, room, and message over a new authenticated
socket and visibly published `LIFECYCLE_GENERATION_RECONNECT_OK`; read-only SQLite
inspection found the exact strings at event sequences 2 and 3 and zero Agent Sessions.
The packaged Agent Add flow also closed an open left-bottom profile card beneath its
modal, exposed no display-name input before provider selection, and introduced the
catalog-derived `Codex · GPT-5.6-Luna` field only after selecting Codex. No Agent Session
was created and no provider was started.

Final normal quit again left no exact app, supervisor, or sidecar. The Computer Use kernel
was reset immediately. The isolated Application Support, cache, WebKit and temporary
WebKit data, app bundle, frontend distribution, generated Tauri schemas, copied sidecar,
and both Cargo target trees were permanently removed. Repository Cargo output accounted
for 14.6 GiB; 894 terminated AgentsAssemble test/runtime temporary directories accounted
for another 46.3 GiB. macOS retained only 36 protected empty WebKit container directories
(0.0 MiB). Dependency installations, source files, original application data, unrelated
applications, and unrelated processes were untouched. Commit/push and both exact-diff
re-reviews remain required before this correction is complete.

### Durable effect-authorization and exact-cleanup correction candidate: 2026-08-25

Clean schema 21 makes the external-effect boundary explicit. A start is first `prepared`
without provider authority, then the common adapter reserves the exact runtime
handle/owner/custody identity and persistence atomically authorizes `effect_inflight`
before provider I/O. An uncertain return becomes `unconfirmed`; a confirmed stop awaiting
its result checkpoint becomes `effect_applied`. The production adapter has no start entry
point that bypasses that durable authorization, and persistence rejects empty or
substituted runtime identity. Schema 20 is rejected without migration, compatibility, or
fallback behavior.

The browser command owner and server-lifetime reconciler now acquire the same exact RAII
request claim across observation, candidate CAS, cleanup, and terminal commit. Abandoned
`prepared` work rejects without provider I/O, and `effect_applied` finalizes without a
second stop. Exact same-sidecar `Adopted` and `LeaseUncertain` results are persisted under
candidate CAS, reloaded only as that recovery operation's current authority, stopped by
the durable handle/owner, committed `Gone`, and released from the confirmed-stop
tombstone. `Ambiguous`, timeout, substituted identity, generation mismatch, or exact-stop
failure remains fail-closed. Cancellation can prevent observation or application but
cannot abandon a provider stop after that external effect starts.

Focused regressions prove exact duplicate-claim exclusion, lost command-owner recovery,
pre-effect terminalization, substituted unconfirmed-identity rejection, and both
same-sidecar browser-identity-loss paths: a pre-effect request becomes terminal and a
custodied running Codex runtime is adopted, exactly stopped, checkpointed, and followed
by a non-reused new runtime. Existing deterministic OpenCode coverage continues proving
that an uncertain session-creation response cannot poll a second `POST /session`.

The first manual Daybreaker review and the independent web review both found a Unix
pre-anchor crash gap: `launching:<generation>` could outlive the server before the
guardian acquired its own process-lifetime proof. The correction overlaps the server and
guardian shared locks on the exact token-bound lifetime inode. The guardian publishes a
bounded readiness record but cannot create the anchor until the server releases its lock
and sends the exact continue record. The guardian retains that lock while spawning an
exact generation-tagged anchor, and the existing guardian-to-stopped-launcher descriptor
handoff remains unchanged. `pending`/pre-anchor `launching` is therefore `Gone` only when
both the lifetime lock and exact runtime tag are absent; any observation failure remains
unknown, and an activated `unix` marker still requires the guardian's exact cleanup
receipt. Guardian normal cleanup, guardian death without a receipt, cancellation before
provider readiness, and cancellation after guardian spawn but before anchor creation are
covered by focused process tests.

The same review found that exact `effect_inflight` replay was admitted by persistence but
rejected by the live-recovery candidate boundary. That phase now retains the same exact
live recovery authority as `unconfirmed`; a focused persistence regression and the server
owner-loss recovery tests prove that it remains unresolved/recoverable rather than being
misclassified as a terminal command rejection.

The second independent web re-review closed both of those findings, then found the two
sides of one remaining boot-boundary defect. On the same boot, startup treated generic
`Ambiguous` stop authority as terminal `owner_lost`, cleared its exact handle/owner and
intent, and therefore admitted a replacement runtime even though an escaped provider
could still be alive. Across a real machine reboot, start did the safe opposite and kept
the authority, but could remain permanently unresolved because the dead guardian could
never write its cleanup receipt. The correction makes every generic `Ambiguous` candidate
retain its request, operation, handle, owner, and recovery-required state. Only a proven
`Gone` transition may release it.

New Unix runtime handles and activated markers bind a platform-domain-separated hash of
the exact OS boot identity. Linux/Android use the kernel boot UUID and macOS uses the
kernel boot epoch returned by the maintained `sysinfo` boundary. The value is read and
hashed once per server process; errors are cached fail-closed. A matching boot keeps all
existing lifetime/tag/receipt rules. A different exact boot proves the earlier OS process
cannot exist, including when the lease file was cleared, and enters the existing `Gone`
recovery UOW. Invalid or earlier handle formats are not treated as proof. Clean schema 21
removes the now-unreachable `owner_lost` reservation state, and schema 20 is rejected
without migration, compatibility, or fallback code.

After the final review corrections, `make verify` passed every mandatory
architecture, source-growth, logical-line, and 800-line gate; generated bindings;
production frontend and original-CSS verification; 72 frontend files with 356 tests; 15
Tauri tests; 18 domain, 93 persistence, four protocol, 104 provider, and 21 server unit
tests; 23 Rust integration tests; documentation tests; warning-denied workspace/desktop
Clippy; and final diff validation. The lifetime handshake was split at its owning boundary
into `guardian_lifetime.rs`; `guardian.rs` is 796 lines and `unix_process_tree.rs` remains
exactly 800. No allow, exception, threshold change, placeholder, fallback, compatibility
path, or hidden schema migration was introduced.

The performance inspection found two deterministic removable costs at the
server/persistence owner: Tokio interval's immediate first tick repeated the pre-admission
scan, and each candidate deserialized and allocated terminal reservation history that its
pending-only CAS never consumed. The watcher now delays its first tick by the configured
interval and loads only pending reservation rows. It retains ordered 64-Agent-Session
pages, eight-observation concurrency, and the two-second per-observation timeout. Recovery
still reuses the captured candidate and exact request claim rather than introducing a
cache, task per reservation, copied authority, or repeated provider effect.

The optimization intent is to remove redundant startup process/disk work and
terminal-history allocation at their existing owners. The preserved invariants are
reconciliation-before-admission, dynamic discovery and corruption detection across every
Agent Session, exact candidate CAS, bounded paging/concurrency/timeout, and fail-closed
uncertainty. A pending-reservation-first alternative was measured with SQLite `EXPLAIN
QUERY PLAN` and rejected: without a schema-21 status/session index it scans the complete
reservation history and builds a DISTINCT temporary B-tree each second. Adding that index
would be an explicit future schema, not a hidden migration here. The accepted trade-off is
the existing bounded per-session candidate read until that schema is designed and
benchmarked. Phase behavior is covered by persistence/server regressions and the
unchanged mandatory structure gates; no workload-calibrated latency claim is made.

The second review also identified an avoidable process-enumeration cost in the provider
observer. Previously, tuple construction evaluated `tagged_runtime_exists` even when the
exact lifetime flock had already proved `Active`, scanning the process table on every such
observation. Both pre-anchor and activated-group paths now short-circuit on that lock and
run the tag scan only when the lock is inactive or unreadable. The cached boot identity
likewise removes repeated kernel reads and hashing. The owning boundary is
`runtime_boot.rs` plus `runtime_lease.rs`; preserved invariants are identical `Active`,
`Gone`, and fail-closed `Unknown` classifications, exact token/generation checks, and no
public boot or runtime authority. The trade-off is one immutable 64-byte hash and one
`OnceLock` result per process. Focused lease tests cover same-boot uncertainty,
different-boot absence, malformed markers, and receipt-only cleanup; no
workload-calibrated latency claim is made.

The installed rustup `x86_64-pc-windows-gnu` target also completed a workspace
all-target/all-feature `cargo check` with the rustup Cargo, rustc, and target explicitly
bound. That cross-check caught and corrected one common marker-parser reference to the
Unix-only boot module; Windows now classifies that impossible Unix marker as unknown
without compiling or inventing Windows boot authority. A warning-denied full Windows
Clippy result is not claimed: the existing Windows-only provider tests and helpers still
contain unrelated warning-denied lint debt. The mandatory host workspace and desktop
Clippy gates above remain clean.

Packaged Computer Use and both pushed exact-diff re-reviews remain required before this
correction is closed.

## API verification scope

When a reachable flow specifically needs an API-backed provider, the allowed paid/provider-specific candidates are the official DeepSeek API and the designated Flash provider path. Every other API-backed verification uses only an explicitly free API or free model. Missing credentials, exhausted free quota, or unavailable models fail visibly; they do not trigger a paid substitution or a fallback provider.
