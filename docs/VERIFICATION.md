# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

The active comparison baseline is original
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust
`644b1d5`. Local uncommitted behavior
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
rejection without another durable room-budget reservation, event, provider
selection, or provider effect. Corrupt stored rejection data fails unresolved.
The later process-wide admission contract additionally requires a new principal
debit for each replay after that definitive rejection. `unconfirmed` start,
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

Clean schema 22 makes the external-effect boundary explicit. A start is first `prepared`
without provider authority, then the common adapter reserves the exact runtime
handle/owner/custody generation and persistence atomically authorizes `effect_inflight`
before provider I/O. An uncertain return becomes `unconfirmed`; a confirmed stop awaiting
its result checkpoint becomes `effect_applied`. The production adapter has no start entry
point that bypasses that durable authorization, and persistence rejects empty or
substituted runtime identity. Schema 21 is rejected without migration, compatibility, or
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
the exact OS boot identity. Linux/Android use the kernel boot UUID. The first pushed
correction used macOS `sysinfo::System::boot_time`; manual Daybreaker and independent web
review both rejected that source because XNU may adjust the wall-clock-derived boot epoch
during one OS boot. The replacement reads immutable `kern.bootsessionuuid` through the
maintained safe `sysctl` crate, parses and canonicalizes its UUID representation, and
never derives authority from wall-clock time. The value is read and hashed once per
server process; errors are cached fail-closed.

The same review found that an old-boot marker or a syntactically old handle could prove
absence without proving that it belonged to the same launch generation as the durable
Agent Session. Runtime-v5 now carries both the boot hash and exact launch token, while
schema 22 requires the same token beside the durable handle and owner. A different boot
enters the existing `Gone` recovery UOW only when marker, handle, and durable token agree;
when the lease file is missing, the strict old-boot handle and required durable token must
still agree. `Unknown` is never upgraded from a handle alone. A current handle with an old
marker, substituted token, missing token, malformed identity, or source/observation error
remains `Ambiguous`. Schema 21 and earlier handles are rejected without migration,
compatibility, or fallback code.

Focused regressions exercise the corrected proof lattice: a matching old marker/handle/token
is `Gone`; an old marker with a current durable handle, a substituted launch token, and an
old handle under `Unknown` are all `Ambiguous`; a missing lease is `Gone` only for a strict
old-boot handle whose token matches the required durable token. A persistence corruption
test removes `runtime_lease_token` from a schema-22 Agent Session and proves that candidate
loading rejects it rather than supplying a default. The macOS source-boundary test accepts
the uppercase UUID form returned by the kernel, canonicalizes it, and proves stable hashing
of the boot-session identity. Full gate results are recorded below after the correction is
verified.

The function-structure intent is reviewable separately from the security change. Boot
identity still performs one kernel read and one SHA-256 operation per process because the
existing `OnceLock` remains the owner; recovery does not add a kernel call or process scan
per candidate. Runtime-v5 parsing is a bounded fixed-offset pass over an opaque private
identifier, avoiding delimiter ambiguity without an allocation-heavy generic parser. The
preserved invariants are strict format rejection, no public boot/token disclosure, and
fail-closed cached source errors. The accepted costs are one required lease-token string
per durable live-runtime identity and carrying that same bounded string through start/stop
effects. This correction makes no workload-calibrated latency claim; focused tests,
warning-denied checks, and the mandatory suite are the verification evidence.

The schema-22 field propagation made the room event loop's provider-result future
16,624 bytes and crossed the warning-denied `large_futures` limit. The owning
`room_runtime` branch now boxes only that completed-provider-result future. The intent is
to keep the long-lived room-loop future bounded instead of embedding the complete result
handler state in every room task. Ordering, cancellation, join-set ownership, room-tool
ingress, and durable publication remain in the same branch and await point. The accepted
trade-off is one heap allocation per completed provider result; no allocation is added to
ordinary room commands, publications, or tool inputs. Warning-denied Clippy and the eight
real WebSocket Agent Session boundary tests verify the changed async boundary; no latency
or throughput improvement is claimed without workload measurement.

The gate-driven function splits also preserve their old owners instead of adding lint
exceptions. Durable Agent Session construction moved into one pure constructor while the
creation transaction still owns capacity, cursor, insert, event, and result ordering; the
pre-start result deliberately retains the stopped public projection. Provider turn health
checks moved into one helper without changing lock lifetime, cancellation priority, health
call count, or error classification. Create/start authorization groups the inseparable
handle/owner/lease-token triple at its call boundary and immediately destructures it at the
existing persistence owner. These are maintainability changes, not performance claims;
the complete lifecycle/provider regressions and unchanged structure gates are their
verification evidence.

After this schema-22 correction, `make verify` passed every mandatory
architecture, source-growth, logical-line, and 800-line gate; generated bindings;
production frontend and original-CSS verification; 72 frontend files with 356 tests; 15
Tauri tests; 18 domain, 94 persistence, four protocol, 106 provider, and 21 server unit
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
QUERY PLAN` and rejected: without a current-schema status/session index it scans the complete
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

The schema-22 correction reran that installed rustup toolchain/target pair explicitly and
passed the workspace all-target/all-feature Windows check, including the required lease
token through the common persistence/server path and the token-bound Windows handle.
An initial attempt with the Homebrew compiler correctly failed before checking project
code because it did not own the rustup target libraries; it is not counted as evidence.
Both attempts used isolated temporary Cargo target directories that were cleaned on exit.

The first pushed exact-diff reviews found two independent correctness gaps. Daybreaker
identified that cold and live absence decisions had diverged: a live slot accepted a
`PreviousBoot` marker by token alone, while Windows cold recovery accepted a
generation-gone marker without decoding and cross-binding the runtime handle. The web
review identified a crash window after a safe provider launch failure: the adapter removed
its lease evidence before persistence committed the terminal rejection, so a crash in that
interval could restart as same-boot `Missing` and remain permanently `Ambiguous`. The same
web pass also found a Linux-only test that still constructed the removed unbound `Gone`
variant; that was a test compilation defect rather than a production use of the old proof.

The correction gives strict runtime-v5 decoding to `runtime_handle.rs` and every `Gone`
decision to `runtime_absence.rs`. One fixed-offset decoder rejects malformed, trailing,
cross-version, and cross-platform handles. One proof function requires the same launch
token in the handle, durable session, and generation marker; Unix old-boot proofs also
require the same non-current boot identity. Cold and live scopes are explicit, and a live
slot always rejects `PreviousBoot`. The former platform-specific proof functions were
removed rather than retained as compatibility paths. Focused Unix tests cover strict
marker/handle/token agreement and the live-slot contradiction; the Windows-only test
compiles the exact handle/durable/marker triple and rejects substituted and malformed
handles. The stale Linux-only test now supplies its required generation token.

Safe provider launch failures now retain the exact pre-effect `Launching` authority or a
`StopConfirmed` tombstone through the persistence terminal checkpoint. Only after that
checkpoint succeeds does the server ask the adapter to release the exact
handle/owner/lease-token triple. A failed checkpoint deliberately retains the evidence for
restart reconciliation. Focused tests prove both sides of the boundary: dropping the
adapter before the terminal checkpoint leaves a disk-observable exact `GenerationGone`,
and the explicit post-checkpoint release permits one fresh generation.

The function/optimization intent is to eliminate divergent parsers and proof branches at
the authority boundary, not to claim synthetic throughput. Previously, Unix and Windows
cold recovery plus live observation and shutdown could evolve different token and handle
rules; each accepted branch had to be audited separately. The common decoder performs one
bounded fixed-offset parse only for a candidate that might prove `Gone`, adds no process
scan, disk I/O, cache, or public state, and removes the duplicate platform proof helpers.
Preserved invariants are fail-closed uncertainty, exact platform/boot/generation binding,
and no public runtime authority. The accepted safe-failure cost is one already-bounded
lease file and one existing runtime slot retained only until the exact terminal, live,
watcher, startup, or shutdown database transition commits; that short resource lifetime is
intentional crash-consistency evidence, followed by exact release. Focused
failure-injection tests and the mandatory host/Windows checks are the
evidence; no latency or allocation reduction is claimed without workload measurement.

After these review corrections, `make verify` again passed every architecture,
source-growth, logical-line, and 800-line gate; generated bindings; the production
frontend build and original-CSS verification; 72 frontend files with 356 tests; 15 Tauri
tests; 18 domain, 94 persistence, four protocol, 112 provider, and 21 server unit tests;
23 Rust integration tests; documentation tests; warning-denied workspace/desktop Clippy;
and final diff validation. The installed rustup `x86_64-pc-windows-gnu` target also passed
the workspace all-target/all-feature check, compiling the Windows-only exact triple test.
That non-warning-denied cross-check retained the already-recorded unrelated Windows-only
test/helper warnings and introduced no new project error. Its isolated temporary target
was removed with Cargo's own target cleanup immediately after the check.

The next Daybreaker manual re-review found one same-sidecar retry gap in that correction;
the web re-review independently found the same tombstone gap plus the corresponding
provider-observation and shutdown release-before-checkpoint path.
If the first terminal database write failed after a factory-safe launch failure, exact
request replay could commit live `Gone` and reset durable authority to `prepared`, but the
live recovery helper did not release the captured `StopConfirmed` tombstone. A subsequent
reservation therefore returned `operation_in_progress` and could make the session
unstartable until the sidecar exited. The common exact-command recovery owner now records
whether the captured start observation was `Gone`, commits its existing exact-CAS live
transition first, and only when that commit returns `RetryOriginalEffect` releases the
captured start-failure handle/owner/lease-token triple. Provider observation now reports
proven `Gone` without changing `Launching` to `Vacant` or deleting its lease. Dynamic and
startup recovery choose the start-absence or stop-tombstone release owner only after their
database commit. Shutdown likewise converts pre-effect `Launching` to `StopConfirmed`, so
its existing database-checkpoint-then-release sequence covers that generation. Failed or
stale commits release nothing, and stop recovery retains its separate checkpoint owner.

A deterministic server regression creates an unsupported transport tuple whose production
factory fails safely after durable `effect_inflight`, deliberately omits the first terminal
write to model its failure, performs exact same-request live recovery, and proves a fresh
lease generation can then be reserved. It therefore covers the prior DB-write-failure →
exact replay → new generation boundary without a provider process, fallback, or fake
authority. A second deterministic provider regression holds the exact Unix launch-lifetime
lock to force a real `begin_launch_effect` failure, observes `Gone`, drops the adapter
before any database checkpoint, and proves the same token remains disk-observable as
`GenerationGone`. The existing live pre-effect recovery regression now also proves its
post-commit reservation receives a different generation.

The implementation intent is crash consistency at the single database/OS-authority
boundary, not a throughput claim. Observation is now read-only with respect to the
in-memory slot and lease; the persistence owner performs one already-required exact-CAS
write and then one exact generation release. This removes the former release-before-write
window and avoids adding polling, a second process scan, copied authority, or a recovery
fallback. The preserved invariants are one lease generation per start attempt, no
replacement before durable proof, no release on failed or stale writes, and no repeated
provider effect. The accepted cost is retaining one bounded slot plus its existing lease
file until a terminal, live, watcher, startup, or shutdown database transition commits.
That resource retention is intentional evidence, not a leak; the successful commit owns
its exact release. No latency, allocation, or throughput improvement is claimed without a
workload measurement.

The final `make verify` passed every mandatory architecture, source-growth,
logical-line, and 800-line gate; generated bindings; the production frontend build and
original-CSS verification; 72 frontend files with 356 tests; 15 Tauri tests; 18 domain,
94 persistence, four protocol, 113 provider, and 22 server unit tests; 23 Rust integration
tests; documentation tests; warning-denied workspace/desktop Clippy; and final diff
validation. One earlier run exposed that the new server regression and an existing
owner-loss regression had accidentally reused the same fixed test Agent Session identity
and therefore contended for the real OS lease under parallel execution. The new fixture
now has its own identity; the complete 22-test server suite and the final mandatory run
then passed cleanly. The installed rustup `x86_64-pc-windows-gnu` target also passed the
workspace all-target/all-feature check. Its isolated target directory was removed on exit;
the previously recorded unrelated Windows-only dead-code warnings remain outside the
host's warning-denied gate.

The third web and Daybreaker exact-diff reviews independently found the symmetric stop
case missing from the live-request release. A successful provider stop followed by a
failed `record_agent_stop_effect` write correctly retained `StopConfirmed`; exact replay
then committed `Gone` as durable `effect_applied`, but the start-only post-commit branch
left that stop tombstone in memory. Provider-free finalization removed the pending
candidate, so every later start was rejected as `operation_in_progress` until sidecar
exit. Both reviewers classified that reachable authority stranding as Medium and found no
other Critical, High, or Medium regression in the correction.

The live-request owner now calls the same action-aware post-commit release used by startup
and dynamic recovery for every `Gone` that successfully returns
`RetryOriginalEffect`. The helper explicitly maps start to exact launch-absence release
and stop to exact confirmed-stop release. It uses the candidate captured before the
database cleared H/O/T; a database error, stale CAS, or unresolved observation releases
nothing. The implementation intent is to remove the action-specific
lifetime divergence at the existing owner. It adds no observation, process scan, retry,
cache, provider effect, or persistence operation; it replaces one start-only branch with
one bounded action dispatch after the already-required commit. Preserved invariants are
DB-before-release, exact generation matching, provider-free stop finalization, and no
replacement generation while evidence is uncheckpointed. No performance improvement is
claimed; the maintainability gain is one release contract shared by live, dynamic, and
startup recovery.

The new Unix integration regression uses the real local Codex protocol fixture and
production supervisor path to start and then stop an exact runtime. It deliberately omits
only `record_agent_stop_effect`, modeling that database write failing after the provider
is already held as `StopConfirmed`; the same authenticated WebSocket stop request then
performs exact live recovery, durable finalization, and a completely fresh non-reused
start. Its first focused run expected one extra WebSocket frame and timed out after the
correct three-frame stop result; the assertion was corrected to the actual protocol
sequence rather than changing product behavior. The focused rerun and complete suite
passed.

After this stop-symmetry correction, `make verify` passed all mandatory gates, generated
bindings, the production frontend build and original-CSS verification, 72 frontend files
with 356 tests, 15 Tauri tests, 18 domain, 94 persistence, four protocol, 113 provider, and
22 server unit tests, 24 Rust integration tests, documentation tests, warning-denied
workspace/desktop Clippy, and diff validation. The installed rustup
`x86_64-pc-windows-gnu` workspace all-target/all-feature check also passed with only the
already-recorded unrelated Windows-only dead-code warnings. Its isolated target directory
was removed on exit.

The fourth web pass approved that pushed stop-symmetry diff. Daybreaker then found a
different valid caller of the newly shared helper that the web pass had treated as
lifecycle-only: startup recovery accepts a normal active runtime whose lifecycle action,
ID, and status are all empty. Cold `Gone` commits that runtime's durable cleanup before
calling the helper. The dynamic page uses the same post-commit release owner, but admits
only reservation-bound lifecycle candidates and therefore does not create the empty-action
case. Treating every value other than start/stop as `unreachable!` could nevertheless
panic the first startup after an unclean shutdown, after the database transition had
already succeeded. Daybreaker classified the reachable pre-admission failure as Medium.
The empty action is now an explicit no-lifecycle runtime cleanup case and uses exact
confirmed-stop release; nonempty values other than start/stop remain stored-authority
corruption rejected before provider observation rather than a fallback.

Two deterministic server regressions now isolate the release owners from the one-second
watcher. One drives exact live stop reconciliation directly through the captured candidate
and proves commit, release, provider-free finalization, and a different fresh lease
generation. The other stages a durable active runtime with empty lifecycle authority and
an exact on-disk `GenerationGone`, drops the original adapter to model restart, runs the
pre-admission reconciler with a fresh adapter, and proves it completes without panic,
removes the candidate, and admits a fresh start. The real WebSocket/provider integration
test remains the end-to-end stop-checkpoint-loss proof; the direct unit regression removes
the reviewer's lower-severity concern that the watcher could otherwise win the test race.

The first complete parallel run of those regressions exposed test-only contention with the
existing owner-loss fixture: several reconciliation tests concurrently consumed the same
real process-wide executable/workspace authority-validation budget and two correctly
failed `runtime_authority_busy`. The production budget was not raised, bypassed, or retried.
The six reconciliation tests that acquire real OS authority now share one test-only mutex,
leaving product concurrency unchanged while making each failure boundary deterministic.
The complete 24-test server suite then passed together. This synchronization intentionally
trades parallel test speed for faithful use of the production admission limit; it is not a
runtime optimization or a product performance claim.

This correction does not add a branch guessed from malformed input. Persistence already
defines the empty triple as the canonical no-lifecycle state and rejects partial or
unknown nonempty lifecycle authority. The helper now mirrors that existing finite state
set. Its performance profile remains one bounded match after a successful database
commit, with no added I/O, scan, allocation, retry, or provider effect and no measured
performance claim.

After the no-lifecycle correction and deterministic test serialization, the final
`make verify` passed every mandatory architecture, source-growth, logical-line, and
800-line gate; generated bindings; the production frontend build and original-CSS
verification; 72 frontend files with 356 tests; 15 Tauri tests; 18 domain, 94 persistence,
four protocol, 113 provider, and 24 server unit tests; 24 Rust integration tests;
documentation tests; warning-denied workspace/desktop Clippy; and diff validation. The
installed rustup `x86_64-pc-windows-gnu` workspace all-target/all-feature check also
passed with the already-recorded unrelated Windows-only dead-code warnings, and its
isolated target directory was removed on exit.

The pushed `12b392946046f526bd80dc18da53a416bb9d7e54..dc4882b923137e3d842630f4bce1e8404528ea77`
correction then received independent manual line-by-line APPROVE results from both the
critical web reviewer and Daybreaker Blue High. Neither reviewer used Deep Scan, another
automated security scanner, or a real provider. The web reviewer independently corrected
the wording above: empty lifecycle authority is a cold-start candidate, while dynamic
reconciliation remains reservation-bound.

The isolated packaged app `AgentsAssemble Absence Verify` used bundle identifier
`app.agentsassemble.rust.absenceverify` with the central URL explicitly empty. Computer
Use completed the fresh local identity flow as `Lifecycle Absence Verify`, created a real
zero-state room, and published `LIFECYCLE_ABSENCE_DC4882B_UI_OK`. With the lower-left
profile card open, opening the Agent-add modal removed the card from the rendered and
accessible surface, so it did not rise above the modal. Before provider selection the
modal exposed no display-name field. Selecting Codex made the catalog-derived display
name, model, effort, permission, and workspace controls visible; the modal was cancelled,
so no Agent Session or provider process was created.

The app then exited through its normal application Quit menu and was relaunched from the
same package. The stored local identity, room, and first message returned without seed,
fallback, or re-entry. The reconnected room published
`LIFECYCLE_ABSENCE_DC4882B_RECONNECT_OK`. A read-only query of the exact isolated SQLite
store found one room, the two ordered `message_final` rows, and zero Agent Sessions. The
second run also exited through the normal Quit menu, and Computer Use reported the exact
bundle as not running before cleanup. The exact package, Application Support, Caches, and
WebKit paths for that isolated bundle identifier were then removed; a final process check
found no owned packaged-app process.

### Process-wide admission and bounded retry-ledger candidate: 2026-08-25

The former WebSocket semaphore, connection-local raw limiter, and room-actor human
write window could be reset or sharded across connections and rooms. The candidate
replaces them with three explicit process owners. A consumed room ticket acquires one
atomic `128 global / 8 principal / 64 room` active lease before HTTP 101. The first
subscription and every later data or control frame are charged before JSON or
authenticated-envelope parsing to fixed global, principal, and room windows. A fresh
typed human mutation is classified against durable replay/lifecycle authority, then
receives one non-refundable process-wide rolling principal debit before nonblocking room
queue admission. Its independent 128-permit in-flight lease moves into the room command,
so caller cancellation cannot free actor-owned work. Provider RoomPortal random results
retain a separate server-owned Agent Session budget and durable room reservation.

The implementation intent is bounded cross-room abuse resistance with short critical
sections, not an unmeasured throughput claim. Normal connection and raw-frame admission
perform fixed-count hash lookups and counter updates under one non-async mutex. Connection
maps contain only active scopes and therefore cannot exceed the 128 global leases. Raw
principal/room maps each retain at most 512 fixed-size windows and prune expired keys only
when capacity is reached. A new key that still cannot be retained is rejected after charging
the global window and any already-tracked principal or room window, without allocating an
overflow entry. The rolling human retry ledger replaces four retained request
strings with one domain-separated 32-byte digest, caps both 512 principal windows and
32,768 total live mutations, and uses a deque plus hash map for amortized O(1) expiry and
exact-retry lookup. RoomPortal byte admission is keyed by the server-owned Agent Session
ID—not by a provider-selected conversation ID—and now carries an O(1) running byte total
instead of folding as many as 3,600 retained results for every call. A room can own at most
64 Agent Sessions, so this actor-local map inherits an existing product cardinality bound.
The accepted fixed-window trade-off is a bounded edge burst at a ten-second raw-window
turnover; hard frame, global, principal, room, connection, and mutation ceilings remain
fail-closed.

The durable request classifier distinguishes a committed result, a pending
lifecycle owner, and a terminal rejected lifecycle owner. The first two avoid a
second process debit; the rejected owner preserves exact replay authority but
requires a fresh process debit because the earlier debit's retry exemption closed
at definitive resolution. A server-boundary regression creates a real terminal
start rejection, resolves the initial `MutationDebit`, and proves that a second
`admit_human_command` returns a new debit rather than entering the room queue for
free.

Focused deterministic checks prove atomic no-charge connection rejection, stale-lease
ABA resistance, one-principal connection enforcement through the actual HTTP upgrade,
cross-room raw-principal aggregation, independent control-frame ceilings, retained
over-limit raw debit, capacity-rejected frames charging both the global and an existing
principal scope, every fresh stop receiving a principal debit, exact unresolved mutation
retry without a second permanent debit, and closure of that exemption after a definitive
outcome without refunding its charge. The stop classifier now performs only the durable
request-identity lookup: it no longer reads Agent Session state merely to exempt the most
expensive fresh stop path. The connection lease implementation uses a checked process-local
`u64` sequence; an earlier review prompt's description of that sequence as random was a
wording error, not an implementation or contract change.
The complete `make verify` then passed
every mandatory architecture, source-growth, logical-line, and 800-line gate; generated
bindings; the production frontend build and original-CSS verification; 72 frontend files
with 356 tests; 15 Tauri tests; 18 domain, 95 persistence, four protocol, 113 provider,
and 31 server unit tests; 25 Rust integration tests; documentation tests;
warning-denied workspace/desktop Clippy; and final diff validation.

The installed `x86_64-pc-windows-gnu` target passed the workspace all-target/all-feature
check with rustup Cargo and rustc explicitly paired. A first Cargo-only invocation selected
the unrelated system rustc and failed before project compilation because that compiler
could not find its `core`; no source was changed, and the isolated temporary target was
cleaned. The correctly paired run compiled the new direct `parking_lot` use and every
server target, retaining only the already-recorded unrelated Windows-only dead-code
warnings. Its isolated target directory was also removed on exit.

Computer Use then drove a fresh debug package named `AgentsAssemble Admission Verify`
under isolated identifier `app.agentsassemble.rust.admissionverify`, with the central URL
explicitly empty. Fresh local identity `Admission Scope Verify` reached the real zero-room
directory, created one canonical room, and visibly published `ADMISSION_SCOPE_UI_OK`
through the authenticated room socket. Opening the lower-left profile card and then Agent
Add removed that card from the rendered and accessible modal surface. No display-name
field existed before provider selection. Selecting the retained Antigravity catalog entry
showed the catalog-derived `Antigravity · gemini-3.6-flash` name plus workspace, model,
effort, permission, and start controls; cancellation created no Agent Session and launched
no provider.

The app exited through its normal application quit shortcut and relaunched from the same
package. The stored identity, room, roster, and first message returned over a fresh socket,
which visibly published `ADMISSION_SCOPE_RECONNECT_OK`. A read-only query of the isolated
SQLite store found one room, those exact `message_final` values at sequences two and three,
and zero Agent Sessions. Final normal quit left no exact app, supervisor, or sidecar
process. The exact package, Application Support, Caches, and WebKit paths were then
permanently removed. Commit/push and both manual exact-diff reviews remain required before
this candidate becomes completion evidence.

## Canonical participant-role candidate: 2026-08-25

The current original commit and copied frontend expose exactly `human`, `director`,
`implementer`, `reviewer`, and `agent`; `director` also participates in reachable
ordered-room routing. Rust now owns those values as one `ParticipantRole` enum and
rejects old aliases instead of importing the Python compatibility normalizer.
`participant.role.update` requires room-management authority and commits the target
participant, `participant_updated` event, and idempotent command result in one SQLite
transaction. Clean schema 23 rejects schema 22 rather than reading its unconstrained
role strings.

The desktop and mobile roster no longer allocate and thread a second
`participant_id -> role` map derived from the same participant array. Both read the
room participant directly, including a human assigned `director` or an agent assigned
`reviewer`; human profile and Agent Session presentation cannot overwrite that field.
Name/provider inference remains only for a presentation row that has no canonical room
participant. This removes redundant per-render map construction and prop plumbing, but
the optimization is intentionally secondary to eliminating duplicate authority; no
cache or compatibility path replaces the canonical projection.

Focused persistence checks cover atomic projection/event/result, snapshot visibility,
exact replay, conflicting request reuse, unsupported alias rejection, missing targets,
and missing room-management authority. Frontend checks prove human and Agent Session
presentation preserve the room role. Signed snapshot, authenticated catch-up, and live
event checks reject a malformed `participant_updated.role` before advancing the durable
cursor, then reconnect from the last verified sequence. They also prove that participant
kind—not role—owns people/Agent
Session grouping, so a human `director` stays a person and an Agent Session assigned
`human` remains an Agent Session. Agent creation records the authenticated owning
participant ID rather than the separate user principal ID; this preserves the original
owner grouping without weakening authorization, which still resolves the current
principal and participant before the transaction. Generated TypeScript bindings carry
the same enum and product surface revision 2 advertises the newly reachable WebSocket
action.

The complete `make verify` passed every mandatory architecture, source-growth,
logical-line, and 800-line gate; generated bindings; the production frontend build and
original-CSS verification; 72 frontend files with 363 tests; 15 Tauri tests; 18 domain,
97 persistence, four protocol, 113 provider, and 31 server unit tests; 25 Rust
integration tests; documentation tests; warning-denied workspace/desktop Clippy; and
final diff validation.

Computer Use drove a fresh debug package named `AgentsAssemble Role Verify` under the
isolated identifier `app.agentsassemble.rust.roleverify`, with the central URL explicitly
empty. Fresh identity `Role SSoT Final` created a real schema-23 room. The copied room
member control changed the human from `human` to `director`; the row remained a person.
Agent Add exposed no display-name field before provider selection, then selected the
retained Antigravity `gemini-3.6-flash` catalog entry and the real project workspace.
`추가하자마자 실행` was disabled before creation, so the durable Agent Session remained
stopped and no provider process was launched. The room control changed that Agent
Session from `agent` to `reviewer`, and it remained nested under its owning person.

After normal application quit and relaunch, both roles, the human/Agent Session grouping,
and the stopped session returned over a fresh authenticated room socket. Read-only
SQLite inspection found schema owner `agentsassemble-rust-v1`, schema version 23,
`human/director` and `agent/reviewer` participant projections, the Agent Session owner
`operator-local`, two sequenced `participant_updated` events, and two matching durable
command results. The first real run exposed and caused correction of a stale frontend
`role != human` kind inference; a later run exposed and caused correction of the Agent
owner principal/participant mismatch. The final run verified both roots, not a visual
workaround. Normal quit left no exact app, supervisor, or sidecar process. The exact
package, Application Support, Caches, WebKit, and sidecar staging directories were then
permanently removed. The web critical reviewer approved the exact five-role design and
authority boundary (`B: APPROVE`). The role implementation was committed and pushed as
`4c7b2a0`. Daybreaker then found that malformed role-update events were validated only
inside projection, after socket cursor advancement. The correction moved that check to
the shared pre-cursor event-schema boundary and added all three delivery-mode regressions
above. Daybreaker approved the exact correction. The independent web Pro review then
traced snapshot, authenticated catch-up, live delivery, reconnect resume cursors, and
connection-generation fencing against `4c7b2a0..8554bb9` and returned `A: APPROVE`.
The role slice, including its cursor correction, is therefore completion evidence at
`8554bb9`.

## Participant-mute and exact-turn design gate: 2026-08-25

No mute implementation was started before the concurrency and crash contract was
reviewed. Daybreaker Blue High manually traced the proposed room, persistence,
provider-control, task-lifetime, and runtime-custody boundaries and returned
`APPROVE` with no remaining Critical, High, or Medium design blocker. Its final
implementation reminders are that a cancelled exact-turn begin handshake must be
joined or leave durable unresolved authority, and `schedule_requested` must be
consumed only in the assignment UOW.

The same design was independently reviewed in the carried web critical-review
session using GPT-5.6 Sol at Pro. The first pass found unsafe restart resend and
interrupt crash ambiguity; the second found ambiguity requeue, execution-owned
scheduling, roomless execution keys, and mutable-owner runtime uniqueness. The
design was corrected after each pass. The final pass approved blocking quarantine
with zero speculative requeue, room-scoped Agent Session scheduling authority,
`(room_id, session_id, turn_generation)` execution identity, immutable launch
uniqueness with full H/O/T CAS fencing, live task-death transitions, provider-specific
replay safety, and the two evidence-based terminal finalizers: `B: APPROVE`.

After approval, that web session was changed from Pro to `매우 높음`. The visible
control reported `매우 높음, 5개 중 4번째`, and the composer button also reported
`매우 높음`; normal implementation reviews continue at that verified level. This
entry freezes only the design. Implementation, deterministic tests, packaged UI and
allowed-provider real validation, commit/push, and post-implementation cross-review
remain required before mute becomes completion evidence.

## Participant-mute implementation candidate: `7a145d3`

Rust commit `7a145d3cb053ef5920e082a99961ecdf735dd14c` implements the
approved participant-mute and exact-provider-turn contract against original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`. Room management owns the mute bit.
Muting an Agent Session fences its exact `(room, session, generation)` execution,
dispatches at most one provider-specific interrupt through the runtime owner, and
does not infer success from process presence or UI state. A durable execution ledger
and durable effect ledger distinguish pre-dispatch, dispatched, ambiguous, finalized,
terminal, and quarantined states. Restart reconciliation never retransmits an
ambiguous provider start or interrupt and never requeues a turn until the exact
effect is resolved. Unmute changes only room authority and schedules eligible queued
work through the existing server-owned scheduler.

The shared runtime owns selection, lifecycle, task custody, and exact-turn admission;
provider drivers own only their native start, observation, and interrupt protocols.
Codex uses its official app-server turn identity, OpenCode uses its bound local server
message/session identity, and the retained Antigravity adapter uses its current
conversation/turn binding. Provider-specific details do not leak into room,
persistence, protocol, or frontend branching. The extracted files separate startup,
prompt construction, exact-turn control, effect persistence, participant moderation,
and reconciliation by change reason. No compatibility reader, Python path, fake
authority, provider fallback, or client-owned orchestration was added. Recovery uses
one explicitly bounded server-owned scan rather than browser retries or provider
effect replay.

The product-surface revision is now generated from the Rust protocol owner into
TypeScript. The first packaged run exposed the previous duplicated frontend literal:
Rust correctly advertised revision 3 while the desktop bridge still required revision
2. Replacing all production and test literals with the generated constant fixed the
actual host/server admission boundary. The same run exposed that the copied member
menu used an empty presentation-model `meeting_id` even though the canonical room
participant had the room scope. The single member-entry projection now prefers the
canonical participant scope, and a focused component regression proves that an Agent
Session with an empty presentation field still invokes `participant.mute` for the
canonical participant.

Computer Use drove the release package `AgentsAssemble Mute Verify` under isolated
identifier `app.agentsassemble.rust.muteverify0825` on macOS, with the central URL
explicitly empty. The normal central default was separately observed to fail visibly
with `Load failed`; the isolated local-only build is therefore local-runtime evidence,
not central-authentication or fallback evidence. Fresh identity `Mute Verify` created
one real room. Opening the lower-left profile card and then Agent Add removed the card
from the modal surface. No display-name field appeared before a provider was selected;
the copied Harness/API/Local groups and catalog-derived provider icons, labels, model,
effort, workspace, permission, and start controls were present after selection.

The retained Antigravity adapter ran the installed exact model identifier
`gemini-3.6-flash`. One turn returned the provider's short `Test message for room.`;
a second produced a real long actor-runtime response. A third repository-audit turn
was visibly `응답 중` when the room member menu sent `뮤트`. Read-only SQLite evidence
then recorded `turn_started` at sequence 53, `participant_muted(muted=1)` at sequence
56, and `turn_finished(status=interrupted)` at sequence 57. Its execution row was
`phase=interrupted` with finalized requeue custody, its one `interrupt` effect was
`phase=finalized`, and no later `message_final` existed for that turn. The UI returned
the Agent Session to `대기` and displayed `뮤트됨`; unmute previously cleared the same
canonical projection.

OpenCode ran the installed free identifier `opencode/hy3-free`. Both its long request
and a post-unmute reuse request ended with the provider result `declined`, with no
assistant response; this is recorded as a real provider failure rather than a pass,
mock, or substituted model. Codex Terra could not be selected reliably through the
macOS grouped native model selector during this run, so its real-provider row remains
`unknown`; no Luna, Sol, or Daybreak substitution was created. Deterministic provider
tests nevertheless cover exact Codex, OpenCode, and Antigravity interruption identities,
task cancellation, ambiguity, restart, and no-retransmission contracts. This distinction
prevents the successful Antigravity run from being generalized to an unobserved Codex
Terra flow.

The exact implementation commit passed `make verify`: mandatory architecture,
source-growth, logical-line, and 800-line gates; generated bindings; the production
frontend build and original-CSS verification; 72 frontend files with 365 tests;
15 Tauri tests; 18 domain, 104 persistence, four protocol, 114 provider, and 31 server
unit tests; 25 Rust integration tests; documentation tests; warning-denied workspace
and desktop Clippy; and final diff validation. Both real Agent Sessions were stopped
through their product controls before normal application quit. Computer Use confirmed
the exact bundle was no longer running, no verification-owned provider or server child
remained, and the exact package, Application Support, Caches, WebKit, and temporary
hook-lock paths were permanently removed. Post-push Daybreaker and web critical reviews
remain required before this candidate becomes completion evidence.

## Participant-mute post-review correction candidate: 2026-08-26

The independent web implementation review of public range `6681012..e4f5949`
returned `REVISE` with six reachable blockers. The correction retains an exact
`NotStartedRetained` or typed result tombstone until a durable terminal ACK, adds a
server-lifetime provider-turn reconciliation cursor beside the lifecycle cursor,
revisits transient lease observations, invokes the shared scheduler in the exact
runtime-gone UOW, validates `participant_muted.participant_id` before any browser
cursor advance, and terminalizes a busy `agent.stop` execution/effect before its
lifecycle checkpoint. The lifecycle checkpoint and subsequent command-result UOW
remain two explicit durable phases: `effect_applied` blocks other command admission
and startup/live lifecycle recovery finishes it without repeating provider stop.

The review also proved that the previous Antigravity implementation treated 300 ms
of PTY silence after Ctrl-C as retained-runtime quiescence. The real
`gemini-3.6-flash` run above remains valid evidence of what that build visibly did,
but it is not evidence that late native output or workspace side effects were
impossible. The correction deletes that silence heuristic. Antigravity now writes
Ctrl-C only for the exact turn, poisons the driver, and requires the common H/O/T
supervisor to stop and reap the exact runtime. Mute can finalize only from the
resulting provider-neutral `RuntimeGone` proof and leaves the Agent Session stopped;
an uncertain stop remains quarantined and reusable by nobody.

Deterministic corrections cover retained completed results, retained pre-dispatch
zero-call proof, exact runtime-stop notification, transient-lease revisit,
runtime-gone ordered-floor scheduling, busy confirmed/ambiguous stop custody, and
malformed mute-event cursor rejection. The correction implementation is split from
`e5172e0` through `fc2b538` into independently reviewable provider, persistence,
server connection, frontend, test-structure, and async-structure commits. Their
changed-line sizes are respectively 924, 142, 458, 215, 660, 251, 97, 515, 107,
484, 94, and 95; no correction commit reaches 1,000 changed lines.

`make verify` passes after those corrections: all repository architecture/source
growth and eight policy-gate tests, Rust format/check, 72 frontend files with 367
tests plus the production build and original-CSS cascade check, 15 desktop tests,
301 workspace Rust tests, workspace/desktop clippy with warnings denied, and the
final diff check. A new packaged Antigravity mute flow, push, and both post-push
reviews are still required before this correction is completion evidence.

The retained Antigravity prompt/interrupt custody correction was subsequently
reviewed as the exact public range `0589ce5..a9cfed4`. Daybreaker Blue High and the
independent `매우 높음` web reviewer both returned `APPROVE` without a Critical,
High, or Medium blocker. The web reviewer confirmed that active-turn custody is
installed before the first PTY write await, exact interrupt poisons the driver before
Ctrl-C, same-request resume cannot duplicate the prompt, and the common owner returns
the driver before exact H/O/T stop. It also confirmed that production commit
`97c3eef` and focused-test commit `a9cfed4` are independently reviewable. Neither
review used Deep Scan, another automated scanner, or a real provider.

The RuntimeGone-versus-`agent.stop` correction was independently re-reviewed as the
exact public range `a9cfed4..da6d47b`; both reviewers returned `APPROVE` without a
remaining Critical, High, or Medium blocker. They traced both SQLite writer orders,
exact pending-reservation binding, stop-owned execution/effect terminalization,
owner-loss completion, mute-effect interaction, duplicate-event prevention, live
floor assignment, and shutdown deferral. The range is five purpose-separated commits:
production fence, race regression, async stack-only boxing, reservation-binding
correction, and forged/missing-reservation regression. These approvals close the
manual-review condition for those two correction ranges, but do not replace the still
required fresh packaged product flow and allowed-provider verification.

## Central guest registration and host-custody verification: 2026-08-26

Public Rust commit `429127e` passed the complete `make verify` boundary: all
architecture/source-growth/policy gates, Rust format/check and generated types, the
production frontend build with original-CSS verification, 74 frontend files with 371
tests, 15 desktop tests, all-feature workspace tests, warning-denied workspace and
desktop Clippy, documentation tests, and the final diff check.

Computer Use drove the release package `AgentsAssemble Central Verify` under the
isolated identifier `app.agentsassemble.rust.centralregverify0826` with the normal
production central-directory URL. A fresh guest display name created a real central
guest, committed the native local profile, obtained the exact purpose-bound local
registration proof, registered the local server, and reached the recovery-code screen.
The recovery code was redacted from tool output and was neither copied nor recorded.
After acknowledging that screen, the copied application reached the real zero-room
directory. The room rail contained no fabricated room and retained its server-owned
create-room entry.

Normal quit stopped the package and its owned sidecar. Relaunch skipped the login gate
and returned to the same zero-room application. Read-only SQLite inspection before and
after restart found the same digest over the public `(server_id, host_public_key)`
identity, bootstrap state `complete`, zero rooms, and exactly one durable host
initialization marker. This verifies restart-stable registration custody without
reading or logging the private key. The Friends view still reported `Load failed`;
that is the separately inventoried, unimplemented `/api/room-friends` surface and was
not counted as central or zero-room parity.

After the run, Computer Use closed the application; no exact package, sidecar, or server
process remained. The isolated Application Support, WebKit, cache, `.app`, and `.dmg`
paths were permanently removed. Daybreaker Blue High and the independent web reviewer
at verified `매우 높음` both manually reviewed the final nonce-bound correction and
returned `APPROVE` with no remaining Critical, High, or Medium blocker. Neither review
used Deep Scan, another automated scanner, or a real provider.

## Participant-mute provider-matrix completion: 2026-08-26

The packaged provider matrix used the copied production frontend and the installed
provider runtimes without mocks, provider substitution, print mode, internal state
injection, or client-owned orchestration. The first isolated local-only package was
`AgentsAssemble Provider Verify`, identifier
`app.agentsassemble.rust.providermatrixverify0826`. The grouped provider menu initially
made the catalog's Terra option unreachable through macOS accessibility because opening
the menu did not transfer focus to its search owner. Production commit `09b79b8` adds
only that focus transfer; test commit `0d6d3a5` fixes the search and grouped-option focus
contracts. The catalog, selection value, provider authority, and option click path did
not change.

Codex used the exact installed `gpt-5.6-terra` model. A real room turn returned
`TERRA_OK`, and a later repository-review turn was visibly `응답 중` when the room
member menu muted the Agent Session. The UI projected `뮤트됨` and returned to `대기`.
Read-only persistence inspection recorded the exact execution as `interrupted`, its
interrupt effect as `finalized`, and no late final after the mute observation window.

Antigravity used the retained native PTY adapter with exact model
`gemini-3.6-flash` and medium reasoning. A real turn returned `AGY_OK`. Muting a later
busy repository-review turn projected `뮤트됨` and `중지됨`; persistence recorded the
execution as `interrupted` and its interrupt effect as `finalized`. No late final was
published. This run used neither print mode nor OAuth substitution.

OpenCode used the exact installed free model `opencode/hy3-free`. A real turn returned
`HY3_OK`, closing the earlier provider-declined observation without substituting another
model. The first busy-mute attempt then found a real defect: UI mute authority committed,
but the execution and effect became `recovery_required` and the session remained
`응답 중`. OpenCode abort side events were being judged as ordinary turn success, while
the quiescence reader depended on deprecated `session.idle` instead of the current
same-session `session.status { type: "idle" }` terminal signal.

Production commit `1fcca2a` separates ordinary turn completion from interrupt
quiescence. Ordinary turns still fail closed on provider errors and interactive requests;
quiescence ignores those nonterminal abort side events and accepts only the current
session's bounded `session.status` idle signal. It keeps the existing session filter,
10-second deadline, 8 MiB stream, 512 KiB line, and 8,192-event bounds. Test commit
`035416c` proves that permission, error, and busy events cannot terminalize the wait.
The correction deliberately removes dependence on the deprecated idle event rather than
adding a compatibility fallback.

Computer Use repeated the OpenCode flow from a fresh release package,
`AgentsAssemble Provider Fix Verify`, identifier
`app.agentsassemble.rust.providerfixverify0826`. The same free model returned
`HY3_FIX_OK`; a later long turn was visibly busy before mute, then projected
`뮤트됨` and `대기`. Ten seconds later there was no late final. Persistence recorded
generation one as `completed`, generation two as `interrupted`, and its single interrupt
effect as `finalized`, with no recovery-required row.

Current commit `035416c` passed complete `make verify`: architecture, source-growth,
and policy gates; Rust format/check; generated bindings; production frontend build and
original-CSS verification; 74 frontend files with 371 tests; 15 desktop tests; all 330
workspace Rust unit/integration tests; documentation tests; warning-denied workspace and
desktop Clippy; and final diff validation. Daybreaker Blue High manually approved exact
public range `0d6d3a5..035416c`. The independent web reviewer at verified `매우 높음`
approved the focus range and then the complete OpenCode correction, with no remaining
Critical, High, or Medium blocker. Neither review used Deep Scan or another automated
scanner.

Both isolated packages were quit normally. No exact application, server, sidecar, or
verification-owned provider child remained. Their Application Support, WebKit, cache,
`.app`, and `.dmg` artifacts, including the unused default-name bundle produced during
packaging, were permanently removed. This completes the fresh packaged provider-matrix,
exact-mute, review, and cleanup evidence for the Phase 4 moderation boundary.

## Local room preference desktop cutover verification: 2026-08-26

The copied room-settings UI now uses a fresh native purpose ticket for each desktop
preference read and write. The browser never receives a generic operator credential for
this flow. POST sends only preference fields, and the response parser requires the exact
requested room and complete canonical wire shape. Production commits `1cb3892`,
`5f44948`, and `490f527` separate the connection, cache isolation, and strict grant validation; commits `fcf49b8`,
`2e155e1`, and `99c159d` separately own their regression evidence.

The concrete cache threat was reuse of the same room-settings URL across a consumed
ticket, changed membership, or changed user. A browser cache hit could otherwise avoid
both one-use consumption and current server authorization. The server therefore applies
`Cache-Control: private, no-store` through the maintained Tower HTTP response-header
layer to every GET/POST success and error, while the desktop fetch also requests
`cache: no-store`. This adds no application cache, custom middleware, or duplicate state.
The concrete typed-boundary threats were JavaScript string coercion accepting a one-item
array as a ticket and URL parsing repairing a noncanonical loopback representation. The
validator now requires an actual string and exact `http://127.0.0.1:<port>` source text.
It does not normalize a malformed host response into a usable capability.

No CPU, memory, disk, or latency hot spot was measured in this slice, so no speculative
performance layer was added. A fresh native grant on every operation is an intentional
security cost that preserves one-use authority. The existing 16 KiB request limit,
54-channel preference bound, short identity transaction, and release-before-body ordering
remain the resource boundaries; the implementation reuses the existing ticket store,
SQLite owner, fetch path, and maintained response-header layer.

Computer Use first drove `AgentsAssemble Preference Verify` under isolated identifier
`app.agentsassemble.rust.preferenceverify0826`, then repeated the final strict contract
from a newly built `AgentsAssemble Preference Strict Verify` package under identifier
`app.agentsassemble.rust.preferencestrictverify0826`. Both used an explicitly empty
central URL and fresh local identities. A real room was created through the copied room
rail. Its settings modal loaded the default room and channel preferences through the Rust
runtime; room notifications and `#general` notifications were changed to `mute`. The
modal and connection panel immediately showed `알림 끔` and `1 muted`. Read-only SQLite
inspection found the same `mute` values in the room's canonical preference JSON.

Normal quit stopped each exact app, supervisor, and sidecar. Relaunching the same package
restored the user, room, `1 muted`, room-level `알림 끔`, and channel-level `알림 끔`
through fresh tickets. No provider was started. After each run, exact process absence was
checked and the isolated Application Support, cache, WebKit, app-bundle, copied sidecar,
frontend distribution, generated schemas, and first run's separate 2.0 GiB temporary
target were moved to the recoverable Trash. The default application identity and data
were never used.

The first complete `make verify` attempt exposed only three rustfmt differences in the
desktop ticket bridge and stopped at that mandatory gate. Mechanical commit `11362af`
fixed only those differences. The clean rerun passed architecture, source-growth, policy,
formatting, generated bindings, original-CSS verification, all 76 frontend files with
375 tests, all 16 desktop tests, every workspace Rust unit/integration and documentation
test, warning-denied workspace and desktop Clippy, and final diff validation.

The later cross-layer review identified a parity overclaim rather than a privilege
bypass: a remote room session was sent directly to the purpose-ticket-only Rust route,
so the real server returned 401 while a fetch mock described success. Rust has no durable
invite/admission session owner yet, so accepting that opaque bearer or issuing a local
operator ticket would have fabricated authority. Production commit `3fb8350` instead
fails remote-session preference reads and writes before native invocation or network I/O,
and the controller exposes that incomplete authority as an error so the copied controls
remain disabled. Test commit `3dbeceb` proves both GET and POST make no fetch or Tauri
call, background refresh remains off, and notification controls stay disabled; the three
focused files passed 25 tests. The previously packaged local-operator read, write, and
restart flow remains unchanged.

This correction adds no cache, session store, protocol fallback, or future-only
abstraction. No CPU, memory, disk, or latency cost justified an optimization. The change
removes one dead request path and records the actual dependency: durable fingerprinted
human admission must exist before a live-session-bound one-use preference exchange can
restore remote parity. Security review additionally requires a derived ticket to retain
session provenance and revalidate current expiry/revocation at consumption, including
session revocation after ticket issuance.

## Asset custody storage correction: 2026-08-26

Public commits `334c918`, `d337003`, `23571fe`, and `ac542de` replace the combined
profile/pre-join avatar table with clean schema 40. Profiles own at most one pending and
one current avatar; exact invite/browser custody owns one expiring pre-join row; bound
room appearance belongs to the room rather than its uploader. The user 64-item/128-MiB
policy and pre-join invite/room operating quotas are removed. The common module owns
only raster safety, the 4,096-item/8-GiB absolute bound, and checked exact-replacement
arithmetic. No migration, compatibility path, fallback, generic asset framework,
configuration layer, message-attachment state, or background cleanup task was added.

The shared usage query streams the three owner tables once with `UNION ALL`. SQLite
query-plan inspection justified replacing the existing profile pending-expiry index—not
adding another index—with `(state, expires_at)`, which serves both live OR branches and
expiry deletion. Admission now pays additional transactional insert/delete statements
to remove impossible cross-owner state; no latency improvement is claimed. Detailed
threat, cost, deletion, and test evidence is recorded in
`verification/2026-08-26-human-invite-schema.md` and
`specs/asset-custody-lifecycle-slice.md`.

All 164 persistence tests, all 58 server unit tests and server integration tests,
warning-denied persistence/server Clippy, and `make check` passed. Production files are
below the unchanged 800-line gate.

## API verification scope

When a reachable flow specifically needs an API-backed provider, the allowed paid/provider-specific candidates are the official DeepSeek API and the designated Flash provider path. Every other API-backed verification uses only an explicitly free API or free model. Missing credentials, exhausted free quota, or unavailable models fail visibly; they do not trigger a paid substitution or a fallback provider.

## Human-session profile exchange verification: 2026-08-26

The copied production frontend was served unchanged from its production build and
proxied to a disposable canonical Axum/SQLite authority. Computer Use exercised the
real `/join?token=…` entry, profile-required preflight, admission, fresh profile read,
display-name save/re-read, custom-status save/re-read, native file selection, crop,
avatar upload, and avatar re-read. The raw invite, browser credential, room session,
and one-use profile tickets were not copied into committed evidence. The run used no
provider because a human-profile flow does not create an Agent Session.

The run found one real state-boundary defect: the pending guest panel tried to hydrate
a server person profile before admission had issued a session. Commit `8cc1064`
prevents that request and keeps the pending profile non-editable, while the focused
test proves the server hydration starts when the admitted session appears. The same
run correctly remained unready at the separate, still-incomplete human WebSocket
exchange and exposed `/api/account` as 404; neither was counted as profile parity.

After evidence capture the exact Safari tab, preview server, Rust fixture server,
test identity data, temporary fixture state, and uniquely named application bundles
were closed or moved to the recoverable Trash. Ports 5174 and 64126 had no remaining
listener and both unique verification applications reported not running.

## Human-session production-browser connection verification: 2026-08-27

Computer Use first exercised the exact production `/join?token=…` URL against a
disposable canonical Axum/SQLite server. It found that Rust served the copied bundle
only below `/app`; direct invite and pairing entrances from the original were absent.
After exact `/join`, `/join/`, `/pair`, `/pair/`, and root asset service were mounted,
admission succeeded and removed the secret URL token, but the room stayed unready.
The guest had no host-authorized room-directory response and therefore no trusted
server product surface to bind before its typed socket-ticket exchange.

The corrected response supplies the existing server ID, authority lineage, and
product surface. The browser reuses the strict room-directory validator and binds the
surface digest before storing the session or exposing its bearer to the socket. A
surface failure is terminal and cannot fall through to the former failed-join stored
session restore. Focused tests cover fresh join and identity-recovery rejection before
session/profile persistence, and real Axum tests request every exact static entrance
plus a production asset.

The post-review correction makes preflight, join, pairing, and recovery response
variants exact at the transport boundary. Fresh joins are bound to the request ID,
preflight room, and requesting client; recovery is bound to the requested room and
client. Only the server-returned avatar can become the admitted profile projection.
Surface verification now checks the admission generation after the asynchronous
digest and before mutating the lifetime pin or any persisted/UI state. The shared
binder accepts the surface type it actually consumes and no longer fabricates
`rooms: []`. Because Vite retains the
copied desktop-compatible `./` base, the same asset directory is mounted at
`/join/assets` and `/pair/assets` rather than copying files or adding a catch-all.
The static router owns one `no-cache` policy for root, app, asset, join, and pair
responses. Its exact entrance and asset-prefix arrays also derive the signed product
surface, removing independently editable wildcard route strings.

The final disposable run used the production frontend bundle and no provider. In an
isolated Chrome guest, normal admission removed the URL token, rendered the canonical
snapshot and roster, and published `HUMAN_SOCKET_NORMAL_UI_OK`. A distinct Safari
private guest admitted read-only, rendered the same prior message and roster, and
showed disabled posting controls with the read-only explanation. SQLite contained
only room creation, the two participant joins, and the one normal `message_final`;
there was no read-only write. The fixture server was stopped, port 43197 had no
listener, both isolated browser resources were closed, and the disposable databases,
keys, fixture source/build outputs, and production bundle were removed or moved to
recoverable Trash.

This evidence is limited to normal snapshot/posting and read-only snapshot/denial.
It does not claim the remaining one-use/reusable avatar, reload, preference,
leave/revoke/restart matrix, controlled expiry/notification-lag/final-outbound races,
or trusted public ingress.

A fresh post-correction Computer Use run opened the production bundle at the exact
trailing-slash `/join/?token=…` entrance in an isolated Chrome window. The asset-loaded
profile form admitted against the current Axum/SQLite server, removed the URL token,
rendered the canonical snapshot and roster, and published `STRICT_SURFACE_UI_OK`; a
read-only SQLite query found the same single `message_final`. The window, fixture
server, listener, database/key state, temporary example source and Cargo artifacts,
production bundle, and generated sidecar were closed or moved to recoverable Trash.
The subsequent client-binding regression rejects a same-request/same-room response
carrying another `client_id` before it can reach session persistence.
Final `make verify` at `62d191f` passed the unchanged architecture/source/policy gates,
78 frontend files with 393 tests, 16 desktop tests, all workspace tests, warning-denied
workspace and desktop Clippy, and final diff validation. No provider was started.

## Human-session invite browser matrix: 2026-08-27

Computer Use drove the copied production bundle against one disposable canonical
Axum/SQLite authority using four distinct invitations: one-use and reusable normal,
and one-use and reusable read-only. The one-use normal flow selected and cropped a
real project PNG before admission, removed the token, published
`ONE_USE_NORMAL_UI_OK`, saved and freshly re-read the display name and custom status,
retained its current avatar and message across reload, and rejected the consumed link
from a fresh browser identity. The reusable normal flow published
`REUSABLE_NORMAL_UI_OK` and re-entered the same link with the same browser identity,
profile, participant, and history without another use.

Both read-only flows rendered the durable normal messages while keeping composer and
attachment controls disabled. The one-use read-only guest saved a display-name change
but received the canonical avatar-upload denial after selecting and cropping the same
PNG; reload retained the name and denial, and a fresh browser identity could not reuse
the consumed link. The reusable read-only guest re-entered with the same identity and
remained read-only. While that browser stayed open, the exact fixture server process
was stopped and reopened on the same SQLite authority. The UI first displayed its
reconnect state, then automatically restored the authenticated snapshot, history, and
read-only controls.

Read-only SQLite inspection showed one use for each exercised invite, exactly the two
normal `message_final` events, one current normal-profile avatar, no pending pre-join
avatar, no read-only avatar, and no preference row. The guest leave confirmation
reported that `participant.leave` is absent from the bound signed product surface and
wrote nothing. At that matrix cutoff remote preferences remained deliberately
client-blocked, and manager
invite create/revoke was not exercised through fake host authority. All isolated
browser windows and both owned fixture processes were closed; port 43217 had no
listener. The fixture source was removed, its database/manifests and production bundle
were moved to recoverable Trash, and its Cargo package artifacts were cleaned. No
provider or user-owned `.agents/` state was used.

## Remote human preference cutover: 2026-08-27

Public backend commit `8b5d4b1` activates exact admitted-session read/write exchanges
and transaction-bound authorization. Public frontend commit `80243e8` connects the
copied preference controller without sending the durable session credential to the
room-settings target. One additional same-origin exchange POST per operation and the
bounded durable revalidation are the structural cost of keeping the longer-lived
credential out of the target route. The change adds no cache, lock, background task,
persistent frontend state, future-only trait, or configuration layer.

`make verify` at `80243e8` passed the unchanged architecture, source-growth, policy,
format, build, generated-binding, desktop, workspace-test, warning-denied Clippy, and
diff gates. The copied production bundle passed 78 frontend files and 395 tests.

Computer Use admitted a writable guest through the production `/join` entrance,
opened the reachable channel context menu, stored channel mute, and re-read the mute
after both browser reload and an actual server restart on the same SQLite authority.
A separate incognito read-only guest loaded the default preference, attempted a write,
received the canonical server rejection, and rendered the stale-state message after
rolling back to the last server value. Read-only SQLite inspection found only the
writable user's mute row; the rejected read-only attempt created no row.

The exact fixture server, normal and incognito browser surfaces, credential manifest,
SQLite/key state, production bundle, and temporary example source were removed after
verification. No provider or user-owned `.agents/` state was used.

Manual review findings for backend range `88a9e07..8b5d4b1`: none. Web verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`.

Manual review findings for frontend range `8b5d4b1..80243e8`: none. Web verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`.

## Exact participant leave cutover: 2026-08-27

Public commits `d7e1f4a`, `051ab5c`, and `8e52f63` add the exact durable
`participant.leave` transaction, its authenticated WebSocket command/terminal-ACK
boundary, and the original connector's separate bounded HTTP entry point. Correction
commits `eec1462` and `1d07590` charge every non-fresh request identity, pass the
original HTTP JSON value through the common admission path, and make one
persistence-owned predicate the sole exact-empty-payload policy owner. Commit
`708eb54` keeps the exact connection generation alive long enough to drain an
already-delivered terminal ACK through asynchronous WebCrypto verification before
deciding whether to reconnect. It does not make a closed socket sendable or retain a
second connection state owner. The change adds no table, index, cache, background
task, runtime trait, configuration layer, compatibility path, or provider cleanup
state. Commits `565d84b` and `04fac7f` bind a terminal leave ACK to its durable event,
exact room, and exact participant, latch protocol failure across the verification
queue, and recheck that latch after asynchronous frame authentication. An ordinary
server close still drains a frame received before close; a protocol-poisoned
connection cannot consume any queued or already-verifying result.

Computer Use admitted a one-use writable guest through the copied production `/join`
entry, opened the reachable server-leave control, and confirmed the exact leave
dialog. The UI removed the room and returned to the server list. Read-only SQLite
inspection found the exact human session `ended`, participant `left`, one
`participant_left` event, and one `participant.leave` command result. Reload did not
restore the guest. The same database was then reopened by a fresh server process and
reload again did not restore it. The fixture server, listener, database/key state,
credential manifest, temporary source/build outputs, and production bundle were
removed after verification. No provider or user-owned `.agents/` state was used.

The first full verification run exposed a test-only readiness race: two provider
fixture modules independently allowed two seconds for an external request marker,
which was insufficient under the full suite's process load even though the focused
contract passed. Commit `976d6f1` removes both copies and gives their single test-only
owner the existing five-second fixture-readiness class. It returns as soon as the
marker exists, so ordinary test latency is unchanged; no product timeout or runtime
behavior changed. Final `make verify` at `04fac7f` passed architecture, source-growth,
policy, formatting, generated bindings, original CSS, all 78 frontend files with 403
tests including the terminal-ACK verification race regression, all 16 desktop tests,
all workspace unit/integration/documentation tests,
warning-denied workspace and desktop Clippy, and final diff validation.

Manual review findings for participant-leave range `0736884..04fac7f`: (1) a prior
committed leave request ID after reusable-identity rejoin escaped the process debit;
(2) HTTP duplicated exact-empty validation and discarded the original payload before
common admission; (3) the corrected HTTP path still left exact-empty policy expressed
independently by command admission and the authoritative leave parser; (4) asynchronous
WebCrypto verification could lose an already-delivered terminal leave ACK after the
server closed the socket; (5) a protocol failure did not latch across its queued frames;
(6) terminal leave ACK validation did not require a durable event or exact room and
participant bindings; (7) a failure that occurred during asynchronous frame verification
could still allow that frame to commit client state. All seven were fixed by `eec1462`,
`1d07590`, `708eb54`, `565d84b`, and `04fac7f`. Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Web verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`.

## Manager invite backend review: 2026-08-27

The manual review of implementation/test commits `24ca404` and `4db3448` found that
active routing and exposure documents still classified the implemented backend
manager create/revoke controls as incomplete. Documentation commit `b3c8f53`
corrected that status. Its re-review then found two documentation issues: the first
correction generalized create's `ReadyIngress` prerequisite to ingress-independent
revoke, and the trusted-ingress status owner retained the stale backend-incomplete
classification. Commit `c697f5c` corrected both without changing product code. Final
web verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or another
automated security scanner.

## Manager invite native bridge review: 2026-08-27

Manual review of native bridge commit `329d8c1` found: (1) manager authority's
`bootstrap_required` and `bootstrap_repair_required` rejections were flattened into a
persistence failure, which made the desktop treat a valid application rejection as a
broken owned runtime; and (2) current workboard, active-spec, ingress-spec, and exposure
owners still classified the implemented native bridge as incomplete. Commit `c104791`
reused the existing bootstrap-error policy owner and preserved both rejections without
changing other persistence failures. Commit `ded26f5` separated implemented/tested
native commands, capabilities, and permissions from the still-incomplete frontend and
packaged flow.

Web re-review of `ded26f5` found: (3) the active spec assigned operation-grant
consumption to the native bridge instead of the matching HTTP route; and (4)
architecture collapsed distinct local HTTP grants into server-operator authority.
Commit `d7dfb44` corrected both. Daybreaker review of `d7dfb44` then found (5)
architecture still claimed every public session exchange was unreachable; commit
`4bf1938` recorded the verified WebSocket/profile/preference exchanges. Web review of
`4bf1938` then found (6) the active spec claimed an attachment grant that has no current
purpose or route; commit `5db9cdd` left room attachment exchange explicitly incomplete.
Full `make verify` passed at `c104791`; every later documentation correction
passed architecture, source-growth, policy, and diff gates. Final web verdict for
`5db9cdd`: `APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or another
automated security scanner.

## Initialization fixture scheduling correction: 2026-08-28

A full verification run exposed a test-only scheduling cost in
`shutdown_checkpoints_gone_after_aborting_initialization`: its synthetic shell
runtime busy-spun on a release file and could occupy one CPU core while the full
suite was also waiting for the observer WebSocket. The focused contract passed, so
there was no evidence for changing a product timeout or runtime lifecycle rule.
The fixture now sleeps while polling the release file. This preserves its exact
blocked-initialization and shutdown ownership transitions while removing avoidable
test-host CPU contention; the accepted trade-off is at most one second of extra
fixture release latency.

The exact boundary test passed three sequential repetitions, the complete
`agent_session_boundary` target passed all 9 tests, and `make verify` passed the
architecture, source-growth, policy, formatting, generated-binding, original-CSS,
frontend, desktop, workspace-test, warning-denied Clippy, and diff gates. No product
code, timeout, persistence state, provider process, or user-owned `.agents/` state
changed.
