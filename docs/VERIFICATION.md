# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

The active comparison baseline is original
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust
`6624e51edbd71c450497c41812eab23bb0e74770`. Local uncommitted behavior
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

## API verification scope

When a reachable flow specifically needs an API-backed provider, the allowed paid/provider-specific candidates are the official DeepSeek API and the designated Flash provider path. Every other API-backed verification uses only an explicitly free API or free model. Missing credentials, exhausted free quota, or unavailable models fail visibly; they do not trigger a paid substitution or a fallback provider.
