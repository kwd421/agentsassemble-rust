# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

Historical entries below retain their contemporaneous batch and review wording as
evidence. They do not own current commit, push, or review timing; only the active
`Standing project workflow` in `AGENTS.md` does.

The active comparison baseline is original
`d5046473010d1353a81ee38337360e6d98f7bd6f`, audited Rust product behavior
`8a5f75a`, and the pre-correction documentation review baseline `d9e6c06`. Local
uncommitted behavior is never described as public completion. Every completed-slice evidence entry must
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

The frontend reference is original commit `d504647…`. The exact active Rust-only
change allowlist is owned by `docs/FRONTEND_BACKEND_GAPS.md`; this document records
verification methods and observed results without restating or widening that list.
An unlisted controller decomposition, client-owned product state, or visual/product
behavior change is a parity failure.

At fixed desktop and responsive viewports, compare asset identity, selector/class,
component and rendered DOM order, responsive breakpoints, left/right panel widths,
central chat bounds, composer bounds, and left-bottom profile-card position and
overlap. Screenshots support, but do not replace, geometry assertions. Exercise
create stopped, create-and-start, stop, resume/restart, reconnect, and one provider
reply through the copied controls. Re-add remains deferred until its complete
participant/session transition is implemented. A hidden fake, no-op, or fallback is
a failed run.

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
- OpenCode: Muse Spark contributor free.

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

Repository audit D-02 supersedes the approval implication of this historical record.
These tests establish properties of the implemented proof machinery after a peer is
given its key; they do not reproduce a packaged actor that can replace the owned
loopback endpoint or relay its traffic without reading the private-control key.
Receipt and per-frame proof therefore remain unapproved without their separate
positive evidence gates.

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

Manual review findings for `b67f4a2`: none. Web verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Daybreaker verdict:
`APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or another
automated security scanner.

## Manager invite method correction review: 2026-08-28

Manual review of `e946bbb` found one Medium issue: an explicitly supplied empty HTTP
method was treated like an omitted method, so the bridge could request one-use native
authority before accepting the request as POST. Commit `6a0aa2c` defaults only an
actually omitted method and rejects the empty method before native invocation.
Final web verdict: `APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker
verdict: `APPROVE — Critical 0 / High 0 / Medium 0`.

## Frontend invite activation plan review: 2026-08-28

Manual plan review found that: credential consistency belonged before transaction
commit; retained revoke custody did not imply current-origin shareability; ingress
status relations were incomplete; a mutable runtime-resource base had last-writer
races; canonical writable avatar references could not become absolute display URLs;
the native socket ticket needed one strict validator; expiry had to disable Copy
without discarding revoke custody; and an in-flight, unknown, or failed retry outcome
could not reopen Copy. It also required packaged-Tauri and non-guest eligibility
before ingress controller mount, status requests, polling, and actions; immutable
`{server_id, authority_lineage_id, room_id, room_uid}` custody through the
native/private-control/ticket path plus
same-transaction stable-room revalidation; and exact outbound-request, returned
credential, and signed-token-digest-derived `invite_id` binding at create-response
acceptance. The accepted design assigns those policies once across
B1a/B1b/B2/C1a/C1b/C2, including restoration of the exact captured revoke-attempt
source state. Final web Pro verdict: `APPROVE — Critical 0 / High 0 / Medium 0`.
Final Daybreaker verdict: `APPROVE — Critical 0 / High 0 / Medium 0`. The web session
was then switched and visibly verified at very-high reasoning for implementation
reviews. Neither review used Deep Scan or another automated security scanner.

Review of published plan record `396169a` found two Medium documentation defects
before implementation. Daybreaker found that the record omitted the approved
packaged-Tauri/non-guest eligibility, exact server/lineage/room/room-UID custody and
same-transaction revalidation, and exact request/credential/invite-ID digest binding,
and that future stages were written as already completed. The independent web review
found that B2 incorrectly rejected the existing confirmed-clear stable-entry state,
whose canonical representation is stable `ready` with an empty stable URL and an
inactive direct target. This correction restores both sets of invariants without
changing product code or adding another policy owner.

Review of correction `52de760` found three remaining boundary defects. First,
create-response acceptance compared the returned join URL with live ingress even
though ingress can independently stop after the invite transaction commits; that
could discard revoke custody for a real invite. Second, the plan required tuple
echoes from private-control, Tauri-ticket, and HTTP responses whose exact current
contracts do not contain them. Third, B2 delayed packaged-host/operator eligibility
until mutation while coupling server-wide ingress control to room authority. The
corrected plan validates the returned join URL intrinsically and defers live-origin
shareability to C2, retains the room tuple only through request/grant/transaction
owners, and separates pre-mount server-wide ingress eligibility from per-room invite
eligibility. Web verdict: `REVISE — Critical 0 / High 1 / Medium 2`. Daybreaker
verdict: `REVISE — Critical 0 / High 0 / Medium 1`. Neither review used Deep Scan or
another automated security scanner.

Correction `bc752d6` separated those owners and retained committed invite custody.
Final web verdict: `APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker
verdict: `APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or
another automated security scanner.

## Frontend operator dispatch and profile provenance: 2026-08-28

The prior desktop client projected socket, operator, and central-registration grants
through one mutable HTTP base. Two operator grants completing in the same microtask
turn could therefore let the first request continuation observe the second grant's
base. Profile rendering also derived absolute attachment URLs from that global value,
so canonical writable profile state and the request that supplied its display origin
did not have one owner.

B1a dispatches every operator and central-registration request with its own strictly
validated grant base. Profile GET and POST now publish one atomic
`{profile, displayResourceBase}` snapshot, while stored and submitted avatar values
remain exact relative `view` references. One shared parser owns the current relative
`view`/`download` attachment forms for profile and room-dock consumers; a pure
resolver combines a validated reference with the immutable response base only for
rendering. One component-owned generation rejects older fetch or save results. The
change adds no network request, retry, timer, cache, durable state, compatibility
path, or background task. Its bounded in-memory cost is one origin string and one
integer per mounted user panel; its security effect is to remove ambient mutable
authority from operator dispatch and prevent absolute or malformed avatar references
from reaching profile persistence.

The deterministic concurrent-grant regression and the profile provenance, relative
serialization, malformed-reference, stale-generation, startup, and persistence
contracts passed as 26 focused tests. Full `make verify` passed architecture and
source-growth gates, formatting, generated bindings, original CSS verification, all
79 frontend files and 410 tests, all 16 desktop tests, every workspace unit,
integration, and documentation test, warning-denied Clippy, and diff validation. The
production main bundle remained one existing large chunk at 768.01 kB; no speculative
code-splitting change was added without a measured runtime bottleneck.

## Frontend public-ingress control activation: 2026-08-28

The B2 series from `e5c9aba` through `2b97a7c` replaces the copied Host-token and
mutable public-URL controls with the existing private-control-issued, one-use
server-operator tickets and the implemented no-store status/Start/Stop routes. One
strict parser owns the complete ingress status relations. Packaged-Tauri plus
non-guest local-operator eligibility is checked before the controller mounts any
native request, network request, poll timer, or mutation handler. One controller
generation owns status publication and its single cancellable wait; closing,
reopening, Start, Stop, or a superseding invite operation retires older work. No
manual-origin compatibility path, retry, background task, durable browser state, or
second ingress authority was added.

The final ordering correction addresses an observed mutation race rather than a
speculative optimization. Once an older Start HTTP request had been dispatched, a
newer Stop could reach Rust first and observe no active generation; the delayed Start
could then create a tunnel after the UI had published `stopped`. Each in-memory
server-operator grant now carries an internal nonzero issuance sequence. The existing
`ManagedLifecycle` mutex is the sole applied-order owner and permits mutation only
for a greater sequence; an older or equal Start/Stop returns the current status. The
sequence is not exposed on either wire, resets with the same process-local ticket
store and lifecycle after restart, and fails closed rather than wrapping.

The accepted cost is one 8-byte atomic counter per `TicketStore`, one bounded
`Option<NonZeroU64>` in the managed lifecycle, and eight bytes in each live
server-operator ticket. The relaxed atomic establishes only unique monotonic issuance;
the existing lifecycle mutex still serializes mutations. There is no added disk I/O,
network round trip, process, timer, or long-lived task. The frontend production main
bundle at final verification was 771.09 kB; the existing large-chunk warning remains
without speculative code splitting.

The final `make verify` passed architecture, source-growth, policy, formatting,
generated-binding, original-CSS, build, workspace-test, warning-denied Clippy, and
diff gates. All 82 frontend files with 475 tests, all 16 desktop tests, domain 23,
persistence 171, protocol 4, provider 120, server 83, and every integration and
documentation test passed. The `control_pipe` suite passed 9/9 and proves the actual
process/TCP ordering boundary by issuing an older Start grant and a newer Stop grant,
sending Stop before Start, observing the managed child and descendant stop, retaining
`stopped`, and rejecting the public `/join`; `ingress_boundary` passed 5/5. Packaged
Computer Use remains deferred to the complete C1/C2 activation and no Deep Scan or
other automated security scan ran.

Manual review findings across the correction series included: nested Rust error envelopes
lost their exact code/message; the frontend rejected Rust-valid `*.localhost` origins;
the completed B2 cutover left the workboard claiming that B2 was still next; retired
async generations could retain timers or publish state; an adjacent invite or pairing
operation superseding Start could leave `publicAccessTransition` at `starting`; a
native ticket that resolved after retirement could still dispatch HTTP; Stop was
unavailable during the server starting phase and while Start awaited its first server
response; independent Start/Stop HTTP requests could mutate Rust in reverse user-intent
order; and the first test-fixture split placed a child module at Cargo's integration-test
root, where all-target autodiscovery compiled it as a second crate. The same
origin-boundary correction separately caught a trailing-dot exact localhost form during
implementation; that was a self-found correction rather than a manual-review finding.
The listed commits correct each owning boundary. Final web verdict for `2b97a7c` and
`e2bd739..2b97a7c`:
`APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker verdict for both scopes:
`APPROVE — Critical 0 / High 0 / Medium 0`. Neither final review used Deep Scan or
another automated security scanner.

## Frontend admission intent custody correction: 2026-08-28

The browser entrance and admission-custody series from `8a8b47d` through `fdb4e49`
removes the copied query-preview authority and binds preflight, admission request,
browser identity, durable session acceptance, and retry classification to their real
owners. Corrections `bccb7ff`, `dc067f4`, `72e9f22`, `291e890`, `3c7127f`, and
`fdb4e49` close the review findings without adding a fallback, compatibility path, or
second admission owner.

The final browser owner stores at most one record under the existing
`agentsassemble.roomAdmissionIntent.v1` session-storage key, capped at 8 KiB. A
network-eligible request is `pending`. After a durable session is accepted or an
exact definitive terminal result is observed, that same owner attempts one retirement
operation. A verified `settled` write is followed by best-effort removal. If the write
cannot be verified, the owner instead attempts and verifies direct removal of the
observed pending record. Failure of both remains unresolved; terminal retirement
exposes explicit cleanup retry, while completed-session retirement does not claim
that UI. A settled record surviving removal is compact and cleanup-only. Reload,
expiration or clearing of the separately owned RoomGuestSession local-storage
record, and later invite navigation cannot turn that marker back into a `/join` request.
Direct external deletion of the admission-intent session-storage key erases the
marker and is outside this guarantee. A matching terminal record restores only its
explicit non-retryable result. A completed-session record permits a different invite
to continue after RoomGuestSession evidence has been removed. A still-pending record
for another invite remains fail-closed unless exact completed-session evidence
matches the invite credential, current browser credential, and client ID.

No raw invite, device, or session bearer is stored in the intent. Ordinary
load/create computes two SHA-256 fingerprints; the exceptional historical
completed-session comparison computes two additional fingerprints only after a
pending mismatch. Each asynchronous digest boundary re-reads and compares the exact
stored record before overwrite or removal, preventing stale work from deleting a
newer intent. The change adds no database row or index, cache, lock, timer, background
task, configuration layer, or second storage key. The pure admission state/reducer is
separated from the effect hook; storage lifecycle remains in the intent module.

The TCP test correction has a separate observed cost. Its prior managed-ingress
readiness helper exhausted 256 immediate polls in about 1.3 seconds while advertising
a five-second readiness window. One five-second deadline owner with a 10 ms poll
interval now returns as soon as readiness is observed, without changing product
timeouts or ingress behavior.

Focused admission tests passed 27/27. The production-browser startup boundary passed
10/10: a definitive terminal response followed by blocked deletion and full reload
kept the join count at three, exposed cleanup-only retry, and removed the record after
storage recovered without a fourth join. A successful invite whose cleanup was
blocked left completed settlement; after the separate RoomGuestSession local-storage
record was removed entirely, a different invite still reached preflight with no
session bearer and retired the stale settlement.

Final `make verify` at `fdb4e49` passed architecture, source-growth, policy,
formatting, generated-binding, original-CSS, production-build, workspace-test,
warning-denied Clippy, and diff gates; all 81 frontend files with 451 tests, all 16
desktop tests, and every Rust unit, integration, and documentation test passed. The
real TCP suite exercised managed/manual public ingress, process readiness and
revocation, peer/Host/Origin/proxy boundaries, incomplete headers, and admission
bounds. The production main bundle was 774.01 kB; no speculative split was added
without a measured runtime bottleneck. No Computer Use resource or provider process
was required for this browser-storage/reload correction, and no Deep Scan or other
automated security scanner ran.

Manual review findings were: ambiguous response-loss errors could be classified as
terminal; live same-room session evidence was checked after the invite gate; cleanup
could discard retry custody or retain it indefinitely; expired session bearers could
reach preflight; reducer/effect ownership was too entangled; and terminal/completed
settlement depended on volatile reducer or separately expiring session state. The
commits above correct each owning boundary. Final web verdict for `fdb4e49` and
`8a8b47d..fdb4e49`: `APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker
verdict for both scopes: `APPROVE — Critical 0 / High 0 / Medium 0`. Neither review
used Deep Scan or another automated security scanner.

Cross-review of documentation commit `5729aef` found one Medium overstatement: it
described settled-write-before-delete as unconditional and did not distinguish
RoomGuestSession local-storage cleanup from direct deletion of the admission-intent
session-storage key. The correction records the actual verified-write-or-verified-
direct-removal branches and leaves both-failed retirement unresolved; product code and
the approved implementation verdict are unchanged.

Web re-review of correction `ae8486a` found one further Medium documentation issue:
the unresolved branch was described as always exposing cleanup retry even though only
terminal settlement dispatches that UI operation. The completed-session branch does
not expose it. This is a documentation correction only; the approved product code is
unchanged.

## Manager invite authority transport: 2026-08-28

The previous native invite bridge requested manager create/revoke authority with only
a mutable room ID. A room can be recreated under a stable ID, and a local authority can
be replaced under a different server or lineage, so that request shape could not bind a
future ticket or mutation to the exact room-directory authority the user selected.

The first C1a unit adds no new authority source or durable state. A future active caller
must supply the exact `{server_id, authority_lineage_id, room_id, room_uid}` emitted by
the currently verified room directory; this transport-only unit does not yet select that
source or activate the controller. The frontend rejects unknown fields, noncanonical
UUIDs, and a nonexact room ID before native invocation. Tauri deserializes the same
strict in-memory `ManagerRoomAuthority` object, repeats canonical UUID and domain room-ID
validation, and passes that object unchanged to the create/revoke shared private-control
serializer. Ticket responses retain their existing shape and contain no tuple echo. The
controller does not call this bridge yet, and server issuance/transaction revalidation
remain explicitly incomplete rather than accepting the transported tuple as authority.

The measurable resource change is four bounded strings on the low-frequency manager
invite request and their existing JSON serialization. There is no database row, disk
write, cache, timer, process, task, network round trip, fallback, compatibility path, or
general authority framework. The tuple object also avoids four positional native
arguments and one duplicate create/revoke validator. This accepts a small per-request
allocation to preserve exact provenance; no runtime optimization was added without a
measured bottleneck.

Focused protocol, desktop, and frontend tests prove exact private-control serialization,
canonical native UUID rejection, no native invocation for malformed frontend authority,
separate create/revoke grants, and fixed HTTP routes. Final `make verify` passed the
architecture, source-growth, policy, formatting, generated-binding, original-CSS,
production-build, workspace-test, warning-denied Clippy, and diff gates. All 82 frontend
files with 476 tests, all 19 desktop tests, domain 23, persistence 171, protocol 5,
provider 120, server 83, and every integration and documentation test passed. The first
full run caught a `similar_names` architecture/lint violation in separate `room_id` and
`room_uid` native parameters; the final design fixed the cause by making the tuple one
input object rather than adding a lint exception. No Computer Use resource, provider,
Deep Scan, or other automated security scanner ran.

The first web review returned `REVISE — Critical 0 / High 0 / Medium 1`, and the first
Daybreaker review returned `REVISE — Critical 0 / High 0 / Medium 2`. Their findings
were that the frontend accepted room identifiers that the Rust domain owner rejects,
while native Serde silently discarded unknown nested
fields and the Tauri commands decomposed the tuple into four positional strings before
reconstructing it. The correction gives the frontend one small canonical-room-ID owner
matching the Rust character and length rules, uses it for both room-directory acceptance
and invite authority, and makes the one native tuple type reject unknown fields and stay
owned through validation and serialization. This adds no compatibility path, policy
option, durable state, or generalized authority framework.

A first correction attempt imported the complete room-directory contract into the hot
desktop bridge solely to reuse room-ID validation. The production main JavaScript chunk
grew from 771.09 kB to 843.30 kB and the original lazy CSS chunks no longer matched the
copied frontend. Moving only the shared 128-code-point canonical-ID calculation to its
own leaf module restored the 771.09 kB main chunk (232.32 kB gzip) and the original CSS
chunk set without changing accepted values. Frontend production build and Tauri asset
embedding must be verified sequentially because the latter consumes the former's output;
the final serial `make verify` passed every gate and the counts recorded above. Final
manual approval is intentionally not claimed until this correction commit is pushed and
both reviewers recheck it.

The second web and Daybreaker reviews each returned
`REVISE — Critical 0 / High 0 / Medium 1`: ECMAScript `trim()` and Rust
`str::trim()` do not own the same whitespace domain. U+FEFF is preserved by Rust but
removed by JavaScript, while U+0085 is trimmed by Rust but preserved by JavaScript.
The first form made a reachable Rust-created room disappear at frontend directory
acceptance; the second let a Rust-noncanonical authority reach native invocation.

The correction keeps the leaf owner but explicitly implements Rust's Unicode
`White_Space` boundary and Unicode-scalar count, rejects isolated UTF-16 surrogates
that a Rust string cannot represent, and removes the later ECMAScript trimming that
could mutate an already accepted directory or socket room ID. Focused tests pin both
whitespace divergences, 128 supplementary-plane scalar values, a rejected 129th value,
an isolated surrogate, the invite pre-effect boundary, and unchanged room-dock
projection. This remains local validation only: no state, task, timer, network or disk
work is added. The production main chunk is 771.05 kB (232.32 kB gzip), and the original
CSS chunks remain exact. The final serial `make verify` passed every repository gate,
83 frontend files with 480 tests, 19 desktop tests, and the unchanged complete Rust and
TCP boundary suites. Web re-review approved both `48eb5bf..525f754` and
`5a032db..525f754` with `Critical 0 / High 0 / Medium 0`.

Daybreaker re-review of `525f754` found one remaining Medium lifecycle defect outside
the directory/socket normalization already corrected: room-dock persistence still
passed a verified room ID through ECMAScript `trim()` and UTF-16 `slice()`. That could
change U+FEFF after reload and truncate a valid 128-supplementary-scalar identifier to
64 scalars. Persistence now delegates acceptance to the same canonical room-ID owner
and stores the accepted string byte-for-byte. Focused tests prove exact storage reload
for both values and prove U+FEFF reaches the native invite-ticket invocation unchanged.
No second policy, compatibility path, state, task, or durable owner was introduced.

The first full rerun could not complete because accumulated regenerable Cargo build
artifacts exhausted the local volume while Vitest was writing its temporary report.
Only the workspace and desktop `target` artifacts were cleaned; source, settings,
credentials, and user-owned files were untouched. This reclaimed 51 GiB of available
space. A cold serial `make verify` then passed every architecture, source-growth,
policy, formatting, generated-binding, original-CSS, production-build, workspace-test,
TCP/integration, warning-denied Clippy, and diff gate: 83 frontend files with 482 tests,
19 desktop tests, domain 23, persistence 171, protocol 5, provider 120, server 83, and
all integration and documentation tests. The production main chunk is 771.11 kB
(232.38 kB gzip). Final approval for this last persistence correction is withheld until
the pushed commit is re-reviewed by both reviewers.

Daybreaker approved `525f754..817ff0d` and `5a032db..817ff0d` with
`Critical 0 / High 0 / Medium 0`. Web review returned
`REVISE — Critical 0 / High 0 / Medium 1`: a cached room with the right stable
`(serverId, roomUid)` but a previously transformed `meetingId` matched the verified
directory entry, yet reconciliation retained that stale ID. The stale value could then
be persisted again and used by the active socket or ticket path.

The correction makes verified-directory reconciliation replace the operational
`meetingId` and includes it in its change decision. The local dock `id` remains stable
so active selection references are not invalidated during hydration. The native
derived-cache writer no longer owns a second generic-text room-ID policy: it calls the
domain `validate_room_id` owner and accepts only an unchanged canonical result. No cache
version, migration, fallback, or compatibility branch was added. Tests prove that a
stable-identity cached alias is replaced by the verified directory ID, that U+FEFF and
128 Unicode scalars reach the native cache unchanged, and that a value normalized by
the domain owner is rejected instead of rewritten.

The final serial `make verify` passed all repository gates, 83 frontend files with 483
tests, 20 desktop tests, and the unchanged complete Rust, TCP, integration, Clippy, and
documentation suites. The production main chunk is 771.18 kB (232.40 kB gzip). This
correction adds no runtime task, database query, durable policy state, or network round
trip; it replaces derived fields during an existing O(rooms) merge and removes duplicate
normalization at the native write boundary. Final approval remains withheld until this
correction is pushed and both reviewers recheck it.

Daybreaker approved `817ff0d..8f71bb6` and `5a032db..8f71bb6` with
`Critical 0 / High 0 / Medium 0`. Web review returned
`REVISE — Critical 0 / High 0 / Medium 1`: replacing the local dock `id` together with
`meetingId` could orphan the separately held active-room/menu references during deferred
hydration and select another room. That extra replacement was not required to correct
the operational room authority. The correction therefore preserves the stable local
UI key and replaces only `meetingId`; it adds no reference-remapping framework or new
state. The serial `make verify` again passed the same complete gate and test inventory;
the production main chunk is 771.16 kB (232.39 kB gzip). Final approval is withheld
until this minimal correction is pushed and re-reviewed.

Daybreaker and the independent web reviewer at verified `매우 높음` each approved
both `8f71bb6..754198d` and cumulative C1a transport scope `5a032db..754198d` with
`Critical 0 / High 0 / Medium 0`. Their final review found no remaining structure,
duplicate-policy, overengineering, authority, or lifecycle issue. Neither review used
Deep Scan or another automated security scanner.

## Manager invite server authority binding: 2026-08-28

The prior server issued manager invite tickets after resolving only a mutable room ID,
and the HTTP mutation later reconstructed manager identity from that generic grant. A
same-ID room generation, replaced bootstrap lineage, or different server between
frontend capture, ticket issuance, and mutation could therefore escape the intended
exact-directory provenance. C1a now makes persistence the single current-authority
owner: it resolves the server ID, bootstrap lineage, stable room UID, canonical room
ID, and local manager together. Private control compares every captured field before
issuing a separate create- or revoke-only one-use ticket, and ticket consumption moves
the immutable snapshot into the owning invite transaction. That transaction resolves
the current authority again and rejects any mismatch before insert or revoke. Create
also compares both newly issued credential fingerprints with the decoded returned row
before commit.

The implementation adds no schema, migration, compatibility path, fallback, cache,
timer, task, tuple echo, post-commit orphan check, or generic authority framework. A
low-frequency issuance or mutation performs only the existing bounded SQLite
transactional reads plus one active-room read needed to obtain the stable UID; the
ticket retains a few bounded strings and one UUID in memory until its existing expiry.
This small cost closes a concrete stale-authority/TOCTOU threat without changing invite
scope, expiry, use limits, ready-ingress creation, ingress-independent revoke, one-use
ticket semantics, or HTTP response shape.

A repository-wide policy search confirmed that the effective invite-use ceiling remains
owned by `effective_human_invite_use_limit`. The schema deliberately keeps only the
nonnegative storage-shape CHECK: adding a product-limit upper CHECK would duplicate that
changeable policy and conflict with the single owner established in `f83707c`. Atomic
admission already predicates its increment on the computed owner limit, and durable
decode rejects a count above it, so no concrete additional write or admission threat
justifies restoring duplicate DDL policy.

Focused persistence tests replace a room UID under the same room ID and prove a stale
snapshot can neither insert nor revoke, with row count and prior revoke state unchanged.
The owned child-process stdin/stdout control-pipe test uses the actual created directory
tuple and rejects independently changed server, lineage, and room UID with
`room_authority_changed`. It also obtains both exact grants through that pipe, consumes
them through the child's real HTTP TCP create/revoke routes, then reopens the database
after control-pipe EOF and proves the exact invite remains durably revoked. HTTP boundary
tests retain consume-before-body, wrong-purpose, wrong-room, ready-ingress,
credential/result, and exact revoke behavior. Warning-denied
workspace Clippy and formatting checks pass. Final serial `make verify` passed every
architecture, source-growth, policy, formatting, generated-binding, original-CSS,
production-build, workspace-test, TCP/integration, warning-denied Clippy, documentation,
and diff gate: 83 frontend files with 483 tests, 20 desktop tests, domain 23,
persistence 172, protocol 5, provider 120, and server 83 all passed. The production
main chunk remains 771.16 kB (232.39 kB gzip). No Computer Use resource, real provider,
Deep Scan, or other automated security scanner ran. Manual approval remains withheld
until the complete candidate is pushed and reviewed.

Web review of `754198d..904b873` and cumulative `5a032db..904b873` found one Medium
verification-scope error: the child stdin/stdout control pipe had been mislabeled TCP,
and the separate TCP manager-route test issued tickets directly. The correction above
joins real control issuance to real TCP create/revoke and restart persistence without
adding product state or a test-only authority. The same-ID/new-UID stale transition
remains proved at the persistence transaction owner. It is intentionally not injected
into the running child: current product behavior has no room-generation replacement
operation, while an out-of-band SQLite writer would violate the exclusive single-writer
contract and a test-only control mutation would be fake authority. Daybreaker's first
review found no Critical, High, or Medium issue; both final approvals remain withheld
until this correction is pushed and re-reviewed.

Final correction review found no further issue. Web and Daybreaker each approved
`904b873..e006e0d`, exact `754198d..e006e0d`, and cumulative
`5a032db..e006e0d` with `Critical 0 / High 0 / Medium 0`. Neither review used Deep
Scan or another automated security scanner.

## Strict frontend manager-invite exchange: 2026-08-28

Before C1b, the copied controller's human create path still used the unchecked generic
moderator helper, while the implemented Rust authority required a create-only native
grant. A delayed grant could not be distinguished from a request already sent, and
typed JSON alone did not bind a returned room, credential, public URL, invite ID, or
expiry to the exact request. Retrying or copying after such ambiguity could duplicate
an invite or expose a substituted credential.

One frontend API owner now validates the captured manager tuple and canonical outbound
human intent, obtains the exact native create or revoke grant, marks the instant before
`fetch`, and parses the complete response. Create acceptance requires exact fixed and
echoed fields, a lowercase 16-hex invite ID equal to the SHA-256 prefix of the exact
signed token, an independent canonical join code, one canonical non-loopback HTTPS
`/join` URL
accepted by the existing shared ingress-origin owner whose only query is that join code,
the existing exact loopback room origin, and a finite canonical server timestamp. It
retains only immutable authority, invite ID, validated URL/origin, and
exact-plus-derived expiry; raw signed and join-code fields get no second storage owner.
Revoke accepts only exact success or exact
`invite_not_found` as terminal. A grant or operation guard failure before `fetch` is
`proven_not_dispatched`; transport, HTTP, JSON, or binding failure after dispatch is
`outcome_unknown`.

This closes an observed authority and ambiguity gap without another network round
trip, database write, disk artifact, cache, timer, task, compatibility path, or
fallback. The low-frequency accepted create adds one WebCrypto SHA-256 calculation
over the server-issued token plus exact object-key, URL, and timestamp validation. Exact
microsecond text remains custody authority; the derived millisecond value rounds down
sub-millisecond precision so later presentation cannot outlive the server instant.
The trade-off is deliberate fail-closed uncertainty after any dispatched request;
C2 must retain and present that state rather than retry automatically.

Focused manager-contract, native dispatch-guard, and ingress-origin tests passed 30
tests. The complete `make verify` passed architecture/source-growth/policy gates,
formatting, generated bindings, original-CSS verification, production build, all 84
frontend files with 489 tests, 20 desktop tests, domain 23, persistence 172, protocol
5, provider 120, server 83, every integration/TCP/doc test, and warning-denied Clippy.
The production main chunk remained 771.16 kB (232.39 kB gzip). No Computer Use
resource, provider, Deep Scan, or other automated security scanner ran. Manual review
is withheld until this independently buildable candidate is committed and pushed.

Daybreaker's first review of `e006e0d..80b4f50` and cumulative
`5a032db..80b4f50` found three Medium contract mismatches. The C1b parser had added a
second `.localhost` suffix rejection that the shared frontend and Rust ingress owners
did not impose, accepted only four-digit years although the server can emit Chrono's
signed expanded years, and treated a shape regex as proof of the signed credential
although the credential owner also requires a 4-KiB maximum, canonical Base64URL, and
an exact 32-byte signature. Web review of the same ranges also returned
`Critical 0 / High 0 / Medium 3`: it independently found the incomplete ingress owner,
found that caller-owned manager authority remained a mutable alias across grant and
dispatch, and found stale prose assigning controller activation to C1b instead of C2.
All findings were reproduced. Final approval remains withheld until the correction is
pushed and re-reviewed.

The correction removes the second C1b host policy and completes non-loopback validation
at the existing Rust and frontend ingress-origin owners, including `.localhost`
subdomains and IPv4-mapped IPv6 loopback or unspecified addresses. One response parser
now recognizes the complete canonical server year form, validates the calendar by round
trip, and still rounds positive sub-millisecond expiry down. The shared desktop parser
now returns one frozen value snapshot, so native grant issuance, dispatch, response
validation, and retained custody cannot observe different caller-owned authority tuples.
The active spec now assigns only the stateless exchange to C1b and controller/UI custody
to C2.

The rejected host forms were not harmless public aliases: a shared URL under
`.localhost` or a mapped local address resolves at the recipient's machine and can send
the exact join credential in its query to a recipient-local HTTPS endpoint instead of
the host. The accepted trade-off is to reject those previously representable but
non-public manual configurations before ingress readiness and invite insertion; ordinary
public DNS and nonlocal numeric origins preserve their existing contract. The Rust owner
lives with the existing host/IP normalization in `ingress_trust`, while
`CanonicalPublicOrigin` and the frontend status/parser consume it rather than owning
another invitation-specific rule.

Current invite prefix, byte-length, and maximum-size wire values move from server-local
constants to the protocol-generated frontend binding, and one frontend Base64URL
mechanism replaces three production copies while product owners retain their own sizes
and error semantics. This adds no network request, persistent state, cache, timer, task,
compatibility path, or fallback. The consolidated mechanism also reduced the production
main chunk from 771.16 kB (232.39 kB gzip) to 770.74 kB (232.22 kB gzip). Final serial
`make verify` passed every mandatory architecture/source-growth/policy/format/generated
binding/original-CSS/build/test/Clippy/diff gate: all 84 frontend files with 495 tests,
20 desktop tests, domain 23, persistence 172, protocol 5, provider 120, server 83, and
every integration/TCP test passed. No Computer Use resource, provider, Deep Scan, or
other automated security scanner ran. Both final manual approvals remain pending.

The next web re-review found one Medium wire-domain mismatch: JavaScript `Date`
accepted years outside the pinned Chrono server's `-262143..=262142` domain. Commit
`cfaf832` makes the protocol own and generate those two bounds, makes the frontend
consume them before its existing canonical calendar and precision checks, and adds a
server assertion that the values equal `NaiveDate::MIN/MAX` plus frontend endpoint
acceptance and adjacent rejection regressions. The complete serial `make verify`
passed with 496 frontend tests and 84 server tests; all other counts and the unchanged
770.74 kB / 232.22 kB-gzip production chunk remain as recorded above. Daybreaker
approved the correction, exact C1b, and cumulative C1 with C/H/M 0/0/0. The web
reviewer found the code correction sound but returned C/H/M 0/0/1 because WORKBOARD
and this verification record had not yet advanced to `cfaf832`; final web approval
therefore remains pending on this current-state-only correction. No Computer Use,
provider, Deep Scan, or other automated security scanner ran.

The current-state correction `7ad8f28` then received final web and Daybreaker approval
for both its docs-only range and exact C1b, each with C/H/M 0/0/0. C1b is therefore
closed without activating the copied controller.

## Directory-owned manager room authority: 2026-08-28

Before C2, controller code could combine a caller-retained mutable `RoomDockItem` with
separately retained server authority. That split could mint a native manager request
from an unconfirmed directory, a remote room, an old authority lineage, or a room UID
that changed after the caller captured the room object.

The room directory now owns one monotonic publication epoch across layout-reserved
hydration, explicit verification, refresh, and room creation. Actual post-ticket POST
and GET dispatch, every awaited continuation, retry custody, visible issue publication,
global authority binding, and final UI publication require the same current continuity.
Only a strict active payload may create frozen manager-map entries, one per exact
eligible local dock; mutable dock fields cannot replace their tuples. Superseded work
cannot dispatch, publish, alert, or clear the retained exact room-create intent.

The Very High review of `26bf183` found one Medium performance issue: the unpaged directory's
manager-map construction rescanned every active payload row for every eligible dock,
making valid large publications synchronous O(A²) main-thread work. `3cfaf4b` replaces
that scan with one strict active-room lookup owner, preserving all duplicate and
fail-closed association checks while reducing construction to O(payload+docks) time
and O(active payload) temporary memory. The production main chunk moved from 774.56
to 774.55 kB; gzip remained 233.37 kB. No network, disk, timer, task, fallback,
migration, compatibility path, or second policy owner was added.

Focused correction verification passed 42 tests. Final `make verify` passed every
mandatory gate, all 84 frontend files with 508 tests, desktop 20, domain 23,
persistence 172, protocol 5, provider 120, server 84, and every integration/TCP/doc
test. No Computer Use resource, provider, Deep Scan, or automated scanner ran. The
resulting C2 foundation through `3cfaf4b` received final web and Daybreaker approval
with `Critical 0 / High 0 / Medium 0`. Retained invite custody,
controller/UI activation, and packaged Computer Use verification remain incomplete.

## Packaged public-frontend resource correction: 2026-08-28

The first isolated release-package run reached the copied manager UI, started the
owned managed tunnel, and durably created a human invite, but a separate real Chrome
browser received HTTP 404 at the exact `/join` entrance. The direct loopback route was
also 404. Process evidence showed that the packaged Tauri owner started its sidecar
without `--frontend`; the WebView's embedded assets did not give the Axum authority a
filesystem frontend to serve. The UI therefore reported a ready public ingress and
issued a real credential for an unreachable product entrance.

Commit `6a8b5f1` uses Tauri's existing resource bundler to place the already built
`frontend/dist` at `Resources/frontend`, and the desktop runtime passes that single
fixed path to the sidecar. The existing server owner still canonicalizes the path,
requires `index.html`, mounts the declared static surface, and fails startup when the
resource is absent. No second server, asset copier, embedded-files framework, runtime
fallback, compatibility path, cache, task, timer, network request, or policy owner was
added. The observed disk cost is one 1.2 MiB resource directory in the package; no CPU,
memory, process, or latency improvement is claimed.

The rebuilt isolated release package contained the exact resource and launched the
sidecar with its absolute bundled path. A direct request to `/join` changed from the
missing-route 404 to the expected 403 without trusted proxy evidence. Through the real
managed tunnel, the packaged UI created a one-use normal invite without rendering its
bearer, copied it only through the exact-key guarded action, and admitted a fresh Chrome
incognito guest. Admission removed the token from the browser URL; the guest published
`C2_PACKAGE_JOIN_OK` over the authenticated room socket and the packaged host received
it. A fresh browser identity was rejected when it reused the consumed link.

The same packaged UI then selected five uses and created a new retained record. The
prior record remained visible as `이전 초대` with Copy disabled. Copying and explicitly
revoking the current reusable record changed it to `폐기됨`, disabled both actions, and
made its already copied link fail in a fresh browser. Read-only SQLite inspection found
exactly `(max_uses, use_count, revoked) = (1, 1, 0)` and `(5, 0, 1)`. This proves only
the managed-ingress normal invite/message, one-use rejection, retained replacement, and
revoke rejection flows; read-only, avatar, reload, preferences, leave, and restart
remain required before packaged C2 completion.

Full `make verify` passed all architecture, source-growth, formatting, generated-binding,
original-CSS, frontend, desktop, workspace, TCP/integration, warning-denied Clippy, and
diff gates: 84 frontend files with 516 tests, 20 desktop tests, domain 23, persistence
172, protocol 5, provider 120, server 84, and every integration/documentation test.
Both exact verification apps, their owned sidecars and managed tunnels, all four
incognito windows, isolated application data, and temporary package configuration were
closed and removed or moved to recoverable Trash. The Computer Use kernel was reset.
No provider, Deep Scan, or automated security scanner ran. At that checkpoint, this
local feature commit was queued under the then-active three-feature-or-2,000-line
manual review rule.

## Packaged invite-scope and completion matrix: 2026-08-28

The next isolated packaged run found one stale Rust settings guard through the real
host UI: selecting the copied read-only invite option returned
`Room invite scope is unavailable until invite admission exists.` The original
comparison commit accepts `invite_scope` as an ordinary strict appearance partial
update, while current Rust already owned room-manager authorization, durable invite
scope, admission, read-only capabilities, and the copied control. Commit `1aca717`
therefore removes only that obsolete unsupported branch and pins the activated update
with one domain contract test. Canonical `room`/`read_only` validation and the existing
`room.manage` transaction owner remain unchanged. A room setting supplies future invite
creation only; an issued invite row retains its captured immutable scope.

The rebuilt `AgentsAssemble C2 Matrix Verify` package then exercised both scopes through
its managed tunnel and separate private browser identities. A five-use read-only invite
accepted a pre-join cropped PNG, admitted a fresh guest, removed the invite credential
from browser history, and recovered the same identity on reload and same-browser link
reuse. Display-name changes projected to the lower-left profile and roster. Message and
attachment controls remained disabled, a post-admission avatar upload failed visibly,
and a channel-notification write did not create preference authority. Switching the
room setting back to normal affected only a subsequently issued invite.

That five-use normal invite admitted independent Chrome and Safari private identities.
The Chrome identity published exactly one `C2_NORMAL_REUSABLE_OK` message and saved a
channel mute. The Safari identity saw the durable message, reused the same link without
a second profile prompt, changed its display name, uploaded a cropped avatar, saved a
channel mute, and retained profile, message, roster, and preference state after reload.
The copied server menu then performed exact leave: the room disappeared, the session
lost send authority, and the host roster removed that participant while preserving the
message.

Read-only SQLite inspection matched the UI without reading credential material. The
two issued rows were `(read_only, max 5, used 1, unrevoked)` and
`(read_write, max 5, used 2, unrevoked)`; same-browser re-entry did not consume another
use. Session authority contained one active read-only, one active normal, and the
leaving normal session in `ended`. The three admitted guests each owned one current
profile-avatar asset, no pending pre-join asset remained, no read-only preference row
existed, both normal identities retained their own mute, and exactly one marker message
remained. Normal application quit removed the exact desktop, supervisor, sidecar, and
tunnel. Relaunch recovered the room, message, remaining participants, and normal future
invite scope. A final normal quit removed every owned process; the isolated app, data,
package config, and generated verification sidecar were moved to one recoverable Trash
directory, and the Computer Use kernel was reset.

The correction adds no state, SQL, task, process, allocation owner, disk owner, network
request, or compatibility path; it removes one validation branch, so no measurable CPU,
memory, disk, or latency improvement is claimed. The security effect is limited to
making already-authorized strict settings reach the existing invite owner. Full
`make verify` passed architecture/source-growth/policy gates, generated bindings,
production frontend and original-CSS verification, all 84 frontend files with 516
tests, 20 desktop tests, domain 24, persistence 172, protocol 5, provider 120, server
84, every integration/TCP/documentation test, warning-denied Clippy, and the final diff
check. No provider, Deep Scan, or automated security scanner ran. At that checkpoint,
the two feature commits remained local under the then-active three-feature-or-2,000-line
review rule.

## Room-appearance persistence and HTTP batch review: 2026-08-28

The pushed batch and corrections through `d82b8e2` received manual cross-review.
Daybreaker found two Medium ownership/status defects: three unused public appearance
ticket consumers plus their duplicate internal helper remained beside the production
single-dispatch consumers, and current-state documentation still described completed
invite activation or pending appearance activation inconsistently. The web reviewer
found one further Medium documentation defect: the active appearance specification
described pending private-control/desktop issuance and frontend object-URL custody as
already active.

Commits `96f9724`, `c0fe269`, `eef84a8`, and `d82b8e2` removed the duplicate consumers
and separated active backend behavior from future desktop/frontend acceptance without
adding state, authority, fallback, compatibility, or migration paths. Full `make
verify` passed after the code correction: every mandatory gate, all 84 frontend files
with 516 tests, desktop 20 tests, domain 25, persistence 178, protocol 5, provider 120,
server unit/integration and real TCP tests, warning-denied Clippy, and the diff check.
The final docs-only corrections additionally passed the architecture, source-growth,
policy, and diff gates. Web and Daybreaker then each returned final
`APPROVE — Critical 0 / High 0 / Medium 0`. No Computer Use resource, provider, Deep
Scan, or automated security scanner ran for this backend/review batch.

## Typed desktop appearance issuance candidate: `1d4063c`

Local commit `1d4063c` adds three closed private-control/Tauri/frontend grant
operations for room-appearance upload, exact pending preview, and exact bound read.
The local operator supplies the directory-owned server ID, authority lineage, room
ID, and room UID; upload and pending preview retain that exact manager authority to
the persistence transaction. Asset reads require the canonical `ra_` identifier and
no path or generic operation string selects a ticket purpose.

The real control-pipe test creates a room over the production TCP server, rejects
changed server, lineage, and room UID tuples, uploads a decoded/re-encoded PNG with
the issued grant, reads it with the exact pending grant, rejects a malformed asset
ID, and receives the distinct bound-read response. Purpose-response mismatch tests,
the Tauri permission/command intersection, and the frontend bridge prove that the
three grants cannot substitute for one another and malformed asset IDs reach no
native invocation.

Full `make verify` passed: architecture, source-growth, policy, formatting,
generated bindings, production frontend and original-CSS checks, all 84 frontend
files with 518 tests, 20 desktop tests, domain 25, persistence 178, protocol 6,
provider 120, server 85 unit tests and every integration/TCP suite, warning-denied
Clippy, and the diff check. Issuance adds one bounded private-pipe exchange per
operation and reuses the existing manager resolver; it adds no durable state,
cache, background task, fallback, compatibility path, or provider process. The
copied settings UI's authenticated fetch and object-URL lifecycle remain visibly
incomplete. No Computer Use resource, provider, Deep Scan, or automated security
scanner ran. At that checkpoint, this candidate was still unpushed under the
then-active configured batch threshold.

## Authenticated room-appearance frontend activation: 2026-08-28

Commits `931beda`, `9404a25`, and `2afed29` connect the copied room-settings and room
shell to the already implemented appearance owners. The strict frontend API accepts
only canonical `ra_<32 lowercase hex>?view=1` references, obtains a fresh exact local
grant or remote-session bound-read exchange, validates the complete no-store PNG
response, and exposes only a browser object URL. It has no unauthenticated direct-image
request, generic attachment compatibility path, alternate bearer, retry fallback, or
client-side binding substitute.

One hook owns rendered asset lifetimes. It loads banner and icon only for the active
room, except for the icon needed by each inactive room-rail entry; deduplicates an
identical banner/icon reference within a room; keeps a newly uploaded pending preview
only until the committed bound reference can be read; rejects stale generations; and
revokes object URLs after their replacement has rendered or when the reference, room,
authority, or component lifetime ends. The controller derives the exact current local
manager for every upload instead of retaining a new authority cache. The copied modal
delegates file and slot only and no longer calls the generic lobby-attachment uploader.

The concrete security defects avoided were private canonical references reaching an
unauthenticated image request, an upload grant being confused with a read grant, and
stale object URLs retaining decoded image bytes after their owner changed. The focused
request and hook tests exercise local upload/pending/bound reads, remote bound exchange,
strict metadata/body rejection, same-reference deduplication, inactive-banner
suppression, pending-to-bound replacement, abort and late-result rejection, explicit
retry, and every URL-revocation boundary. The design adds no durable frontend state,
cache, timer, task, SQL, transport fallback, compatibility layer, or generic asset
framework.

Full serial `make verify` passed architecture, source-growth, policy, formatting,
generated-binding, original-CSS, production-build, every workspace/TCP/integration/
documentation test, warning-denied Clippy, and the final diff gate: 86 frontend files
with 530 tests, desktop 20, domain 25, persistence 178, protocol 6, provider 120, and
server 85. The production main chunk changed from 780.71 kB (234.81 kB gzip) before
frontend activation to 787.36 kB (237.05 kB gzip). That 6.65 kB raw / 2.24 kB gzip
increase is the observed code cost; there is no claimed CPU, memory, disk, or latency
improvement. Request-count reductions are limited to the tested owning boundary:
inactive banners are not fetched and identical active banner/icon references share one
read. No real provider, Deep Scan, or automated security scanner ran.

Computer Use drove a fresh isolated release package named
`AgentsAssemble Appearance Verify`, bundle identifier
`app.agentsassemble.rust.appearanceverify0828b`, against its own application data and
an explicitly empty central URL. The real startup UI created profile `Appearance
Verify` and a canonical room. Native file selection uploaded the repository's
`deepseek.png` as banner and `cursor.png` as icon; the copied modal displayed both
saved states, and the room rail, left room banner, and chat introduction rendered the
authenticated images. After normal application quit, relaunch skipped the identity
gate, restored the room, and rendered the same rail icon, banner, chat icon, and
settings preview through fresh bound reads.

Read-only SQLite inspection after restart found exactly two appearance rows, both
`bound`, with null pending owner and expiry and byte sizes 12,016 and 43,016. The room
settings retained two distinct canonical `ra_` view references. A final normal quit
left no packaged app or sidecar process. The Computer Use kernel was reset, and only
the isolated build, Application Support, cache, and WebKit data were moved to the
recoverable Trash directory
`AgentsAssemble-Appearance-Verify-20260828.fKzLaX`; unrelated processes and user data
were untouched. At that checkpoint, this threshold batch still awaited exact
public-diff review by the critical web session and Daybreaker Blue High.

## Authenticated appearance review corrections: 2026-08-29

The first Daybreaker review of `b690604..8ec147a` reported three Medium findings:
banner preset selection omitted the explicit empty-string clear, concurrent uploads
had no room-and-slot latest-generation owner, and frontend canonical-reference grammar
duplicated the Rust domain. Commits `4e69646`, `8feff01`, and `008f935` respectively
closed those findings. Daybreaker's next review found one remaining Medium: the SQLite
CHECK and HTTP raw-query boundary were not mechanically bound to that domain grammar.
Commit `93b036f` adds the schema invariant and makes the route consume the domain-owned
query contract.

The independent web review of the original batch reported two Medium findings. A local
directory becoming unconfirmed or unavailable changed the stable manager resolver from
success to failure without waking the object-URL owner, so an installed URL could remain
rendered. Commit `72582d9` passes the directory owner's existing currentness projection
into the hook; loss removes local desired assets, aborts reads, and revokes installed
URLs, while restoration reuses the exact resolver. The second finding was that the
frontend accepted `image/png` text without the private/no-store response contract or
bounded PNG bytes although the record claimed strict body validation. Commit `e747317`
moves the shared 10-MiB encoded-raster ceiling to the Rust domain owner, exports it to
the generated frontend wire constants, and makes the appearance HTTP owner require
exact JSON/PNG media types, semantically exact `private, no-store`, a nonempty bounded
body, and the PNG signature before object-URL creation. Focused tests reject missing or
wrong cache policy, invalid signature, oversized bytes, stale upload completion, owner
unmount, and directory-currentness loss.

These corrections add no durable state, cache, timer, task, SQL transition, fallback,
compatibility path, or generic asset framework. Authority cleanup reuses the existing
directory sync state. PNG acceptance already buffered the response blob; the new work is
one two-directive header parse, bounded-size comparison, and an 8-byte slice/signature
comparison before publication. Immediately before strict PNG acceptance the production
main and API chunks totaled 840.60 kB raw / 251.53 kB gzip; afterward they total 840.98
kB raw / 251.63 kB gzip, an observed +0.38 kB raw / +0.10 kB gzip. No CPU, memory, disk,
or latency improvement is claimed; the accepted cost closes a concrete private-response
and decoded-byte retention threat at the owning boundary.

Full serial `make verify` passed architecture, source-growth, policy, formatting,
generated bindings, original-CSS, production build, all workspace/TCP/integration/doc
tests, warning-denied Clippy, and the diff gate: frontend 87 files / 538 tests, desktop
20, domain 26, persistence 179, protocol 6, provider 120, and server 85. No real
provider, Deep Scan, or automated security scanner ran. Final web and Daybreaker
approval remains pending until this correction range is pushed and re-reviewed.

Computer Use then drove a fresh isolated release package named
`AgentsAssemble Appearance Correction Verify`, bundle identifier
`app.agentsassemble.rust.appearancecorrectionverify0829`, with its own new application
data and an explicitly empty central URL. Native file selection uploaded the
repository's `deepseek.png` as the room banner and `cursor.png` as the room icon after
the strict private/no-store, media-type, size, and PNG-signature checks landed. The
copied settings preview and the main room rail, room banner, and introduction rendered
both authenticated object URLs. This proves the tightened response path accepts the
real Rust server contract rather than only rejecting malformed fixtures.

Normal quit left no verification app or sidecar process. Only the isolated Application
Support, cache, WebKit, package-build directory, and generated sidecar binary were moved
to the recoverable Trash directory
`AgentsAssemble-Appearance-Correction-Verify-20260829.ojQ6vP`; unrelated applications,
processes, and user data were untouched. After the required critical-web response
completed, its tab and the browser/Computer Use connection were closed and reset.

The next critical-web re-review returned `REVISE — Critical 0 / High 0 / Medium 2`.
First, the installed SQLite CHECK admitted any 32-character suffix after `ra_`, while
the domain parser admitted only lowercase hex. Commit `5fcc51f` replaces the superficial
DDL-string assertion with an installed-schema behavior comparison against the domain
parser, adds the suffix CHECK, and advances the clean schema from 41 to 42 so the older
weaker schema is rejected without migration or compatibility code. Second, authority
loss was handled by a passive effect and the controller reconstructed only
`syncIssue === null`; a deferred read could therefore publish an object URL after the
loss commit. Commit `4794ca0` makes the directory owner expose currentness from the same
snapshot, epoch, active-host, sync, and bound-authority checks as its resolver. The
appearance owner hides local URLs in the false render, performs abort/removal/revocation
in layout effects, and checks currentness before object-URL creation. Controlled tests
prove immediate installed-URL revocation, no late URL from a deferred read, and a fresh
exact resolver/read after authority restoration.

The second correction adds no cache, durable frontend state, timer, task, fallback,
compatibility path, or generic authority framework. Schema 42 narrows one existing
durable CHECK; the lifecycle change reuses existing refs and state. The production main
chunk changed from 785.51 kB / 236.55 kB gzip after strict PNG acceptance to 785.89 kB /
236.62 kB gzip, an observed +0.38 kB raw / +0.07 kB gzip. No CPU, memory, disk, or
latency improvement is claimed; the accepted work closes a concrete durable-language
mismatch and stale decoded-byte publication window.

Full serial `make verify` passed every architecture/source-growth/policy/formatting/
generated-binding/original-CSS/build/workspace/TCP/integration/documentation/
warning-denied-Clippy/diff gate: frontend 87 files / 539 tests, desktop 20, domain 26,
persistence 179, protocol 6, provider 120, and server 85. Daybreaker approved the prior
range with `Critical 0 / High 0 / Medium 0`; final approval of these two new corrections
remains pending after push. No provider, Deep Scan, or automated security scanner ran.

The final critical-web re-review of `53033b6..9ab84ec` returned
`REVISE — Critical 0 / High 0 / Medium 1`. The lowercase-hex correction closed the
reported uppercase/nonhex cases, but SQLite rowid-table `TEXT PRIMARY KEY` still
accepted null without an explicit constraint, and TEXT length/pattern semantics were
not a byte-exact proof across embedded NUL. Commit `cc2aebd` advances the clean schema
from 42 to 43, adds explicit `NOT NULL`, requires the exact 35-byte BLOB length, and
uses a fixed 32-position lowercase-hex GLOB after the literal `ra_` prefix. The
installed-schema regression now compares an embedded-NUL candidate to the domain
predicate and separately proves that null is rejected. Schema 42 is rejected by the
existing version owner; no migration or compatibility path was added.

This correction changes one existing CHECK and adds no runtime allocation, query,
cache, task, state, fallback, or abstraction. Its purpose is durable-language equality,
not a claimed performance improvement. Focused behavior and all 179 persistence tests
passed. Full serial `make verify` again passed the architecture, source-growth, policy,
formatting, generated-binding, original-CSS, production-build, frontend 87/539,
desktop 20, domain 26, persistence 179, protocol 6, provider 120, server 85, every
TCP/integration/doc test, warning-denied Clippy, and diff gates. Final web and
Daybreaker approval of this last correction remains pending. No provider, Deep Scan,
automated security scanner, or Computer Use ran.

Daybreaker first re-reviewed the final schema correction as
`REVISE — Critical 0 / High 0 / Medium 1`: the embedded-NUL fixture used only 30
lowercase-hex bytes, so schema 42 also rejected it before the NUL and the regression did
not exercise the reported bypass. Commit `e7312d4` changes the fixture to the actual
37-byte predecessor bypass, `ra_` plus 32 lowercase-hex bytes plus NUL and trailing
`x`. Direct SQLite diagnosis confirmed schema 42 observed TEXT length 35 while the BLOB
length was 37 and admitted the value; schema 43 and the Rust domain predicate reject it.
The focused test and full serial `make verify` then passed again with the same suite
counts and all TCP/integration/Clippy/structure gates.

The final Daybreaker review returned `APPROVE — Critical 0 / High 0 / Medium 0`.
The final critical-web review independently returned
`APPROVE — Critical 0 / High 0 / Medium 0` with no findings. No provider, Deep Scan,
automated security scanner, or Computer Use ran. The sole critical-review browser tab
was closed after reading the completed response, the tab list was empty, and the browser
connection was reset.

## Lobby message-pin threshold batch: 2026-08-29

The incomplete threshold range `258c365..3b2c47f` establishes the lobby pin's durable
and HTTP authority without claiming frontend completion. Schema 44 owns one bounded
pointer from `(room_id, event_id)` to the exact `(room_id, event_seq)` in `room_events`;
it copies no message, author, attachment, or participant state. Event or room deletion
cascades only its owned pins. The persistence unit revalidates the current local manager
or exact human session, derives `room.history`/`message.modify` from current room
authority, validates one public non-deleted `message_final`, mutates the pointer, and
loads the returned canonical projection in one transaction. Read-only humans can list
but cannot mutate.

The concrete transport threats were cross-purpose/replayed credentials, a ticket bound
to another room, a revoked identity reaching body allocation, and a non-message pointer
becoming durable. Separate one-use pin-read and pin-write purposes now span the private
desktop control pipe and remote session exchange. The real TCP tests prove wrong-purpose
tickets are consumed, wrong-room tickets cannot be replayed, stale local identity is
rejected before an 8-KiB body reaches the 4-KiB decoder, and a non-message target returns
not-found with zero pins. No host token, raw session bearer at the pin route, WebSocket
pin command/event, compatibility path, migration, fallback, cache, timer, background
task, generic message repository, or client-owned authority was added.

The owning queries allocate only the returned projection. Listing joins pin pointers to
events through their composite primary key; mutation's event-ID lookup is currently a
room-key range scan with a JSON identity predicate. No new event index was added because
pin mutation is not yet shown to be latency-bound, while such an index would impose
disk and write amplification on every room event. This batch therefore claims no CPU,
memory, disk, or latency improvement. The next packaged frontend verification will
measure the actual flow before any performance structure is considered.

Full serial `make verify` passed architecture, source-growth, policy, both Cargo
formatters, generated bindings, original-CSS verification, production frontend build,
frontend 87/539, desktop 20, domain 26, persistence 184, protocol 6, provider 120,
server 85, every TCP/integration/doc test, warning-denied Clippy, and the final diff
gate. No real provider, Deep Scan, automated security scanner, or Computer Use ran.
The copied frontend still uses its old pin transport and the feature remains explicitly
incomplete pending that cutover and packaged verification. At that checkpoint, this
was the then-2,000-line threshold batch and awaited critical-web and Daybreaker review
before implementation continued.

The first threshold cross-review returned `REVISE`. Daybreaker reported one Medium:
unpin deleted before validating the canonical target, so missing and non-message targets
could succeed as no-ops. The critical web review reported that same issue and two related
Medium defects: a structurally decoded `message_final` with missing content was projected
as an empty normal message, and the complete pin list had no total bound, allowing one
authenticated room member to amplify later GET and POST responses without limit.

Commit `b72a6b0` moves the existing target validator before both mutation branches,
requires visible message content rather than defaulting corruption to empty text, and
adds persistence plus real-TCP regressions for missing/non-message unpin and corrupt
target rollback. One domain-owned absolute limit now permits 64 lobby pins per room. A
new pin checks the other-pointer count in the same mutation transaction; re-pin and
valid-message unpin remain available at capacity. Complete-list SQL reads at most 65
joined rows and rejects excess durable state before event deserialization, so a bad
database cannot restore the unbounded `fetch_all` allocation.

The bound addresses a concrete availability cost rather than claiming a speedup. At the
12,000-character message limit, even worst-case JSON escaping keeps 64 returned contents
to roughly 4.6 MiB before the small projection envelope, while 10,000 near-limit pins
could otherwise exceed 120 MiB before row JSON, allocation, and serialization overhead.
The accepted trade-off is one short indexed count on pin/re-pin; unpin pays no count and
all successful operations preserve the copied complete-list response. No index, cache,
pagination framework, configuration layer, task, timer, fallback, compatibility path,
or migration was added.

Focused persistence tests passed all four pin contracts and the real TCP suite passed
all three pin boundaries. Full serial `make verify` passed again: frontend 87/539,
desktop 20, domain 26, persistence 185, protocol 6, provider 120, server 85, every
TCP/integration/doc test, warning-denied Clippy, architecture/source-growth/original-CSS,
and diff gates. No real provider, Deep Scan, automated scanner, or Computer Use ran.
The final correction reviews found no further Critical, High, or Medium issue. The
critical web session and Daybreaker each approved exact correction
`3b2c47f..d940313` and cumulative `258c365..d940313` with
`Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or another automated
security scanner.

## Lobby message-pin frontend threshold batch: 2026-08-29

The local range `d940313..4a7d8d8` replaces the copied pin client's unauthenticated
local request and raw remote-session bearer with the backend's final authority. The
domain-owned 64-pin bound is exported into one generated TypeScript constant. Every
packaged local list or mutation requests its distinct room-bound native ticket, while
every remote operation first exchanges the live session for the matching one-use read
or write ticket. Only that ticket reaches `/api/room-pins`; an absent exact authority
exposes no lobby mutation. The old generic moderator, host-token, raw-session, and
arbitrary-channel pin paths were removed rather than retained as fallbacks.

The response parser now requires the exact lobby projection, safe positive sequence,
valid timestamps, unique bounded event identities, the currently empty attachment
projection, and a mutation list consistent with the requested pin state. Missing,
extra, malformed, contradictory, duplicate, or over-limit state fails instead of
becoming an empty list. One Lobby-owned operation identity permits at most one active
pin request and discards a delayed response after the room or authority changes. This
avoids overlapping one-use grants and prevents a previous room's list from becoming
the next room's UI state without adding a cache, queue, timer, retry, or second authority.
Custom-channel pin state and per-message controls were removed; its copied header now
states that the still-unimplemented surface is unavailable.

The production main chunk changed from the preceding 785.89 kB / 236.62 kB gzip to
788.89 kB / 237.59 kB gzip, an observed +3.00 kB raw / +0.97 kB gzip. The shared API
chunk changed from 55.47 kB / 15.08 kB gzip to 55.85 kB / 15.14 kB gzip, while removing
the premature custom-channel client reduced that view from 9.82 kB / 3.48 kB gzip to
8.78 kB / 3.22 kB gzip. No runtime CPU, memory, latency, or disk improvement is claimed
before packaged verification; the only runtime bound added in this frontend is the
single active request per mounted lobby.

Full serial `make verify` passed architecture, source-growth, policy, formatting,
generated bindings, original-CSS verification, the production build, frontend 89/551,
desktop 20, domain 26, persistence 185, protocol 6, provider 120, server 85, every
TCP/integration/doc test, warning-denied Clippy, and the final diff gate. No provider,
Computer Use, Deep Scan, or automated security scanner ran. Packaged local and remote
human verification, restart retention, and manual cross-review remain pending; this
record does not claim the frontend slice complete.

The first frontend threshold cross-review returned `REVISE`. Daybreaker found two
Medium issues: pin selection reused the not-yet-implemented search-context request and
could send a raw remote session to it, and the 128-byte/NUL-free event-ID policy was
independently hard-coded in the frontend, persistence validator, and DDL. The critical
web review independently found two Medium issues: a grant delayed across room/session
replacement could still dispatch its retired operation, and the strict response parser
accepted invisible message content or two distinct IDs with one canonical sequence.

Four independent correction commits close those boundaries without enabling search or
adding another history transport. Pin navigation now scrolls only to an exact event in
the already canonical visible history; an older unloaded pin reports that it is not in
the current history instead of calling the unavailable context route. The domain owns
the pin event-ID byte bound and predicate, the protocol generates the frontend bound,
persistence consumes the predicate, and an installed-SQLite behavior matrix binds the
remaining DDL constraint to ASCII, multibyte UTF-8, empty, over-bound, NUL, and null
cases. The pin parser mirrors the domain visible-text categories and rejects duplicate
canonical sequences.

One view-owned operation record now survives a room/session change in retired state.
Both local native grant and remote session exchange call its currentness assertion after
grant acquisition and immediately before the target request; a retired operation cannot
publish UI state or issue the target request, and no replacement pin operation starts
until it settles. A request already dispatched before authority replacement retains the
server-owned outcome contract. Deferred local and remote tests prove zero post-grant
target dispatch, and the view test proves a replacement room cannot overlap or publish
the retired result.

These corrections add no durable state, transport, queue, timer, task, cache,
compatibility path, migration, fallback, or generic lifecycle framework. Moving the two
pin constants and predicate into their domain owner removes policy drift; the remaining
runtime cost is one short currentness call before each request and bounded response
checks over at most 64 pins. The production main chunk is 789.35 kB raw / 237.85 kB
gzip, an observed +0.46 kB raw / +0.26 kB gzip from the pre-correction 788.89 kB /
237.59 kB gzip; the API chunk remains 55.85 kB / 15.14 kB gzip. No CPU, memory, disk,
or latency improvement is claimed.

Full serial `make verify` passed the corrected code: frontend 89 files / 554 tests,
desktop 20, domain 27, persistence 186, protocol 6, provider 120, server 85, every
TCP/integration/doc test, warning-denied Clippy, and all architecture, source-growth,
policy, generated-binding, original-CSS, production-build, and diff gates. No provider,
Computer Use, Deep Scan, or automated scanner ran. Final correction cross-review and
packaged local/remote/restart verification remain pending.

The correction re-review remained `REVISE`. Both reviewers found that retiring the
view-owned operation from a passive React effect left a commit-to-effect interval in
which a completed grant could still dispatch its target request under replaced room or
session authority. Daybreaker also found that JavaScript accepted isolated UTF-16
surrogates: `TextEncoder` would replace them while Rust strings require Unicode scalar
values, so the two validation boundaries did not describe the same identity or visible
text.

Two narrow commits close those remaining boundaries. The existing operation owner now
uses the layout-effect cleanup phase, so authority replacement retires the operation
before passive request continuations can run; no state, transport, timer, task, queue,
cache, or lifecycle abstraction was added. The pin parser and input validator reject an
isolated high or low surrogate while preserving valid non-BMP scalar pairs. Focused
tests cover delayed local and remote grants, isolated surrogates in identity and visible
fields, pre-grant input rejection, and valid emoji. The production main chunk measured
789.42 kB raw / 237.89 kB gzip, an observed +0.07 kB raw / +0.04 kB gzip from the prior
789.35 kB / 237.85 kB gzip; the API chunk remains 55.85 kB / 15.14 kB gzip. No CPU,
memory, disk, or latency improvement is claimed.

Full serial `make verify` passed again: frontend 89 files / 555 tests, desktop 20,
domain 27, persistence 186, protocol 6, provider 120, server 85, every
TCP/integration/doc test, warning-denied Clippy, and all architecture, source-growth,
policy, generated-binding, original-CSS, production-build, and diff gates. A Computer
Use launch was attempted only against the isolated verification package, but the Mac
was locked and no app was launched; packaged local/remote/restart verification therefore
remains pending. No provider, Deep Scan, or automated scanner ran. Final cross-review
of these two corrections also remains pending.

The next manual cross-review returned the same single Medium finding from both
reviewers: `pinned_at` and `created_at` still reached permissive JavaScript date parsing
without the Unicode-scalar fence, and the scalar predicate duplicated the equivalent
room-ID check. The authority lifecycle correction was approved. One independent
correction now gives valid-Unicode-scalar text a seven-line frontend owner, reuses it for
canonical room IDs and every pin string, and checks both timestamps before `Date.parse`.
Malformed-response regressions cover isolated high and low surrogates in the two
timestamp fields; the existing non-BMP regression continues to prove valid scalar pairs.

The correction removes the duplicate validator and adds no state, authority, transport,
retry, compatibility path, or generic string framework. A `for...of` scan exits at the
first invalid scalar and avoids allocating a scalar array. The observed production main
chunk is 789.39 kB raw / 237.85 kB gzip, versus 789.42 kB / 237.89 kB before the shared
owner; the API chunk is 55.90 kB / 15.17 kB gzip, versus 55.85 kB / 15.14 kB. No runtime
CPU, memory, disk, or latency improvement is claimed from these build-size movements.

Focused pin and room-ID tests passed 12/12, and full serial `make verify` passed with
frontend 89 files / 555 tests, desktop 20, domain 27, persistence 186, protocol 6,
provider 120, server 85, every TCP/integration/doc test, warning-denied Clippy, and all
architecture, source-growth, policy, generated-binding, original-CSS, production-build,
and diff gates. Computer Use remained blocked by the locked Mac; no app was launched or
left running, and its kernel was reset. Packaged local/remote/restart verification and
final correction approval remain pending. No provider, Deep Scan, or automated scanner
ran.

Final manual correction review found no actionable issue. Daybreaker and the critical
web session each approved both `c6c6f41..0a1bec6` and cumulative
`d940313..0a1bec6` as `APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used
Deep Scan or another automated security scanner.

The release package was rebuilt from `0a1bec6`, but two Computer Use launch attempts
stopped at the locked-Mac boundary before the app opened. No exact app, sidecar, or
server process remained, no application-support/cache/WebKit data was created, and the
Computer Use kernel was reset after each attempt. The isolated 1.2 GiB build directory,
generated sidecar, and generated frontend `dist` were moved to exact recoverable Trash
paths. Packaged local/remote/restart verification remains the sole uncompleted exit
evidence for this slice and must use a freshly rebuilt isolated package after the Mac is
unlocked.

## Lobby message-pin packaged completion: 2026-08-29

The isolated release package built from `0a1bec6` was restored from its recoverable
Trash location after the Mac was unlocked. Later commits through `4959f9b` contain only
review corrections already present in that binary or documentation, so the package
exercised the final lobby-pin code. Its distinct bundle identifier, SQLite state,
Application Support, caches, WebKit data, sidecar, frontend distribution, and build
directory remained separate from every installed or development copy.

Computer Use created one real room and published `LOBBY_PIN_E2E_20260829` through the
packaged copied frontend. The local operator exposed the message action, pinned it,
opened the complete pin list, selected its list row, and observed the exact timeline
message receive focus/highlight. Unpin emptied the list and re-pin restored one row.
After a normal `Cmd-Q` and exact bundle relaunch, the same room, message, and pin were
loaded from canonical SQLite state. This passes local list/mutation/navigation and exact
restart retention without fixture, fake authority, compatibility state, or fallback.

The same app opened its owned Cloudflare quick tunnel and created one-use human invites.
A fresh Chrome incognito browser admitted `Writable Pin Guest`, removed the invite token
from the address bar, loaded the canonical message and existing pin, unpinned it through
the remote HTTP authority, observed the empty list, and re-pinned it from the real
message row. Reopening the host list showed the same restored pin. The host then changed
the room-owned invite scope to read-only and generated a new one-use invite; the prior
invite was visibly retired rather than reused. After the writable incognito window was
closed, a new incognito identity admitted `Read Only Pin Guest`. It could read the exact
pin and message, while the UI stated that the room was view-only, disabled the composer,
omitted the list unpin action, and exposed no per-message pin toggle. The earlier real
TCP contract test supplies the complementary direct-mutation evidence: the same
read-only authority receives stable `session_read_only` denial with no durable change.

The host's copied custom-channel dialog was also exercised with a non-empty channel
name. It remained open and reported `Custom channels are unavailable until their
message and voice owners exist.` No custom channel or custom-channel pin authority was
created. This is the intended visible incomplete state, not a client-side substitute.

Cleanup followed the owner boundaries used for the run. The app reported public ingress
off before either browser or host shutdown. Exactly one verification incognito window
was open at a time and each was closed without closing or inspecting the user's normal
Chrome window. The exact packaged app exited normally; no matching desktop/server or
`cloudflared` process remained. Only the verification build directory and the bundle-ID
specific Application Support, cache, and WebKit directories were moved to distinct
recoverable Trash paths, alongside the already isolated sidecar and frontend build. The
Computer Use kernel was reset. No provider, Deep Scan, or automated security scanner
ran, and this UI-only closure makes no CPU, memory, disk, or latency improvement claim.

## Retained-asset accounting correction: 2026-08-29

Commit `1552b14` corrects the absolute local-storage owner before message attachments
join it. The prior query omitted expired pending rows even though their BLOBs still
occupied SQLite. An expired row in one lifecycle could therefore stop contributing to
the 4,096-item / 8-GiB ceiling before that lifecycle owner deleted it, allowing another
lifecycle to admit additional physical storage. The correction moves only the shared
checked replacement arithmetic and three-table retained-row aggregate from the raster
decoder into `asset_storage`; profile, pre-join, and room-appearance modules retain
their own authority, expiry cleanup, exact replacement, and state transitions.

Accounting now includes every physically retained row and still computes exact
replacement as `current - predecessor + new`. Limit failure preserves the existing
error code and message, deletes no row, and leaves each lifecycle able to collect only
its own expired pending rows. A cross-owner regression seeds 4,096 expired pre-join
rows, proves a profile upload is rejected, and proves all pre-join rows remain for their
owner to collect. The existing boundary regression continues to prove replacement at
the exact ceiling succeeds while net growth fails.

The aggregate reads only `size` from the three current asset tables. A healthy store is
bounded to 4,096 retained rows, and the focused 4,096-row regression completed in 0.03
seconds on the development machine. Removing the expiry predicates also removes three
timestamp comparisons and binds, but no CPU, memory, latency, or disk-performance
improvement is claimed. A cache, counter table, background sweeper, generic repository,
or cross-owner garbage collector would add split authority without observed need, so
none was introduced.

Full serial `make verify` passed architecture and 800-line source gates, policy tests,
formatting, generated bindings, original-CSS verification, the production frontend
build, frontend 89 files / 555 tests, desktop 20, domain 27, persistence 187, protocol
6, provider 120, server 85, every TCP/integration/doc test, warning-denied Clippy, and
the final diff gate. Repository-wide searches found one owner for the 4,096/8-GiB asset
policy and one call path from each implemented lifecycle. No provider, Computer Use,
Deep Scan, or automated security scanner ran for this persistence-only correction.

## Message-attachment pending storage: 2026-08-29

The message-attachment persistence owner now accepts one real 1-byte-to-10-MiB file
only for the current writable, joined, unmuted human room principal. Local authority
loads the current room participant inside the insertion transaction. Remote authority
first revalidates the exact persistence-issued human session and then applies the same
domain-owned message-write policy. Read-only, Agent Bridge, stale, left, and muted
authority therefore cannot create pending bytes. The opaque `ma_` identifier is created
inside that transaction and is bound to the exact room and uploader until the later
message transaction promotes it.

The original reachable filename and MIME normalization were retained at the message
owner: path components and control characters are removed, names are bounded to 120
characters, invalid MIME declarations use maintained `mime_guess` data, and unknown
types become `application/octet-stream`. Normalized MIME metadata is additionally
bounded to 127 bytes before it can later enter an event projection. Arbitrary files
retain their exact bytes and remain download-only. Only declared PNG, JPEG, GIF, or
WebP whose detected format agrees is marked inline-safe; the existing two-permit raster
decoder, 4,096-dimension, 16-Mpixel, and 72-MiB decode-allocation bounds are reused.
Message images are decoded for safety but are not re-encoded, because byte preservation
is a product contract rather than an optimization opportunity.

The concrete threats were active-content preview, declared/detected type confusion,
decode resource exhaustion, stale upload authority, and one lifecycle deleting another
lifecycle's retained bytes. The implementation adds no attachment framework, generic
repository, cache, counter table, background sweeper, fallback, compatibility path, or
operating-quota configuration. The message owner deletes only its own expired pending
rows on its upload write path, then calls the shared physically-retained accounting
owner. Expired profile, pre-join, and room-appearance rows remain untouched. After the
message table joined accounting, the cross-owner ceiling regression uses 2,048 expired
pre-join rows plus 2,048 expired message rows; omitting either table would admit the
rejected net-new profile upload. Shared accounting diagnostics were renamed from
`raster` to `attachment` without changing their stable error codes.

Focused tests prove exact byte preservation, safe-image classification, declared-type
mismatch rejection, filename/MIME normalization, read-only and mute denial with no
partial row, exact human-session revalidation, and message-only expiry cleanup. On a
warm development build, the combined real SQLite/safe-raster focused test completed in
0.13 seconds with a 100,728,832-byte maximum test-process resident set; this includes
the Rust test harness and is not claimed as per-upload cost. No CPU, memory, latency, or
disk improvement is claimed, and no cache or alternate encoding path was justified.

Full serial `make verify` passed architecture and 800-line source gates, policy tests,
formatting, generated bindings, original-CSS verification, the production frontend
build, frontend 89 files / 555 tests, desktop 20, domain 28, persistence 194, protocol
6, provider 120, server 85, every TCP/integration/doc test, warning-denied Clippy, and
the final diff gate. This completes the unexposed pending-storage boundary only; atomic
message binding, HTTP grants and reads, Agent Session reads, copied-frontend connection,
packaged Computer Use, and real providers remain pending. No provider, Computer Use,
Deep Scan, or automated security scanner ran for this persistence-only boundary.

## Message-attachment atomic binding: 2026-08-29

Commit `292cb06` makes the existing `message.send` transaction the sole binding owner.
The command accepts visible text, one to eight distinct canonical pending IDs, or an
attachment-only message. After idempotency admission and current participant loading,
the transaction verifies every ID against the exact room, principal, unbound pending
state, and one-hour expiry. It then places only bounded public metadata in the canonical
`message_final` event, inserts that event, promotes every row to the event sequence,
routes ordered/ambient work, records the command result, and commits once. Replay returns
the stored event without rebinding. A missing, foreign, expired, or already-bound row,
payload conflict, or later routing failure leaves the event, attachment custody, turn
queue, and command result unchanged. Failed expiry checks deliberately do not perform
cleanup; only the upload owner's bounded write path commits that lifecycle operation.

The concrete threat was split durable authority: accepting client metadata, binding
outside event insertion, or committing attachment state before turn routing could leave
bytes referenced by no canonical message or a message referring to bytes still owned by
the composer. The implementation adds no binding table, repository trait, fallback,
compatibility state, cleanup transaction, or client orchestration. The existing event
sequence is the foreign-key owner, and the existing command transaction is the smallest
boundary that preserves replay and routing semantics.

At most eight indexed point reads and eight exact conditional updates occur in the one
SQLite write transaction. This fixed bound was retained instead of adding dynamic SQL,
a cache, or a bulk-binding abstraction without measured need. Event and provider queues
carry metadata and opaque IDs only, never attachment BLOBs. The focused real-SQLite
binding tests completed in 0.06 seconds on a warm development build; this is a harness
measurement, not a per-message latency claim. No CPU, memory, disk, or latency
improvement is claimed.

The serial verification run passed the production frontend build and original-CSS
check, frontend 89 files / 555 tests, desktop 20, domain 29, persistence 197, protocol
6, provider 120, server 85, and every TCP/integration/doc test. Its final Clippy step
first rejected a 104-line test function; the assertions were split at their bound-row
verification responsibility rather than suppressing the gate. The focused tests,
warning-denied workspace Clippy, architecture gate, 800-line source gate, and diff gate
then passed. No provider, Computer Use, Deep Scan, or automated security scanner ran for
this persistence-only boundary. Pin projection, HTTP grants/reads, Agent Session reads,
copied-frontend connection, packaged verification, and real providers remain pending.

## Retained/message-storage threshold cross-review: 2026-08-29

- Web and Daybreaker each found one Medium in `6bd791b..7ddf858`: the installed
  `room_message_attachments.attachment_id` CHECK accepted an exact-length embedded-NUL
  value that the domain-owned lowercase-hex grammar rejected. All other reviewed
  accounting, schema, pending-storage, authority, structure, duplication,
  overimplementation, and lifecycle criteria were approved.
- Commit `5e324b8` replaced the terminating-text predicate with an exact fixed-position
  32-lowercase-hex grammar, added the 35-byte NUL regression, and advanced the clean
  schema version without compatibility or migration code.
- Web final verdict: `7ddf858..5e324b8` APPROVE C0/H0/M0;
  `6bd791b..5e324b8` APPROVE C0/H0/M0.
- Daybreaker final verdict: `7ddf858..5e324b8` APPROVE C0/H0/M0;
  `6bd791b..5e324b8` APPROVE C0/H0/M0.

## Attachment-aware message-pin projection: 2026-08-29

Commit `185edeb` keeps `room_message_pins` as an event pointer and derives attachment
filenames only while projecting the canonical `message_final`. A target is now valid
when it has visible text or at least one attachment. Attachment-only pins therefore
retain empty message content and the event-defined filename order without copying IDs,
metadata, bytes, or lifecycle authority into pin storage.

The concrete integrity threat was treating arbitrary flattened event JSON as trusted
pin metadata. The message-attachment owner now strictly decodes at most eight entries
and rechecks the canonical ID, distinctness, sanitized filename, MIME grammar, item
size, safe-raster classification, and exact view/download paths. Pin reads and writes
fail closed on corrupt metadata. The safe-raster MIME decision reuses the raster owner;
no second MIME allowlist, attachment repository, cache, or pin-owned state was added.

Decoding is bounded by the existing eight-item event ceiling and does not read BLOBs.
The focused real-SQLite pin suite, including attachment-only projection and corrupt-URL
rejection, completed in 0.13 seconds on a warm development build; no CPU, memory, disk,
or latency improvement is claimed. Full serial `make verify` passed architecture and
800-line gates, policy tests, formatting, generated bindings, original-CSS verification,
the production frontend build, frontend 89 files / 555 tests, desktop 20, domain 29,
persistence 198, protocol 6, provider 120, server 85, every TCP/integration/doc test,
warning-denied Clippy, and the final diff gate. No provider, Computer Use, Deep Scan, or
automated security scanner ran for this persistence projection.

## Canonical bound message-attachment reads: 2026-08-29

Commit `66fd6c2` adds the persistence read boundary for an attachment only after its
canonical `message_final` owns it. Local reads resolve the exact current
room/user/participant identity in the read transaction. Remote human reads revalidate
the exact durable session and require its current `room_history` capability; read-only
or muted membership may still read history because message-write authority is a
separate contract. Pending, expired, foreign-room, unreferenced, deleted-message, stale
membership, and malformed-ID requests fail closed without returning bytes.

The concrete disclosure threat was reading a same-room pending or orphaned BLOB merely
because its opaque identifier was known. The read owner first selects bounded metadata,
stored byte length, and the joined canonical event, then verifies the event type,
sequence, room, deletion state, and exact strict metadata reference. Only after those
checks does a second indexed query materialize the BLOB. This avoids copying a
potentially 10-MiB value before current authority and durable reachability are proven.
No cache, attachment repository, fallback, compatibility path, background state, or
second lifecycle owner was introduced.

The fixed event ceiling keeps metadata validation at no more than eight entries. The
focused real-SQLite binding/read suite completed in 0.06 seconds on a warm development
build; that is a harness observation rather than a production latency claim. Full
serial `make verify` passed architecture and 800-line source gates, policy tests,
formatting, generated bindings, original-CSS verification, the production frontend
build, frontend 89 files / 555 tests, desktop 20, domain 29, persistence 199, protocol
6, provider 120, server 85, every TCP/integration/doc test, warning-denied Clippy, and
the final diff gate. No CPU, memory, disk, or latency improvement beyond avoiding the
premature BLOB copy is claimed. HTTP grants/routes, Agent Session reads,
copied-frontend connection, packaged verification, and real providers remain pending.

## Message-attachment transfer bridge threshold review: 2026-08-29

- The critical web reviewer approved the initial pushed bridge range
  `bbf5b0f..c74e268` with C0/H0/M0. Daybreaker found one Medium omitted normal-error
  classification: `muted` and `permission_denied` upload-ticket rejections became
  `TicketFailure::Broken`, so the desktop could stop a healthy owned runtime for a
  valid application denial.
- Commit `46e3a4e` aligned the desktop classifier with the server-owned normal control
  codes, including `room_authority_changed`, and added an owned-process preservation
  regression. Daybreaker approved that correction and cumulative range with C0/H0/M0.
- The web re-review found one Medium in the first regression: it used a fixed sleep and
  compared only the retained PID handle, so it did not deterministically prove that the
  child remained alive and conflicted with the repository's barrier/event test rule.
- Commit `9c3bbb5` replaced the sleep with an exact child-ready/stdin-release barrier,
  checks the same PID and `try_wait() == None` after every normal denial, and joins the
  released child successfully. No production state or abstraction was added.
- Final web verdict: `46e3a4e..9c3bbb5` and `bbf5b0f..9c3bbb5`
  `APPROVE — Critical 0 / High 0 / Medium 0`. Final Daybreaker verdict for both ranges:
  `APPROVE — Critical 0 / High 0 / Medium 0`. Neither review used Deep Scan or another
  automated security scanner.

## Message-attachment copied-frontend threshold review: 2026-08-29

- The first web and Daybreaker reviews found Medium lifecycle and resource-boundary
  gaps: passive-effect cleanup left commit windows for replaced upload/read authority
  and object URLs; caller cancellation could release a read slot before an
  abort-ignoring transport actually settled; deferred dispatch and the local ticket
  boundary needed current abort checks.
- Commits `8f3f504`, `fd176e6`, and `2c19b1c` moved authority retirement to layout
  cleanup, separated caller settlement from actual transport-slot release, rechecked
  deferred and local grant boundaries, and added deterministic cancellation and URL
  lifetime regressions.
- The next reviews found two Medium ownership gaps: per-authority schedulers allowed
  unresolved transports to accumulate across generations, while retiring a memoized
  scheduler broke React StrictMode effect reconnection. Commit `dcdb550` introduced one
  non-global shared capacity owner with task-local exact room/authority capture.
- Both reviewers then found one Medium at the real `LobbyView` unmount/re-entry edge:
  the owner still ended with that view. Commit `de0a8d8` moved the owner to `AppView`
  lifetime and added an actual child unmount/remount barrier proving replacement work
  waits for an old abort-ignoring transport to settle.
- Final web verdict: `de0a8d8`, `dcdb550..de0a8d8`, and `791ecd0..de0a8d8`
  APPROVE C0/H0/M0. Final Daybreaker Blue High verdict for the same three ranges:
  APPROVE C0/H0/M0. Neither review used Deep Scan or another automated security scan.

## Message-attachment provider threshold review: 2026-08-29

- The critical web reviewer initially approved `2a04152..24fe2f1` with C0/H0/M0.
  Daybreaker found one Medium: connection concurrency did not bound repeated reads of
  the same accepted ID, allowing cumulative SQLite BLOB loads, base64 allocations,
  Antigravity file syncs, and inactivity-deadline refreshes within one turn.
- Commit `7a3f0f4` added the active-turn-owned per-ID pending and attempt ledger, the
  successful-byte ceiling, terminal/finish exclusion, abort tombstone, and deterministic
  retry/cancellation regressions. The next web and Daybreaker reviews each found one
  Medium: `RoomToolReservation::resolve` still ignored a pending attachment read and
  could remove the closing generation after the room tool resolved first.
- Commit `4ab614e` reused `ActiveObservation::has_pending_operations()` at that final
  release path and added the mixed room-tool/attachment abort-order regression. Final
  web verdict: `4ab614e` and `2a04152..4ab614e` APPROVE C0/H0/M0. Final Daybreaker Blue
  High verdict: `4ab614e` APPROVE C0/H0/M0, with no Low finding.
- The web review recorded executable-staging orphan cleanup as a nonblocking, separate
  lifecycle-owner finding. Neither final review used Deep Scan or another automated
  security scan.

## Executable-staging crash lifecycle: 2026-08-29

The prior concrete cost was a reproducible 64-MiB provider executable orphan after one
forced guardian-death test and 159 historical macOS staging directories exceeding 10
GiB, eventually producing `ENOSPC`. Commits `af6297d`, `11fa808`, and `afd3997` put
that cleanup at its process-custody owners. Filesystem-staged provider images and Unix
private companions share one provider lease root; Linux and Android provider images
retain their sealed `memfd` path. Desktop-image re-exec and server-sidecar copies share
a separate desktop lease root. Both filesystem owners retain active locked directories
and reclaim only unlocked crash directories on the next create or owner drop. Root and
child locks are private, owner-validated, opened without symlink following, and
serialized for create and reclaim. Scans stop at 1,024 entries and unsafe or unknown
state fails closed.

This preserves the opened-source identity, staged-byte hash, `0500` executable,
`0700` directory, active process custody, and existing launch proof. It adds no timer,
background sweeper, configuration layer, fallback, legacy migration, or generic
workspace crate. The provider and desktop implementations remain separate because
their process lifetimes and architecture owners are separate; maintained `fs2` and
`tempfile` provide the shared solved mechanisms. An initial nonblocking create lock
failed under normal parallel Codex staging, so create now waits for the root lock.
Owner drop acquires that lock nonblocking, but after either acquisition the owner runs a
bounded synchronous scan and may recursively delete up to 1,024 unlocked directories.
Worst-case cleanup latency was not measured; no latency improvement is claimed.

The active-versus-abandoned regressions, running-image and source-replacement binding
tests, forced guardian-death test, and full `make verify` passed: frontend 92 files /
588 tests, persistence 200, provider 128, server 86, desktop 22, every TCP/integration
and doc test, warning-denied Clippy, and architecture/source/diff gates. No old macOS
provider, companion, or server executable-staging directory remained; each managed
root contained only its zero-byte root lock.

- The initial web and Daybreaker reviews each found one Medium: Unix companion staging
  remained outside the provider lifecycle owner. Commit `11fa808` moved that reachable
  path under the same provider lease owner without changing byte or Room Portal
  authority.
- Daybreaker found one additional Medium: both macOS `BoundSidecar` entry points still
  used desktop-owned raw temporary directories. Commit `afd3997` gave those paths one
  desktop lifecycle owner. The web reviewer independently confirmed that the separate
  provider and desktop roots are required custody boundaries, not approval-blocking
  policy duplication.
- Final web verdict: `11fa808`, `afd3997`, `af6297d..afd3997`, and
  `47c75d1..afd3997` APPROVE C0/H0/M0. Final Daybreaker Blue High verdict:
  `11fa808..afd3997` and `47c75d1..afd3997` APPROVE C0/H0/M0. Neither review used
  Deep Scan or another automated security scanner.

## Windows attachment-save creation security: 2026-08-29

- The critical web review found one Medium in `5b7f8e4..1fe8ff2`, and Daybreaker Blue
  High independently confirmed it: Windows share mode zero blocks later read, write, and
  delete categories but not `WRITE_DAC`. Because the post-create hardening verified only
  the DACL, a principal able to replace the staging name with a directory it owned could
  retain the owner's implicit `WRITE_DAC` and relax that DACL during payload creation.
- Commit `77624ca` moved the owner-only protected inheritable DACL to directory creation,
  verifies the named parent against the already retained parent handle, reopens the random
  staging name relative to that handle without following reparse points, and validates both
  current owner and exact inheritable DACL through the retained staging handle before any
  payload exists. The Windows filesystem policy remains in `private_fs`; the save module
  owns only path/handle identity and relative access. No fallback, compatibility path,
  timer, background cleanup, or reusable security framework was added.
- The concrete extra cost is two Windows-only parent identity handles, one security
  descriptor construction, and one owner query per explicit save. A hostile parent-path
  replacement can leave one empty private directory outside the retained parent because
  unvalidated objects are not deleted. Windows paths not representable as Rust UTF-8 are
  rejected because the safe creation wrapper accepts `str`; the code does not trade the
  fixed race for a lossy path conversion. No CPU, memory, disk, or latency improvement is
  claimed.
- Daybreaker Blue High found one Medium in `77624ca..60d8a97`: the dependency's blanket
  handle extension queried `GetSecurityInfo` with `SE_UNKNOWN_OBJECT_TYPE`, which Windows
  rejects for a file or directory handle. That made every native Windows save fail after
  creating its staging directory and made the earlier successful-validation claim
  inaccurate. Commit `d8f4aa3` removed that extension path and calls the dependency's
  explicit safe wrapper with `SE_FILE_OBJECT`; the retained-handle owner and DACL
  validation contract is otherwise unchanged.
- The critical web review found a second Medium in `77624ca..60d8a97`: cap-std maps a
  write-only Windows payload open to `GENERIC_WRITE`, while the following handle-based
  DACL update requires `WRITE_DAC`. Commit `0369d6d` gives that one create-new payload
  handle exactly `GENERIC_WRITE | WRITE_DAC`; the existing private-file policy still owns
  the DACL update and validation, with no new abstraction or dependency.
- After `0369d6d`, warning-denied Windows all-target/all-feature cross-compilation and full
  `make verify` passed architecture and 800-line gates, copied-frontend/original-CSS
  verification, frontend 93 files / 591 tests, desktop 26 tests, all Rust/TCP/integration
  and doc tests, Clippy, and the diff gate. Windows-only private-creation and exclusive
  handle tests compile but could not run on the available macOS host; packaged Windows and
  cross-principal runtime verification remain pending. No Deep Scan or automated security
  scanner ran.
- Final critical web verdict: `d8f4aa3`, `1175969`, `0369d6d`, `fb03430`, and
  `77624ca..fb03430` APPROVE C0/H0/M0. After receiving the web finding explicitly,
  Daybreaker Blue High re-reviewed `1175969..fb03430` and `77624ca..fb03430` and also
  returned APPROVE C0/H0/M0.

## Persona library schema and atomic repository candidate: 2026-08-30

Commits `33cd4f6` and `fc47a37` give normalized persona assets one SQLite custody owner.
The fresh-only schema has exactly the canonical ID, private card JSON, and optional thumbnail
PNG; it retains no raw import, filesystem path, migration state, quota ledger, or duplicate safe
summary. One upsert replaces the card and thumbnail atomically. The real-SQLite regression proves
that an invalid same-ID replacement preserves the prior card, a valid thumbnail-free replacement
removes the prior thumbnail, the original summary ordering is retained, and the replacement
survives close/reopen with no extra row.

The list query does not materialize thumbnail BLOBs and computes each casefolded sort key once.
The focused warm test body completed in 0.04 seconds (`real 0.26` seconds including Cargo process
startup). No broader CPU, memory, disk, or production-latency improvement is claimed. Full
`make verify` passed the architecture and 800-line gates, workspace and desktop checks, copied-CSS
verification, frontend 94 files / 593 tests, persistence 210 tests, every Rust/TCP/integration/doc
test, warning-denied Clippy, and the diff gate. The build initially stopped with `ENOSPC`; removing
only this repository's regenerable Cargo outputs recovered about 40 GiB, after which the unchanged
verification completed. User-owned files and `.agents/` were untouched. No provider, Computer Use,
Deep Scan, or automated security scanner ran. At that checkpoint, these commits remained local
under the then-active three-feature-or-2,000-line push rule, so manual cross-review was not yet
claimed.

## Persona local-operator HTTP candidate: 2026-08-30

Commits `da407d2` and `54ed384` connect the normalized library to private
`/api/personas`, `/api/personas/import`, and `/api/personas/{persona_id}/thumbnail`
routes. The server consumes an exact one-use local-operator ticket before any body, rejects and
consumes crossed-purpose authority, applies the shared bounded base64 JSON calculation, and exposes
only safe summaries or canonical PNG bytes. Import parsing and decompression run outside the async
executor. A process-wide two-import admission is owned through decode, normalization, and the atomic
store, including after caller cancellation; no fallback, raw-source copy, migration, or cleanup task
was added.

The real loopback TCP tests import an actual CCv3 PNG, list the exact safe projection, read the
thumbnail with private/no-store, CORS, disposition, and `nosniff` headers, reject missing thumbnails,
prove ticket replay failure, prove crossed-ticket consumption before malformed body handling, and
leave rejected imports absent. The two tests completed in 0.07 seconds during full `make verify`.
That full run also passed architecture and 800-line gates, copied-CSS verification, frontend 94 files
/ 593 tests, persistence 210, server 86 plus every TCP/integration test, desktop 26, provider 134,
warning-denied Clippy, and the diff gate. No provider, Computer Use, Deep Scan, or automated security
scanner ran. At that checkpoint, the candidate remained local under the then-active configured push
threshold; reviewer approval was not claimed.

## Persona packaged completion candidate: 2026-08-30

Computer Use drove a release bundle built from the current Rust sidecar and copied frontend under
the isolated identifier `app.agentsassemble.rust.personaverify0830`, with central hosting disabled
and a fresh SQLite authority. The copied picker imported representative CCv3 JSON, CCv3 PNG,
CHARX, and standalone Risu module fixtures. Their SHA-256 digests were, respectively,
`2deaeedcce7371022f9ed41ed6254aeed4284595a6c6377071f623e1f8eb23e7`,
`c8ddd2fa7fb0d1edd27781e7e59b2f000d71767b9a70099c10f2d23bc801d394`,
`b0d5aa070215b3c633f205222eac7cc5e3d0b1e0a1877df3cfd064859017c042`, and
`1c16558eb011845edb1279e84d54ef978d1e74dd24ef1b76f84231f81b4b4202`.
The library showed all four exact safe summaries, retained three verified thumbnails, displayed
the PNG and CHARX thumbnails rather than generic item icons, and restored the same library after a
normal application restart. Before provider selection, Agent Add exposed no premature nickname
field. A stopped DeepSeek Agent Session selected CHARX, retained that selection across restart,
replaced it with the Risu module, cleared it, and reapplied CHARX through the real controls.

That packaged flow found two product defects rather than accepting a partial UI pass. Commit
`aaacdaa` makes absent filesystem authority a valid complete pair for API providers while retaining
canonical path and file-identity revalidation for providers that own filesystem authority. An
identity without a path still fails closed. Commit `1bd2d05` extends the copied frontend's strict
`agent_session_created` projection with the server-owned persona ID and safe summary, including
exact ID/null consistency. Before that correction, the valid event was rejected and the room socket
reconnected; no permissive parser or client-owned persona state was added.

The configured official `deepseek-v4-flash` path then started through the copied controls, moved
through starting, idle, responding, idle, and stopped states, and completed one authorized ordinary
room turn. The human message asked about `harbor` using the active persona and lore. The public final
identified both the archive-guide persona and the embedded Risu lore that the module keeps the
harbor bell. This proves the selected private CHARX/Risu content reached the existing server-owned
provider input; it is not inferred from the picker label. No credential, hidden reasoning, private
provider identifier was copied from the isolated database into screenshots, logs, fixtures, or
committed documentation. No mock, substitute model, compatibility path, or fallback was used.

The first `make verify` was intentionally not reported as clean: it was started while the packaged
verification supervisor and sidecar were still running, and four process-custody tests failed to
establish their test guardians. After the exact app stopped normally, no matching process or SQLite
handle remained. The four exact tests then passed serially in 5.36, 5.21, 7.53, and 6.89 seconds,
and an unchanged second `make verify` passed as a whole. That clean run covered the architecture,
policy, and 800-line source-growth gates; the production frontend build and original CSS/cascade
check; 96 frontend files with 606 tests; 26 desktop tests; 42 domain, 214 persistence, 146 provider,
and 86 server unit tests; every TCP/WebSocket/integration/doc test; warning-denied workspace and
desktop Clippy; formatting; generated bindings; and `git diff --check`.

The exact test bundle, identifier-specific Application Support, WebKit and cache directories, and
fixture directory were permanently removed after confirming the Agent Session was stopped. The
Computer Use kernel was reset. No default application data, user-owned process, Deep Scan, or other
automated security scan was touched.

Manual review of the pushed completion range then exposed one real projection threat after the
initial persona corrections: the official response ID remained necessary as the private durable
`provider_turn_id`, but ordinary `message_final` and `turn_finished` events carried it to the shared
browser projection. Commit `5c2c998` adds only that key to the existing recursive public-redaction
owner. The focused domain regression proves the value is absent from public events; the existing
persistence and provider tests continue to prove that the private durable identity is retained for
exact replay, interruption, and reconciliation. Repository-wide search found no frontend consumer,
so the correction removes provider-private disclosure without changing a reachable browser action,
durable lifecycle, event count, or retry behavior.

The post-review validation also established a test-infrastructure cost rather than hiding it as a
product failure. Under concurrent host load, two unrelated frontend tests alternately exceeded the
default five-second worker limit; the same 96 files and 606 tests passed with two Vitest workers.
Provider process fixtures also crossed their old five-second readiness guard. An isolated diagnostic
root showed each test privately stages and synchronizes the verified current test executable
(67 MiB) before guardian and provider readiness; the affected marker arrived in 5.04–5.10 seconds,
and the guardian-death fixture completed in 6.93 seconds. Commit `4d4b8ed` first gives the shared
fixture marker, guardian readiness, and working-directory readiness one test-only 20-second owner.
Manual review then found one remaining pre-anchor marker whose child guardian performs the same
test-executable staging before publishing readiness. Commit `fc6cf9b` removes that local five-second
poll and reuses the shared owner. Together they cover the bounded filesystem-preparation and
protocol windows without changing a product startup, filesystem, protocol, cleanup, or security
timeout; a real hang still fails closed at the test boundary.

After that correction, the full provider suite passed with its normal concurrency (146 tests), the
server unit suite passed (86 tests), the process-heavy Agent Session boundary passed serially (9
tests in 103.53 seconds), and every remaining server TCP, WebSocket, HTTP, persona, attachment,
credential, invite, profile, directory, preferences, and runtime boundary passed serially (50
tests). The two-worker frontend run passed all 606 tests, the desktop suite passed all 26 tests, and
warning-denied workspace Clippy, formatting, architecture, policy, 800-line source-growth, generated
binding/build, CSS-cascade, and diff checks passed. The external applications creating the observed
host load were not stopped or modified. The critical web session approved exact
`fc6cf9b..f6f8636` and cumulative `f3c91e9..f6f8636` with C0/H0/M0/L0 after the evidence attribution
was corrected; Daybreaker Blue High independently approved the same final state with C0/H0/M0/L0.
No automated security scan was used.

## Lobby message-search backend and HTTP review: 2026-08-30

- Daybreaker Blue High found one Medium: long queries used only the compact candidate path and
  therefore lost the original `unicode61` phrase candidates. Commit `71161a5` combines both paths
  into one duplicate-free record result. Its follow-up Low found that the active specification still
  described the index by its old short-token role; commit `31c1a63` names its actual contentless
  phrase-candidate responsibility.
- The critical web review found one Low in `f6f8636..31c1a63`: Rust trimming did not include Python's
  U+001C-U+001F whitespace before the query character limit. Commit `3ba5111` restores the original
  shared cleaner's limit-before-and-after trim meaning and fixes that exact regression without a
  search-only shim.
- Final critical-web verdict for `31c1a63..3ba5111` and `f6f8636..3ba5111`: APPROVE C0/H0/M0/L0.
  Final Daybreaker Blue High verdict for the same corrected state: APPROVE C0/H0/M0/L0. Neither
  review used an automated security scan. This approves only the pushed backend/HTTP range; the
  copied frontend, RoomPortal tools, packaged and real-agent flows, and final measurements remain
  active work.

## Lobby message-search copied-frontend review: 2026-08-30

- The first cumulative review of `3ba5111..3809d08` found two Medium issues: valid empty optional
  provider provenance was rejected, and an already-dispatched context read could update a later
  room, channel, query, or authority state. It also found two Low strict-validation gaps in result
  ordering and context radius. Commits `2d19a11` and `16ffe96` closed those findings without adding
  a local-history fallback or a second request authority.
- Daybreaker Blue High then found one remaining Low in the cumulative state: two context selections
  under the same query shared a generation, so a late first selection could replace the second.
  Commit `828d632` gives context selection its own latest-intent generation while leaving paginated
  search ownership independent.
- Final critical-web and Daybreaker Blue High verdicts for correction-only `16ffe96..828d632` and
  cumulative `3ba5111..828d632`: APPROVE C0/H0/M0/L0. Neither review used an automated security
  scan. This approves only the pushed copied-frontend range; packaged local/read-only restart, the
  configured real-Agent matrix, and final measurements remain active work.

## Missing runtime-version poll deactivation: 2026-08-30

The copied `FrontendUpdateNotice` called the absent Rust `/api/runtime/version` route immediately,
every 15 seconds, and again on focus or visibility restoration, while swallowing every permanent
failure. That was at least 5,760 known-failing requests per continuously mounted client per day and
could not detect an update because no Rust route or generation owner exists. The component remains
copied source for the future rolling-restart slice, but the current Rust entry point and its cascade
preservation list no longer import or mount it. Repository search confirms the request remains only
inside that unreferenced component. This removes no working Rust behavior and adds no fallback, timer, state,
or replacement update claim; the missing runtime-version owner remains explicit in the exposure
inventory.

## Quiet room WebSocket keepalive: 2026-08-30

The server's existing five-minute client-ingress deadline previously had no browser-side ping
owner, so an otherwise healthy quiet room closed, reacquired a ticket, reconnected, and verified a
new snapshot every five minutes. The browser now schedules one authenticated protocol ping only
after three minutes without an authenticated client frame. Any command resets that one-shot timer,
and the exact connection close cancels it. The three-minute quiet threshold leaves two minutes
before the server deadline. A continuously connected quiet client sends at most
480 small keepalive frames per day; the path reads no HTTP route, database row, or filesystem state.
Focused fake-clock tests prove the authenticated counter sequence, exact pong matching,
command-based postponement, and close cleanup. The existing real server boundary already proves
authenticated ping/pong handling. The focused socket run passed 30 tests before exact-pong
validation; the final focused run passed 31 tests, the complete two-worker frontend suite passed
99 files and 622 tests, and the production build, original CSS check, architecture gate, and
800-line source-growth gate passed.

## Bounded unresolved command replay: 2026-08-31

The copied browser retained one exact command ID and serialized payload after authenticated ACK
silence, a post-send connection loss, or an `unresolved` NACK, but only its delay was capped. A permanent persistence or external-effect
uncertainty therefore kept acquiring tickets, authenticating sockets, reading snapshots, and
replaying forever; at the 30-second delay cap that was up to 2,880 connection cycles per day for one
pending command. Fresh IDs or an inferred result would violate at-most-once and authority contracts,
so the exact replay remains unchanged and gains only a terminal local budget.

One small browser-owned policy now permits eight total sends of the same bytes. The seven replay
delays are 500 ms, 1 s, 2 s, 4 s, 8 s, 16 s, and 30 s, totaling 61.5 seconds before the eighth
server reply, excluding connection and server latency. An eighth `unresolved` reply removes the
pending command, rejects its caller as `outcome_unknown`, and keeps the valid authenticated socket
open. An eighth ACK deadline does the same before replacing the unresponsive connection once without
replaying the command. Neither path reports a server rejection, erases durable authority, retries
under a new ID, or adds a fallback. The policy has no persistent state, worker, or independent timer;
the pending command continues to own its one existing retry timer and close cleanup. Focused
fake-clock tests prove exact byte replay after ACK silence, growing per-command delay across
successful handshakes, committed deduplication, both eight-attempt exhaustion paths, local failure
exposure, no ninth command replay, and the healthy retained or restored room connection. Both focused
files passed 28 tests, and the production TypeScript/Vite build plus the original-CSS verification
passed. The complete frontend run passed 100 files and 625 tests.

The first manual correction review found one remaining Medium: the per-connection set reserved a
request before asynchronous authenticated-frame signing and `WebSocket.send`, so a close during
signing could consume one of the eight attempts without sending a command. Retry charging also did
not itself reject a duplicate signal from the same connection generation. The connection-local
transmission owner now records `encoding` separately from `sent`, promotes only after `send` returns,
and close handling charges only `sent`. The retry-policy owner returns `already_counted`, `retry`, or
`exhausted` and alone claims each generation. This replaces the prior set rather than adding a
parallel authority, timer, worker, persistence, or fallback; its bounded memory remains one entry per
pending command. A gated-signature regression proves that a pre-send close does not alter the first
real send's 500 ms retry, and a policy regression proves one charge per generation. The focused six
tests and complete 100-file/627-test frontend run passed, as did the production TypeScript/Vite build,
original-CSS verification, architecture/source-growth/policy gates, and diff check. Final correction
approval remains pending both manual reviewers.

Daybreaker's next correction review marked that attempt-accounting Medium closed, then found one
separate Medium and one Low: close accounting could exhaust the eighth attempt while an already
received terminal ACK was still in authenticated-frame verification, and settled transmissions
remained in the connection map until close. Close accounting now runs after the existing ordered
verification queue and examines only commands still pending at that barrier. Committed ACK,
definitive NACK, and healthy-socket unresolved exhaustion remove their exact transmission entry when
they settle. This changes neither server authority nor retry count and adds no wait, timer, second
queue, or fallback. A WebCrypto barrier regression holds the eighth ACK verification across close
and proves the server-terminal result wins. The focused seven tests, complete 100-file/628-test
frontend run, production TypeScript/Vite and original-CSS build, architecture/source-growth/policy
gates, and diff check pass. Final correction approval remains pending.

Daybreaker's next review marked those Medium/Low findings closed and found one remaining Low: a
pre-send timeout could delete `pending` while leaving its separate `encoding` Map entry until the
connection closed. Rather than add another cleanup branch, the separate Map is removed. Each pending
command now owns one transmission generation and `idle`/`encoding`/`sent` phase alongside its retry
lifecycle; terminal pending removal releases all of it, and close scans only commands that still own
a `sent` phase for that exact generation. This reduces runtime state, leaves the existing pending
cardinality bound unchanged, and adds no task, timer, polling, fallback, or compatibility behavior.
The existing gated pre-send-timeout regression, focused seven retry tests, complete 100-file/628-test
frontend run, production TypeScript/Vite and original-CSS build, architecture/source-growth/policy
gates, and diff check pass. Final correction approval remains pending.

Final manual source re-review found no remaining actionable issue. Daybreaker Blue High and the
critical web reviewer independently marked the publication, infinite replay, pre-send accounting,
verified-reply ordering, completed-command lifecycle, and pre-send-timeout findings closed. Both
approved `00c0e8b..e1f7cca`, `0a5e1b3..e1f7cca`, `36e827d..e1f7cca`, and cumulative
`828d632..e1f7cca` as C0/H0/M0/L0. Neither review used an automated security scan.

## Lobby message-search packaged and measurement evidence: 2026-08-31

The isolated copied release was built from the Rust search candidate at pushed `e1f7cca`; local
documentation-only commit `0781d9e` changed no executable. The original comparison remains
`d5046473010d1353a81ee38337360e6d98f7bd6f`. Computer Use launched only `AgentsAssemble Search
Verify 51` (`app.agentsassemble.rust.searchverify51`) and the isolated admitted browser needed for
the remote-human path.

The local host committed `SEARCH_E2E_BEFORE_20260831`, `SEARCH_E2E_TARGET_20260831 harbor needle`,
and `SEARCH_E2E_AFTER_20260831`. Channel search returned one canonical result; selecting it showed
the preceding, target, and following records rather than the loaded-timeline fallback. A normal
application quit and relaunch preserved both the messages and search result. A read-only guest
admitted through the real tunnel repeated search, context navigation, and reload persistence while
its composer remained disabled. With a reusable five-use invite, one fresh guest was admitted,
the host revoked the invite, and a second fresh identity was rejected as invalid. The first session
remained active because invite revoke prevents future admission and does not revoke an independently
issued room session. The persisted invite recorded `use_count=1` and `revoked=1`.

Real provider verification did not accept an echoed success token as sufficient evidence. Three
adjacent public messages were committed as `CTX_PRE_a7p9m2`, `CTX_TARGET_q4v8n1`, and
`CTX_NEXT_z6k3r5`. A fresh Codex `gpt-5.6-terra` Low, room-read-only native app-server session was
given only the target marker and instructed to call `search_messages` and `read_message_context`.
It returned `CTX_PRE_a7p9m2|CTX_NEXT_z6k3r5`. The resumed real Antigravity
`gemini-3.6-flash` Medium PTY session received only the same target marker and used its exact room
search/context helpers; it returned the same withheld pair. Antigravity remained on the existing
live CLI path, with no print or transcript mode.

The configured OpenCode `opencode/hy3-free` packaged session became recovery-required and exposed
no turn event, standard error, or fabricated reply. Its copied UI remained `응답 중` until explicit
stop, while the interrupt control reported that `agent.interrupt` was absent from the bound server
product surface. A separately authorized direct installed-free-model probe exited immediately with
an external provider error. That evidence leaves OpenCode explicitly unavailable for this run and
records the UI provider-failure/interrupt gap; no credentialed model, alternate model, mock, retry
loop, or fallback substituted for it.

A temporary release probe measured the final search schema with 100,000 representative canonical
messages. Initial canonical/search/live-database sizes were 54,829,056, 46,383,104, and 101,576,704
bytes. After 100 actual `message.send` writes they were 54,882,304, 46,428,160, and 101,752,832
bytes. Twenty-five selective reads measured 21.861 ms median, 22.539 ms p95, 22.723 ms maximum, and
21.985 ms mean. Twenty-five absent reads measured 20.732 ms median, 21.394 ms p95, 21.691 ms maximum,
and 20.859 ms mean. The 100 writes measured 0.530 ms median, 0.772 ms p95, 1.245 ms maximum, and
0.557 ms mean. Maximum RSS was 16,449,536 bytes and peak memory footprint was 7,160,288 bytes.
Search owns zero background tasks or recurring timers; result and context allocations remain bounded
to 30 and 31 records on the existing single SQLite connection. Direct dataset construction took
4.978 seconds and is recorded only as probe setup, not production write performance.

After the two real helper results, the Antigravity session was stopped through the copied UI and the
release app exited normally. Process inspection found no verification-owned app, Rust server,
provider, or tunnel process. Computer Use was reset. The exact app, Application Support, cache, and
WebKit directories were moved to the recoverable
`~/.Trash/AgentsAssemble-verification-20260831-0207` bundle. The removed performance database is in
`~/.Trash/AgentsAssemble-search-measurement-20260831-0213`; the failed first probe and OpenCode probe
are separately recoverable in Trash. No user application, provider, profile, or uncommitted source
was stopped, moved, or modified.

The unchanged post-cleanup `make verify` passed the architecture, policy, 800-line source-growth,
generated-binding, original-CSS, formatting, and diff gates; the production frontend build; 100
frontend files with 628 tests; 26 desktop tests; 44 domain, 218 persistence, 150 provider, and 88
server unit tests; nine process-heavy Agent Session boundaries; and every control-pipe, real-TCP,
WebSocket, HTTP, invite, profile, attachment, pin, search, preference, directory, credential, and
runtime boundary. Warning-denied workspace and desktop Clippy both passed.

## Failure-owned room publication retry: 2026-08-30

Every active room previously queried the durable publication cursor every 250 milliseconds even
after a successful empty drain. One idle room therefore opened and committed four SQLite read
transactions per second, or 345,600 per day; each transaction also executed cursor initialization,
cursor selection, and pending-event selection. The existing bounded wake channel already receives
normal external commits, while room-owned commands, admissions, provider results, room tools, and
recovery already drain at their ordering boundary.

The room actor now has no retry deadline after a successful startup or publication. An actual drain
failure returns one typed `PublicationAttempt::Retry` to that same actor, which schedules a 250 ms
one-shot retry and exponentially backs persistent failure off to a five-second cap. Success clears
the deadline and resets the delay. Startup backlog, commit-before-provider ordering, exact durable
cursor order, wake coalescing, restart recovery, and unresolved create/start publication remain
preserved; no writer gains a second broadcaster or fallback. Focused tests prove missing-room
failure classification, failure-only/capped/reset retry state, external profile wake publication,
and blocked N+1 before N+2 cursor order. The first full server run then exposed one cross-room
dependency formerly hidden by the interval: admitting the same person to another room committed a
profile update for the first active room but woke only the admission room. The admission actor now
wakes every other already-active affected room after commit, even if the HTTP reply owner was
dropped; inactive rooms create no task and drain on their next real subscription. The exact
cross-room and dropped-reply regressions pass. On the exact final candidate, warning-denied
all-target server Clippy passed, as did 88 server unit tests and all 63 real TCP, WebSocket,
control-pipe, Agent Session, invite, profile, attachment, pin, search, preference, directory, and
runtime boundary tests. Formatting, architecture, 800-line source-growth, policy, and diff gates
also passed. The publication retry policy lives in the 258-line publication owner; the room actor
remains 726 lines and only connects that owner to room inputs.

A later manual whole-repository review found that the five-second delay cap did not bound the
failure epoch: a permanent database fault still caused one SQLite drain and error log every five
seconds until process cancellation. The retry owner now permits one initial failed drain plus seven
timer-owned retries (250 ms, 500 ms, 1 s, 2 s, 4 s, 5 s, and 5 s), then disarms after the eighth
consecutive failure. That bounds one failure epoch to eight drain attempts over 17.75 seconds,
excluding each database call's existing 250 ms busy timeout. Pending rows and the canonical cursor
are not altered or reported as delivered; the next real room input or external commit wake may
observe recovery, and one successful drain resets the epoch. The accepted trade-off is that a room
with no later activity waits for restart rather than polling a recovered database forever. The
room owner emits one distinct exhaustion error at that terminal transition. The focused fake-clock
owner test proves the exact schedule, exhaustion transition, no re-arm after exhaustion, and success
reset; all four publication-owner tests pass.

## Pending-only lifecycle reconciliation scan: 2026-08-30

The live one-second reconciler is retained because it owns a concrete failure contract: a room or
provider task may end after durable external-effect authority is committed but before that owner
returns a typed result. Existing owner-loss tests prove recovery without a browser request, while
startup reconciliation remains the complete pre-admission integrity owner. The interval therefore
does not justify rescanning unrelated Agent Sessions.

Previously, one lifecycle page selected up to 64 ordinary sessions and issued three additional SQL
statements per row: blocking provider-turn lookup, session/room load, and pending-reservation load.
A full inactive page was 193 statements plus its transaction boundaries. Clean schema 50 adds one
partial `(room_id, session_id)` index containing only `status = 'pending'` lifecycle reservations.
The page now starts from that exact owner, excludes blocking provider turns in the same query, and
performs the two complete candidate reads only for selected unresolved work. An isolated
`EXPLAIN QUERY PLAN` over the exact relevant schema used the covering pending index, the Agent
Session primary key, and the blocking-turn partial index, with no DISTINCT temporary B-tree. Thus
an empty lifecycle scan is one candidate statement rather than `1 + 3N` statements for `N`
ordinary sessions. The index adds no terminal reservation history and only the rare lifecycle
write/update/delete maintenance cost.

The change preserves the 64-candidate bound, exact in-memory request claim, complete stored
authority validation, provider-turn precedence, candidate CAS, two-second observation timeout,
eight-observation concurrency, fail-closed uncertainty, and failure logging. Session JSON and its
pending reservation are still written atomically by the single process-locked database owner;
startup still detects corrupt or orphan session authority before network admission. Schema 49 is
rejected without migration or compatibility behavior. All 217 persistence tests, 88 server unit
tests, and 63 serial real TCP/WebSocket/integration tests passed. That includes dynamic
post-admission discovery, owner-loss recovery, safe replay, exact tombstone release, startup cleanup
of an active runtime without lifecycle intent, Agent Session restart, control-pipe, invite, profile,
attachment, pin, search, preferences, directory, and runtime boundaries.

## Lobby history packaged completion candidate: 2026-08-31

The isolated release was built from local Rust `bfb6ccf`, on pushed server correction `b95e128`,
and compared against original `d5046473010d1353a81ee38337360e6d98f7bd6f`. Its product name was
`AgentsAssemble History Verify 831A` and its distinct identifier was
`app.agentsassemble.rust.historyverify0831a`; the central URL was explicitly empty. The exact local
HEAD passed `make verify` before packaging: architecture, policy, 800-line source-growth,
generated-binding, formatting, original-CSS, and diff gates; the production frontend build and 101
files with 639 tests; 26 desktop tests; 45 domain, 220 persistence, six protocol, 150 provider, and
94 server unit tests; every real TCP/WebSocket/integration/doc test; and warning-denied workspace
and desktop Clippy.

Computer Use created a fresh local profile and room through the copied packaged UI, then committed
205 distinct messages through its real composer. Read-only SQLite observation found a contiguous
room record through sequence 206 including room creation. A normal quit released the exact server
writer. Relaunching the same package restored the room and latest snapshot; the first message and
channel introduction were absent before the timeline reached its top, then both became visible
after the copied top-history interaction.

The host opened the owned quick tunnel, changed the room-owned invite scope to read-only, and
issued one one-use human invite. A fresh Chrome incognito window completed the production preflight,
profile, and join flow as `History ReadOnly 831A`. The URL token was removed after admission, the
read-only reason was visible, and the composer stayed disabled. Its latest snapshot did not expose
the first fixture message; scrolling to the top made that message and the true channel introduction
visible. Browser reload retained the admitted session without a token in the URL and reproduced the
same latest snapshot and top-history read.

Before the final reload/read interaction, the isolated store contained 208 room events and 206
command results. Both counts were identical afterward. The consumed invite was `read_only` with
`use_count=1`, `max_uses=1`, and no revocation. This is direct evidence that the observed history
read created no room event or replay/result state; it is not a general performance claim. No
provider ran.

The verification-only incognito window closed while the user's normal Chrome window remained. The
host UI stopped public ingress and the owned `cloudflared` process disappeared before application
shutdown. Normal quit left no matching packaged app, supervisor, sidecar, server, or SQLite writer.
Only the exact package and identifier-specific Application Support, cache, and WebKit directories
were moved to the recoverable
`~/.Trash/AgentsAssemble-History-Verify-20260831-0415` bundle (50 MiB); shared Cargo caches and user
data were untouched. Computer Use was reset.

Daybreaker Blue High manually re-reviewed pushed `4cbbdcd..b95e128` and cumulative
`e1f7cca..b95e128`. It marked the earlier unbounded history-read High and broader-scope debit Medium
CLOSED and returned APPROVE C0/H0/M0/L0 for both exact correction and cumulative ranges. It ran no
automated scan, test, provider, or app. At that checkpoint, critical web review and review of the
local frontend threshold batch were pending, so this section did not claim their approval.

## Lobby message-mutation packaged completion candidate: 2026-08-31

The isolated copied release was built from local Rust `a81d3ad` on pushed baseline `a958bab`, with
`VITE_AGENTSASSEMBLE_CENTRAL_URL` empty. Its distinct product name was `AgentsAssemble Mutation
Verify 831A` and its identifier was `app.agentsassemble.rust.mutationverify0831a`. Before packaging,
the exact candidate passed `make verify`: architecture, policy, source-growth, generated-binding,
original-CSS, formatting, and diff gates; the production frontend build and 106 files with 657
tests; 26 desktop tests; 55 domain, 233 persistence, six protocol, 152 provider, and 94 server unit
tests; all real TCP/WebSocket/integration/doc tests; and warning-denied workspace and desktop
Clippy.

Computer Use created a fresh local profile and room through the copied packaged UI. The local human
sent `MUTATION_LOCAL_ORIGINAL_20260831`, edited it through the real menu and dialog to
`MUTATION_LOCAL_EDITED_20260831`, and observed the edited marker. It then sent and edited a second
message to `MUTATION_RESTART_EDITED_20260831`. Normal application quit and relaunch retained both
edited states. Later, deleting the first message through the real confirmation dialog and restarting
again retained its tombstone while preserving the second edited message and marker.

The host opened its owned quick tunnel and issued a one-use read/write invite. A fresh Chrome
incognito identity joined through the production preflight/profile flow as `Mutation RW 831A`; the
token disappeared from the URL and the guest observed the host's prior edits. It sent and edited its
own message to `MUTATION_RW_EDITED_20260831`, which the host observed live. The guest's real delete
dialog named that exact edited value; confirmation produced `삭제된 메시지입니다` for both clients.

The host then changed the room-owned invite scope to read-only and issued a separate one-use invite.
One unused read-only invite was explicitly revoked during verification and a replacement was issued;
no token from the revoked invite was admitted. A second fresh incognito identity joined as
`Mutation RO 831A`. The explicit read-only reason was visible; composer, attachment, app, mention,
emoji, and send controls were disabled; and message hover exposed no pin, edit, or delete action.
Tokenless browser reload retained the session and exact denial. The user's normal Chrome window was
not closed or modified.

Read-only SQLite observation after these interactions found 14 room events, 11 command results, and
three invite rows. The consumed read/write and read-only invites each had `use_count=1`,
`max_uses=1`, and no revocation; the unused revoked read-only invite had `use_count=0`. Current and
transition event aggregates showed one room creation, two joins, three room-settings updates, three
message updates, two message deletions, two current deleted messages, and one current nondeleted
message. A direct query found zero deleted current rows retaining nonempty content or nonempty
attachment metadata. These counts describe this isolated run only.

No provider ran and this slice added no polling, heartbeat, periodic timer, retry, fallback,
reconciliation loop, or background task. One final idle point sample observed 0.0% CPU for the
desktop, supervisor, and server, with RSS of 122,016 KiB, 9,728 KiB, and 21,232 KiB respectively.
An earlier tunnel-active point sample also observed 0.0% CPU, with desktop, supervisor, server, and
owned tunnel RSS of 114,752 KiB, 9,664 KiB, 21,664 KiB, and 43,840 KiB respectively. These are point
observations rather than latency or general performance claims. The isolated identifier-owned data
used 492 KiB of Application Support and 76 KiB of caches; the packaged application used 50 MiB.

Public ingress was stopped through the copied UI before application shutdown and the exact owned
tunnel process disappeared. Normal quit left no matching desktop, supervisor, server, or tunnel
process. The exact application, identifier-specific Application Support/WebKit/cache state, and
regenerable runtime staging were moved to the recoverable
`~/.Trash/AgentsAssemble-Mutation-Verify-20260831-2100` bundle; regenerable artifacts from one
never-launched packaging attempt were included in that same bundle. No shared build cache, user
profile, user browser, provider, or uncommitted source was removed, and Computer Use was reset.

The cumulative threshold batch and packaged results received both required manual source approvals.
Daybreaker approved through `d168354` at C0/H0/M0/L0. The independent critical-web review then
approved pushed HEAD `23c9f35`, cumulative `a958bab..23c9f35`, and corrections
`edd9ce4..23c9f35` at C0/H0/M0/L0.

## Codex code-mode-host custody correction: 2026-09-01

The prior packaged pause run exposed a real custody boundary rather than a pause defect. Codex
0.147.0 started its internal code-mode host in a process group outside the guardian-owned anchor.
UI Stop killed and reaped the guardian, anchor, and `app-server`, but the escaped companion kept the
generation from receiving its exact cleanup receipt. Persistence correctly retained
`provider_stop_unconfirmed` and recovery authority. Process absence was never promoted to proof, and
no compatibility path, retry, polling loop, timer, or cleanup fallback was added.

The correction keeps the official mechanism and changes only its owner. Discovery and binding now
treat the installed Codex executable and `codex-code-mode-host` as one byte-identified bundle. The
existing private executable staging owns both files. The stopped provider launcher starts the
companion explicitly with `--listen ws://127.0.0.1:0`, verifies the child is still in the anchor
group before and after a bounded 128-byte readiness line, accepts only the canonical
`ws://127.0.0.1:<nonzero-port>` form, and then execs `app-server --code-mode-host <endpoint> --stdio`.
The companion inherits the sanitized provider environment plus only the runtime custody token; the
RoomPortal bearer remains with `app-server`, and inherited descriptors 4 through 8 are replaced by
`/dev/null`. Missing, changed, ambiguous, non-loopback, exited, or group-escaping companion
authority fails the launch and leaves cleanup to the existing guardian owner.

The focused Unix regression proves the companion endpoint reaches the provider, the companion is
not its own process-group leader, and exact adapter Stop removes it. Fixture descriptors also try to
write a protocol marker through descriptor 5 before readiness, exercising the descriptor
replacement boundary. The unchanged complete `make verify` passed architecture and source gates,
657 frontend tests, 26 desktop tests, all Rust unit/integration/TCP/WebSocket tests, warning-denied
Clippy for every target and feature, and the diff gate.

A fresh isolated copied release then created one stopped `gpt-5.6-terra` session through the real
agent modal, started it through the copied member controls, and sent one addressed room message.
Terra replied exactly `CODE_HOST_STOP_901`. While resident, the observed guardian, anchor,
`app-server`, and code-mode host RSS values were 8,592 KiB, 7,984 KiB, 110,176 KiB, and 22,032 KiB.
The anchor, `app-server`, and code-mode host shared one process group, and `lsof` found the companion
listening only on one IPv4 loopback endpoint. These are point measurements that establish the added
resident process cost; they are not a CPU, latency, or leak improvement claim. The cost is accepted
because current official Codex code-mode execution requires that companion, while explicit external
launch closes the observed custody escape.

The copied UI Stop immediately reached `stopped`; all four captured process identities were absent.
Read-only durable inspection found `runtime_status=stopped`, no error code, and
`recovery_required=false`. Normal application quit left no isolated desktop, supervisor, server, or
provider process. Computer Use was reset, and only the isolated app bundle, identifier-owned
Application Support/WebKit/cache state, and temporary package configuration were moved to the
recoverable `~/.Trash/agentsassemble-code-host-0901-final-20260901-0341` directory. Unrelated
ChatGPT/Codex processes, shared build artifacts, user files, and uncommitted source were untouched.

The LOC review found `codex.rs` at 803 lines and `guardian.rs` at 893 lines, so both received the
required strong structural check. `codex.rs` remains one JSONL protocol and thread/turn state owner;
the new 149-line companion lifecycle was separated rather than mixed into it. `guardian.rs` remains
the single private launch-manifest, stopped-launcher, process-group, and cleanup-receipt invariant;
extracting that state flow would increase cross-module state transfer and private interfaces. No new
public trait, configuration layer, background task, or parallel lifecycle owner was introduced.
The threshold corrections and their final manual approvals are recorded in the next section.

## Pause/custody threshold-review corrections: 2026-09-01

Manual review of pushed `23c9f35..a340a31` diverged. The critical web session returned
APPROVE C0/H0/M0/L0. Daybreaker returned REVISE C0/H0/M2/L0: Linux/Android could drop the staged
multi-file Codex bundle after guardian readiness but before the resumed launcher opened its paths,
and a fresh pause/resume trusted durable resident identifiers without consulting the live adapter.
No other finding was reported. The correction range begins at `c53fa5a`.

The Codex issue was a concrete lifetime race introduced when the native companion changed Codex
from the ordinary Linux/Android single-file `memfd` path to a filesystem-staged two-file bundle.
The resident driver now retains `BoundExecutable` on every platform. A test-only Unix-stream barrier
blocks the resumed provider launcher after guardian readiness and reports both launch paths; the
test proves both verified staged files still exist before releasing the launcher, then completes
attachment and exact owned shutdown. The barrier has no production branch, timer, polling, retry,
or sleep. Runtime cost is the required lifetime of the existing private staging directory and its
already-open executable/companion files; no additional production process, copy, or scan is added.

The resident-state issue could otherwise produce a false `process_preserved`/`process_reused`
claim, and resume could advance queued work toward a dead or mismatched runtime. Exact command replay
still returns before mutable runtime consultation. Every fresh state-only pause/resume now performs
one on-demand adapter check of the exact slot, handle, owner, lease, profile, absence of an active
turn, driver liveness, and safe attachment state. The database transaction then compares the same
identity before writing. Missing, borrowed, stopped, dead, uncertain, or changed runtime authority
rejects without a state event, command result, or consumed durable room budget. It does not perform
full filesystem selection revalidation, and adds no background task, polling, heartbeat, timer,
retry, reconciliation fallback, new lifecycle owner, or provider-specific branch. The accepted
cost is one existing bounded driver-health exchange per fresh pause or state-only resume; replay and
unrelated commands pay none.

Focused verification passed the stopped-launcher staging test. That test now waits concurrently for
the launcher barrier and `CodexDriver::spawn` to return, so the returned driver—not a caller-local
launch variable—owns the executable guard before either staged path is inspected or the launcher is
released. It then attaches and stops that same driver and confirms the cleanup receipt. Focused
verification also passed provider resident proof success/borrowed/stopped cases,
persistence replay/conflict/stale-proof and no-mutation cases,
the real TCP/WebSocket lifecycle boundary through Codex start/pause/resume/stop, and warning-denied
workspace Clippy. The fresh Android cross-check first exposed two existing guardian cfg defects once
the installed versioned NDK compiler was supplied: `set_child_subreaper` returned `Errno` where the
guardian boundary required an explicit `io::Error`, and Android did not consume the macOS-only fork
policy during cleanup. The minimal correction preserves the same behavior, compiles warning-free on
macOS, and `cargo check -p agentsassemble-provider --target aarch64-linux-android --lib` then passed
with the installed target and NDK compiler. A subsequent complete `make verify` passed the source
and architecture gates, original-CSS check, 657 frontend tests and production build, 26 desktop
tests, all Rust unit/integration/TCP/WebSocket tests and doc tests, warning-denied workspace Clippy,
and `git diff --check`.

The resident-proof fixture initially pushed the provider runtime test module over the 800-line
strong-warning boundary and mixed provider-neutral residency invariants with Codex/guardian launch
tests. It now lives in an 83-line private sibling test module; the original module is 734 lines and
no production or public interface changed. The 947-line guardian remains a reviewed cohesive
exception below the absolute 1,000-line gate: launch-manifest custody, stopped-launcher handoff,
process-group ownership, and exact cleanup receipts share one state machine and invariant. Splitting
that owner would add cross-module state transfer and private interfaces while making the custody
proof harder to follow. `codex.rs` is 799 lines. These are structural decisions, not targets to grow
to; any separate authority, lifecycle, or change reason still splits immediately.

A fresh isolated macOS package named `AgentsAssemble Resident Verify 901` used its own bundle ID,
application-support directory, SQLite database, identity keys, logs, and room. Through the packaged
frontend, the exact `gpt-5.6-terra`, `gemini-3.6-flash`, and
`opencode/muse-spark-1.2-contributor-free` sessions each produced an exact initial marker, entered
idle pause, accepted an addressed message without starting or showing a provider turn while paused,
and after resume produced the exact queued marker before returning idle. OpenCode's two Muse results
were disambiguated by the exact free model ID; no Go credential, paid model, substitute, fallback,
or direct database mutation was used. Every provider was stopped through its session control. The
exact app, owned server/provider children, and staging directories were gone after quit; the unique
bundle, application data, and temporary Tauri config were moved to the user's Trash for recovery.
No unrelated app, provider, user data, shared build cache, or user-owned working-tree change was
touched.

Critical-web review of intermediate pushed HEAD `ffd4fd8` returned REVISE C0/H0/M1/L1. The Medium
finding showed that the new `agent.resume` resident preflight rejected an exact existing lifecycle
reservation before its established retry/recovery owner could classify it. The Low finding showed
that the first stopped-launcher fixture could inspect staged paths before `CodexDriver::spawn`
returned, so it did not causally prove that the resident driver retained their guard. No other
finding was reported.

Commit `81b74fe` closed the Low finding by waiting concurrently for the launcher barrier and the
returned driver, then using that same driver for path inspection, attachment, exact stop, and
cleanup-receipt verification. Commit `0821b0a` closed the Medium finding by reusing the existing
`ExistingRequestIdentity` owner: committed results replay directly, while exact pending or rejected
lifecycle identities continue through the existing launch/reservation owner. Changed action or
payload identity still conflicts. New WebSocket boundary tests prove current-generation pending
resume completion plus replay and stop, stored rejected-reservation replay, and previous-generation
prepared-reservation startup recovery. The complete `make verify` passed at `0821b0a`.

Daybreaker manual source review and the independent critical-web review both approved individual
`0821b0a`, cumulative `a340a31..0821b0a`, and pushed HEAD `0821b0a` at C0/H0/M0/L0. Both reviews
included authority/SSoT, lifecycle, duplication, overimplementation, repository structure, LOC,
fallback/polling/heartbeat/timer/retry/failure-swallowing, performance-evidence, and fixture-quality
checks. Neither reviewer ran an automated security scan.

## Agent Session idle pause/resume packaged candidate: 2026-09-01

The isolated copied release was built from local `ab4ab78` on pushed baseline `23c9f35`, with the
distinct product name `AgentsAssemble Pause Verify 831` and identifier
`app.agentsassemble.rust.pauseverify0831`. The active implementation adds no schema, migration,
compatibility path, provider branch, process restart, background task, polling, heartbeat, timer,
retry, or fallback. Pause is one persistence-owned state transition; resume uses that same owner and
then the existing ordered-floor progression boundary.

Computer Use selected the actual Rust project workspace and exercised exact installed models rather
than substitutes. Codex `gpt-5.6-terra` returned `CODEX_FIRST_831`, paused while idle, accepted a
direct mention without starting a turn, and after resume returned `CODEX_PAUSE_831`. The same Codex
provider PID, runtime handle, and provider conversation remained present across pause and resume.
Antigravity `gemini-3.6-flash` similarly returned `ANTIGRAVITY_FIRST_831`, retained its guardian,
anchor, PTY provider, runtime handle, and conversation while the paused input remained pending, then
returned `ANTIGRAVITY_PAUSE_831` after resume. It never used print mode.

The installed OpenCode 1.17.18 catalog exposed exact
`opencode/muse-spark-1.2-contributor-free`; the retired Hy3-free identifier and every paid or
fallback model remained unused. The copied selector created that exact read-only workspace session.
After one normal application restart, resume reused its private provider conversation and a new
persistent loopback server. Its first real turn returned `OPENCODE_FIRST_831`. Idle pause retained
the exact guardian, anchor, `opencode serve` PID, runtime handle, and provider conversation; the
addressed `OPENCODE_PAUSE_831` input remained `pending=1`, `inflight=0`, and `turn_count=1` with no
visible provider response. Resume consumed it through the existing floor owner, returned exactly
`OPENCODE_PAUSE_831`, and reached `turn_count=2` on the same resident process set and identities.

The resident-process comparison observed no process replacement across any pause/resume pair.
OpenCode's point RSS sample was 919,392 KiB before and during pause and 773,136 KiB after the queued
turn; Antigravity's observed provider process remained resident at 182,768 KiB after resume. These
are upstream point observations, not a claimed optimization or leak diagnosis. The state-only
implementation performs no provider I/O at pause, adds no allocation owner or disk record beyond the
existing command result/event, and preserves the existing combined 256-input queue bound.

Antigravity and OpenCode stopped through the copied control with confirmed cleanup, detached/stopped
durable state, and no owned process left. Codex stop killed and reaped its exact guardian, anchor, and
provider tree, but the macOS guardian did not write the required generation-bound cleanup receipt.
The runtime therefore emitted `provider_stop_unconfirmed` and persistence retained
disconnected/recovery-required authority instead of claiming a successful stop or admitting a
replacement. Process absence alone was not promoted to security proof. This is an explicit cleanup-
receipt follow-up, not a successful-stop claim and not a reason to add a fallback.

The exact app and all provider processes were normally shut down. Computer Use was reset, and only
the identifier-owned Application Support, WebKit, cache, preferences, temporary package config, and
isolated app bundle were moved to the recoverable
`~/.Trash/agentsassemble-pause-verify-0831-cleanup-20260901-0131` directory. Shared Cargo artifacts,
user files, other applications, and unrelated providers were untouched. The post-cleanup unchanged
`make verify` passed all mandatory architecture, policy, source-structure, generated-binding,
frontend, Rust unit/integration/TCP/WebSocket, warning-denied Clippy, and diff gates. The later
threshold-correction section records the final critical-web and Daybreaker approvals.

## Lobby message-mutation review corrections: 2026-08-31

Daybreaker manual source review of pushed `a958bab..edd9ce4` returned REVISE C0/H0/M1/L1. The
Medium finding was unbounded historical vote-transition rewriting during poll deletion under the
global SQLite writer. The Low finding was elapsed-time-based TCP absence evidence using 50/100 ms
silence windows and a one-second close timeout. This section records the correction evidence only;
critical-web review and Daybreaker re-review remain pending.

The old delete path selected and decoded every transition for the deleted poll and then updated each
matching row. A controlled fixture with 14,400 transitions measured about 6.34 MiB and 180.6 ms for
that read/decode portion before the 14,400 writes. Because a poll may remain open indefinitely, the
per-minute room command budget did not bound the lifetime row count or global-writer occupancy.

The correction moves privacy minimization to the durable vote-write boundary. One domain function
constructs the stored and replayed transition marker with only event identity, sequence, room, time,
kind, and `vote_id`; current identity and choice remain owned by the vote projection and ballots.
Poll deletion now removes that projection and the poll secret without reading or rewriting historical
transitions. Daybreaker correction review then found that current ballot rows could still grow with
unbounded sequential admissions to an indefinite poll. The vote owner therefore caps one poll at
192 current ballots: the complete concurrent room surface of 112 public-human, 16 reserved
operator/external, and 64 Agent Session participants. Existing-ballot replacement and withdrawal
remain available at capacity, and a withdrawal releases one slot. Schema 54 is a clean
stored-contract cutoff and rejects older stores without migration, fallback, or compatibility
behavior. A trigger that rejects any transition-row update remains armed while deletion succeeds,
the transition rows compare byte-for-byte before and after, and the stored command result carries
the same minimized marker.

On the same 14,400-transition debug fixture, the corrected real deletion completed in 10.650 ms and
database page bytes remained 4,947,968 before and after. These are controlled point observations,
not production latency claims. Event cursor identity and current vote summaries are preserved; no
index, cache, task, cleanup loop, timer, polling, heartbeat, retry, fallback, or swallowed failure was
introduced.

The maximum-ballot fixture proved that the 193rd distinct ballot fails with
`vote_capacity_reached` without a partial event or command result; an existing voter can still
replace and withdraw, the released slot accepts the previously rejected voter, and deleting the poll
removes all 192 ballot rows. The real deletion took 1.035 ms in that debug in-memory point. This is
evidence for the absolute work bound, not a production latency claim. The cap uses one domain
constant and one persistence enforcement owner; no duplicated SQL quota or schema CHECK was added.

Daybreaker source tracing found that provider terminal rejection commits its error/finalization in
the same transaction rather than propagating the command error. Because the cast event was inserted
before the capacity check, that path could retain an anonymous but false `vote_cast` marker even
though no ballot or tally changed. The cast owner now performs ballot validation/replacement before
event insertion in the same transaction; later event or projection failure still rolls every write
back. A provider fixture fills all 192 ballots with departed participants, assigns one real Agent
Session turn, and attempts a new cast. The commit contains only `error`, `turn_finished`, and
`agent_session_state`, reports `vote_capacity_reached`, has zero `vote_cast` rows after the pre-call
cursor, and retains 192 unchanged Yes votes. The correction adds no preflight query, compensating
write, transaction, task, timer, retry, or fallback.

The TCP correction removes all uses of the shared elapsed-silence helper. Exact replay is proven by
the next durable snapshot and event sequence containing exactly one target mutation, read-only
rejection is bracketed by unchanged durable room state, and server rejection closure is awaited as a
causal socket event without an arbitrary deadline. Daybreaker manually approved pushed HEAD
`d168354` and cumulative `a958bab..d168354` at C0/H0/M0/L0. The independent critical-web review
approved pushed HEAD `23c9f35`, cumulative `a958bab..23c9f35`, and corrections
`edd9ce4..23c9f35` at C0/H0/M0/L0.

## Agent Session explicit interrupt local candidate: 2026-09-01

Commits `8d4c91b`, `3d432f1`, and `be7c4fc` connect the copied busy-session
`agent.interrupt` control without adding a second provider or lifecycle state machine. Clean schema
55 adds one constrained `interrupt_cause` field to the existing exact provider-turn effect owner.
The field distinguishes `participant_muted` from `agent_interrupt` across replay and restart so the
shared finalizers preserve each product contract: mute retains its established floor progression,
while explicit interrupt restores inflight input to pending and does not immediately assign it
again. Both retained-runtime and runtime-gone paths consume the same durable cause.

The command requires current server-derived `agent.control`, active membership, a joined unmuted
Agent participant, complete busy-turn authority, and exact session/execution/runtime
handle-owner-lease agreement. Its command result and complete `agent_session_state` event commit in
the same transaction as the interrupt effect before provider I/O. Same identity replay returns that
accepted result, changed reuse conflicts, and another request cannot own the unresolved exact turn.
The ACK means durable acceptance; uncertain dispatch or quiescence remains visible durable recovery
authority. Public interruption output uses only fixed bounded text and code `interrupted`, so no
provider diagnostic, path, token, or private runtime identity is copied into events or state.

The prior common effect row could express only participant-mute semantics. Reusing it for the copied
control without a durable cause would either schedule the restored input immediately or lose the
reason during restart. The accepted storage cost is one short constrained TEXT value per interrupt
effect plus the existing command-result/state-event records. No new process, task, cache, queue,
allocation owner, polling, heartbeat, timer, retry, fallback, provider branch, compatibility path,
or swallowed failure was introduced. Explicit finalization deliberately skips the existing
assignment query and provider scheduling call; this is required product behavior rather than a
speculative performance claim. The 766-line effect module remains one exact-effect claim,
dispatch-fence, quiescence, and recovery state machine. Splitting it would add effect-state transfer
and private interfaces; it remains below the 800-line strong warning. The 802-line room-turn test
root is a module router plus the single room-turn invariant suite; the new behavior itself is in a
160-line sibling test module, so no production owner or forwarding abstraction was added to meet a
line target.

Focused persistence verification proves pre-dispatch interrupt, exact replay and changed conflict,
competing-request rejection, retained runtime/provider conversation, terminal execution, restored
input, runtime-gone cause survival, fixed public diagnostics, and no immediate assignment. The
existing ten participant-mute effect tests also pass unchanged. The real TCP/WebSocket boundary
starts an owned Codex app-server fixture, observes exactly one `turn/start`, sends exactly one
official `turn/interrupt`, receives the committed accepted ACK and terminal events, returns to
attached/idle with the provider session active, observes no second `turn/start`, and replays the ACK
without another effect. The generated product surface advances to revision 12; its canonical digest
fixtures were recomputed from the same length-delimited action registry rather than bypassing the
integrity check.

The complete `make verify` gate passes: architecture and source-structure policy, generated
bindings, original-CSS comparison, the production frontend build and 657 frontend tests, 26 desktop
tests, all Rust unit/integration/TCP/WebSocket tests and doc tests, warning-denied workspace Clippy,
and `git diff --check`. Packaged copied-frontend verification with the exact allowed real providers
and restart recovery remain pending; this local candidate is not yet declared complete.

### Retained-interrupt capability correction

Critical-web review of pushed `6f9115a` found that the Antigravity Ctrl-C implementation poisoned
the driver and required the common runtime owner to stop it after the interrupt command had already
been accepted. That contradicted the copied control's same-runtime/same-conversation contract. The
review also found two independently written public interruption diagnostic literals; `e4bf62e`
centralizes those literals in the persistence-private interrupt owner.

An isolated interactive probe used the exact installed Antigravity CLI 1.1.22 and model
`gemini-3.7-flash-low`; it did not use print mode. A turn running `sleep 30` received Ctrl-C, returned
the native interrupted prompt, kept the same opaque conversation identity, and left no sleep
process. Its configured official synchronous Stop hook did not run. A subsequent ordinary
`Reply exactly ok.` turn did run the same hook and reported that same identity plus
`terminationReason=NO_TOOL_CALL` and `fullyIdle=true`. The transcript had no explicit cancellation
receipt. This proves that Stop-hook or transcript authority cannot currently establish retained
quiescence after Ctrl-C; it does not prove packaged command behavior.

Commit `5aa5219` moves that uncertainty before durable command admission. One driver-owned immutable
capability is captured when the resident runtime launches. Codex and OpenCode opt in because their
native exact interrupt protocols already provide retained completion; Antigravity and DeepSeek do
not. A fresh command first performs a read-only durable exact-turn preflight and then checks the
matching resident slot and capability. Unsupported paths return
`provider_turn_interrupt_unsupported` without a command result, state event, effect row, room-budget
reservation, Ctrl-C, or runtime stop. A committed replay bypasses live proof, and the existing
mutation repeats every durable authority check after proof to close the race. The exact durable
`Assigned` phase alone permits a matching retained-capable runtime whose in-memory turn slot has not
yet been installed; `StartDispatching` and later phases still require the exact active turn. Focused
tests prove both sides of this scheduling boundary. The ordinary process-wide principal mutation
debit still applies to a fresh unsupported request; only durable room-write reservation is avoided.

Critical review also found a concrete write-amplification path in interrupt recovery: when a
durable recovery candidate had no exact in-memory turn control, the existing one-second lifecycle
scan could claim and immediately release it, producing two SQLite updates and commits on every
pass without changing authority. Commit `22c8df0` moves the existing read-only
`owns_exact_turn` check ahead of the durable claim in the recovery owner. A candidate with no exact
control now performs no claim write; if control disappears after that proof, the existing release
still closes the narrow race. This adds no timer, cache, retry, or fallback. The accepted trade-off
is that control installed immediately after a negative proof is observed on the next existing scan.
The focused no-control recovery test, warning-denied server Clippy, and the complete `make verify`
gate pass after this correction.

Focused tests prove that the persistence preflight is repeatable and read-only, a committed replay
is still returned, a non-capable driver retains its prepared exact turn without control I/O, and the
Codex driver passes capability proof. The real TCP/WebSocket Codex fixture still observes one
`turn/start`, one `turn/interrupt`, retained idle state, and exact ACK replay. Warning-denied Clippy
for persistence/provider/server, all focused tests, and the complete `make verify` gate pass. The
four-line capability override moves both `codex.rs` and `opencode.rs` from 799 to 803 lines. Each
override belongs to that provider's existing `ProviderDriver` implementation and introduces no
state flow or independent invariant; moving it solely to reduce LOC would add forwarding glue, so
both cohesive provider owners remain strong-warning candidates rather than being mechanically
split. At this local checkpoint the copied packaged Antigravity rejection boundary and
post-rejection turn continuity were still required; the evidence below closes only that
fail-closed boundary and does not claim Antigravity explicit-interrupt parity.

### Packaged Antigravity rejection evidence: 2026-09-01

Computer Use drove release package `AgentsAssemble Interrupt Verify 901` under isolated identifier
`app.agentsassemble.rust.interruptverify0901`. The copied Agent Add surface selected the installed
Antigravity `gemini-3.6-flash` catalog entry, Medium reasoning, the real repository workspace, and
the persistent native PTY path; no print or one-shot mode was used. A fresh Agent Session completed
two ordinary turns as `FAST_CONTINUITY_901` and `FAST_INTERRUPT_CONTINUITY_901` before the interrupt
boundary was exercised.

A third turn asked the provider to inspect the provider source tree. While the copied UI and durable
session both reported `busy`, the copied `agent.interrupt` control returned
`The provider cannot prove retained-runtime interruption for this turn.` Read-only inspection
immediately before and after that rejection showed the same guardian, anchor, and provider process,
the same active turn and provider conversation, zero `agent.interrupt` command results, and zero
`agent_interrupt` effect rows. The same process remained alive and the turn remained busy until the
existing three-minute provider inactivity deadline reported `provider_turn_timeout`; the rejection
did not send Ctrl-C, stop, replace the runtime, reserve durable room-write budget, or create durable
interrupt authority.

That deliberately overlong turn is not same-process completion evidence. Explicit UI Resume started
a new owned process for the unchanged provider conversation and completed the preserved pending
input as `LONG_CONTINUITY_901`; the input was not lost or treated as interrupted. The result proves
the fail-closed packaged boundary and post-rejection durable input continuity, while leaving native
Antigravity retained-interrupt parity unsupported and same-process completion for that timed-out
turn unclaimed. Point observations during the rejected busy turn showed the desktop, supervisor,
server, guardian, anchor, and provider near idle CPU; the provider RSS was about 268 MiB. These are
resource observations for this run, not a performance claim or optimization basis.

Both Agent Sessions were stopped through their copied product controls before normal application
quit. No verification-owned desktop, supervisor, server, guardian, anchor, or provider process
remained. The exact 50 MiB package, isolated Application Support, Caches, WebKit, preferences, and
the 49 MiB standalone Antigravity probe were moved together to a recoverable Trash directory; shared
staging roots and unrelated processes were untouched. Computer Use was then reset. Supported Codex
or OpenCode packaged busy-turn interruption and restart recovery remain required before the overall
explicit-interrupt extension is complete.

### Retained-interrupt correction final cross-review: 2026-09-01

The critical web reviewer and Daybreaker Blue High found no actionable issue and independently
approved each of `9fa3dac`, `7d06973`, `5b7dcc0`, `22c8df0`, and `1c5b37e`, cumulative
`6f9115a..1c5b37e`, and pushed HEAD `1c5b37e` at C0/H0/M0/L0. Neither reviewer ran an automated
security scan.

### Uncertain-turn recovery projection correction: 2026-09-01

The first 1440-by-900 package resumed the exact OpenCode Muse Spark contributor Agent Session and
its preserved input. OpenCode returned a provider error after external turn dispatch, so the
provider adapter correctly classified the result as effect-uncertain and retained the same
guardian, anchor, provider process, runtime owner, active turn, and one inflight input. Persistence
changed the exact execution to `recovery_required`, but the public Agent Session remained
ordinary `busy` with `recovery_required=false`. The copied UI consequently offered
`agent.interrupt`; its exact durable recheck rejected `stale_provider_turn`, and no interrupt
command result or effect row was created. The rejection was safe, but the public state and offered
control contradicted the already authoritative execution phase.

The correction makes the provider-turn execution transition, public recovery flag, bounded stable
error code/message, and one `agent_session_state` event a single SQLite transaction owned by
provider-turn persistence. The server publishes that commit while retaining the exact in-memory
turn and runtime custody. Live reconciliation now recognizes an owned, already-classified
`recovery_required` execution before consulting its retained result, avoiding a redundant
classification attempt on every existing one-second unresolved-owner pass. The copied control
disables interrupt for that state and presents the recovery requirement. The change adds no schema,
provider branch, process, task, cache, queue, poll, timer, retry, fallback, compatibility path, or
client authority. Its only added durable cost is the existing canonical state event when the rare
uncertain transition first commits; inflight input, active-turn identity, provider conversation, and
runtime handle/owner/lease remain unchanged until the existing runtime-gone recovery owner proves a
terminal transition.

The focused persistence regression proves the execution phase, public flag/error, unchanged busy
turn authority, single state event, and absence of follow-on assignment in one commit. Five server
provider-turn/reconciliation tests pass, and the copied frontend test proves the stale interrupt is
disabled while the bounded recovery notice is visible. The rebuilt isolated package then reproduced
the same real Muse failure on execution generation 3. Read-only SQLite inspection observed one
`recovery_required` transition and one matching `agent_session_state` event, while Computer Use
showed the session as busy with the bounded recovery notice and a disabled interrupt control. No
repeated classification event appeared. Normal application shutdown provided runtime-gone proof;
the existing recovery owner finalized only generation 3 as `failed/requeue_finalized=1`, cleared the
active turn and recovery flag, and detached the stopped session. A healthy supported-provider
retained interrupt remains required before this active slice can close.

### Room search header and result identity correction: 2026-09-01

The 1440-by-900 packaged comparison exposed two desktop search fields with different owners: the
channel header searched canonical messages while the right panel filtered only its current member
projection. Discord reference captures and the user's explicit UI direction selected one room search
at the stable header position, so the correction intentionally removed the member-filter state,
kept one room-named message search in a fixed 300-pixel slot, and made `all` the default. The
right-panel body retained its existing 300-pixel ownership and gained no request authority.

The later repository audit limits that evidence: current Rust `all` is a lobby-only alias, not the
original union of lobby plus custom text channels. The displayed “all readable channels” promise and
default therefore remain F-20, not room-wide completion. Until the custom-channel owner exists, a
real-client result proves only lobby search even when the request parameter is `all`.

Search results now return the canonical message actor `participant_id` and resolve the current
profile image through the existing room participant/Agent Session profile projection. Display-name
matching was rejected because names are mutable and non-unique. Missing or failed images fall back
to the bounded author initial. The wire addition reuses the already decoded canonical event and
adds no SQL column, index, join, background task, timer, polling, cache, fallback, disk projection,
or unbounded allocation. Focused persistence, provider, TCP, parser, search lifecycle, header, and
lobby tests plus the production frontend build pass. Computer Use then verified the rebuilt
1440-by-900 package with one `새 회의실 검색` field, no `general` or right-panel member-search
duplicate, and a stable search/toggle header position before and after closing the member body. A
real `INTERRUPT` query returned ten canonical lobby results through the `all` alias with a circular avatar
surface for every result. The profile-image branch is covered by a canonical-participant fixture;
this isolated room's current profiles intentionally use initial fallbacks where no SSoT image is
set.

A follow-up visual pass found that the member panel's root border still crossed the 48-pixel room
header, making the single header look like two adjacent menu bars. The copied CSS now keeps the
member root borderless and assigns the same one-pixel separator only to the tabs and their active
tabpanel below the header. This changes no width, breakpoint, state, event, or input owner. The CSS
integrity gate was updated to the one expected production chunk and passed. In the rebuilt isolated
package, the whole accessibility tree contained exactly one room search with the member panel both
open and closed; the channel title, alert, pin, member toggle, and search stayed on one continuous
header, while the vertical separator began below it and disappeared with the panel body.

The exact app and owned sidecar were quit normally; its isolated Application Support directory,
current package, two obsolete package copies, and generated DMG were then removed, reclaiming about
217 MiB. No unrelated application, provider, or user data was touched.

The first packaged attempt failed strict response parsing because the frontend and desktop had been
rebuilt while the bundled server sidecar still emitted the previous result contract. No fallback or
lenient parser was added. Rebuilding the release server sidecar and repackaging made the same query
pass, confirming the build-input mismatch as the root cause. The repository already has one
sidecar-preparation owner in `desktop/scripts/prepare_sidecar.mjs`; subsequent ordinary packages use
`npm --prefix desktop run build`, and isolated-identifier packages run that same release-sidecar
preparation immediately before Tauri's configured build. No second build path or compatibility
check was added.

### D-01 HTTP host bootstrap removal: 2026-09-01

Repository-wide caller search found no production consumer of the 30-second host-challenge map,
HMAC exchange, HTTP ticket route, or startup-secret preamble. Desktop already owns ticket issuance
through its private anonymous control pipe, and admitted humans already use their authenticated
session exchange. Commit `a7949bd` therefore deletes that second authority instead of replacing it.
The preserved invariants are one-use room tickets, exact room and participant validation, private
control EOF shutdown, admitted-human issuance, loopback ingress checks, and bounded socket admission.

The removed path eliminates one mutex/map lifecycle, challenge expiry state, HMAC work, startup
secret generation and transfer, and two private HTTP routes. This is evidence-based authority and
resource removal, not a latency claim; no fallback, compatibility shim, timer, retry, cache, or new
abstraction was added. Full server tests (93 unit plus all integration suites), 25 desktop tests,
warning-denied server and desktop Clippy, formatting, architecture/source-growth gates, and diff
checks passed. The runtime TCP boundary additionally proves both retired routes return 404, while
control-pipe tests prove local issuance and EOF-owned shutdown still work.

### D-02 subscription receipt proof removal: 2026-09-01

The receipt challenge, receipt HMAC, snapshot-byte digest, and duplicate capability digest had no
controlled endpoint-substitution reproduction or independent remote trust boundary. Commit
`3ffb9eb` removes those four values and their server, browser, generated-schema, and test owners.
It keeps the one-use ticket, exact room/participant/product-surface checks, finite snapshot cursor
and catch-up high water, sequence validation, request replay, strict receipt keys, and bounded
WebSocket admission. The remaining frame HMAC/base64/counter path is explicitly still active and
belongs to the next D-02 change; no fallback, compatibility protocol, timer, cache, or new
abstraction was introduced.

The removed receipt avoids one browser challenge, one server HMAC, browser HMAC verification,
snapshot hashing, capability hashing, and their transient buffers per connection. This records
eliminated work rather than claiming representative latency improvement. Full server tests (92
unit plus all integration suites), 657 frontend tests and production build, 25 desktop tests,
protocol tests, warning-denied workspace Clippy, formatting, architecture/source-growth gates,
and diff checks passed. Actual TCP/WebSocket tests still prove exact private-ticket scope, finite
snapshot/catch-up readiness, binary-frame rejection, connection limits, replay recovery, and
message-size enforcement.

### D-02 bounded JSON frame and proof-state removal: 2026-09-01

The prior product-frame owner HMAC-signed, base64-expanded, copied, and asynchronously
serialized every post-subscription frame, maintained two direction counters and browser
promise queues, and generated a second 64-hex random secret for every ticket grant,
including HTTP tickets that never consumed it. No packaged endpoint-substitution or
active-relay reproduction established a threat that survived the existing exact-child,
one-use-ticket, origin, ingress, and TLS boundaries. This was concrete recurring work and
state without a demonstrated security owner, not an optimization based on expected future
load.

Commit `77cae0e` moves both directions to the protocol-owned 256 KiB strict JSON
boundary consumed by `room_channel` and the generated frontend constant. It measures
incoming browser frames by UTF-8 bytes and rejects the retired authenticated envelope
before dispatch; a real TCP test wraps a valid message command in the old canonical
base64 envelope with a structurally valid nonmatching proof, then distinguishes the
current `frame_schema_invalid` failure from the retired decoder's authentication failure
and proves no durable mutation occurs. Commits `0d24741` and `57fd6ec`
remove the HMAC/base64 modules, connection nonce, proof-key ticket storage and wire fields,
duplicate browser queues/counters, and obsolete proof-oriented test harness. This halves
the random UUID pairs generated per ticket and removes the roughly one-third base64 wire
expansion without adding a fallback, legacy decoder, compatibility branch, polling loop,
cache, or speculative abstraction.

Preserved product and security contracts are one-use ticket consumption, exact room and
participant authority, product-surface equality, TLS/origin and ingress admission, strict
schema and byte bounds, finite `C/H` Snapshot/catch-up readiness, contiguous event sequence,
request-ID replay, uncertain-ACK recovery, revocation revalidation, and the three-minute
quiet-room keepalive required by the server's five-minute idle-input deadline. The accepted
trade-off is the absence of a separate application-layer active-relay authenticator where no
independent authority or reproduced attacker exists.

Verification passed all 90 server unit tests and every real TCP/WebSocket/HTTP integration
suite, 650 frontend tests and the production CSS-verified build, 25 desktop tests, generated
protocol export, warning-denied server and desktop Clippy, Rust formatting, diff checks, and
the architecture/source-growth policy gate. Repository-wide current-source search found no
remaining proof key, connection nonce, authenticated envelope, frame-HMAC module, or obsolete
proof-oriented test peer.

### D-03 direct remote profile authorization: 2026-09-01

Before `8d0c9f5`, every remote profile read, patch, or avatar upload first authorized the reusable
session in SQLite, allocated a short-lived profile grant under the shared ticket-store lock, returned
it through an extra HTTP response, and then consumed and durably revalidated it at the target. The
exchange did not create an independent trust boundary: both requests used the same HTTPS origin and
an injected script able to use the session could also mint the grant.

The profile target now classifies the canonical `aas1.` bearer before any local-ticket lookup,
resolves it once from durable session authority, and performs the existing operation-specific
revalidation at the owning profile or attachment transaction. A malformed session-shaped bearer
fails as unauthorized and never falls through to local authority. The retired profile exchange
returns 404 at the real TCP boundary. Local desktop profile tickets, pre-join avatar credentials,
public bound-avatar reads, read-only profile patch and avatar-upload denial, no-store responses,
and the separate one-use WebSocket exchange remain unchanged.

This moves the initial durable session resolution into the target request and removes one HTTP round
trip plus one ticket-map insertion/consumption per remote profile operation. It adds no cache, timer,
retry, fallback, compatibility path, or new durable state; no latency claim is made beyond the removed
operations. All 90 server
unit tests, the real invite/profile TCP suite, 651 frontend tests, the production CSS-verified build,
warning-denied server Clippy, formatting, and the architecture/source-growth/policy gates passed.

### D-03 direct remote preference authorization: 2026-09-01

Commit `1333c5c` removes the remote preference-read and preference-write exchange routes and grants.
The target distinguishes canonical session bearers from local one-use tickets before lookup, resolves
the remote session once, and keeps preference read/write revalidation in the owning SQLite unit. The
write target now owns the read-only denial that the deleted exchange previously enforced.

Cross-room requests fail without consuming a reusable session, session replacement invalidates the
old bearer, malformed session-shaped values never fall through to local tickets, and both retired
exchange routes return 404 at the real TCP boundary. Local desktop read/write tickets, auth-before-body
for both authority kinds, no-store responses, bounded bodies, and room-global WebSocket ownership are
unchanged. The change moves initial durable session resolution into the target request and removes
one HTTP round trip plus one ticket-map insertion/consumption per remote preference operation without
adding state, fallback, polling, retry, or compatibility handling.

All 90 server unit tests, the real remote preference TCP suite, 651 frontend tests, the production
CSS-verified build, warning-denied server Clippy, formatting, and the architecture/source-growth/policy
gates passed.

### D-03 direct remote message-pin authorization: 2026-09-01

Before `ed9720c`, every remote pin list or mutation first authorized the reusable room
session in SQLite, allocated a short-lived read/write grant under the shared ticket-store
lock, returned it through an extra HTTP response, and then consumed and durably revalidated
it at `/api/room-pins`. Both requests crossed the same HTTPS authority, so the second
credential did not restrict an actor already able to present the session.

The pin target now classifies the canonical `aas1.` bearer before local-ticket lookup,
resolves its exact durable room principal, and leaves current history/mutation permission
and session revalidation with the owning persistence operation. Local desktop read/write
tickets remain exact-purpose and one-use. Read-only sessions can list pins but fail a
mutation before its malformed body is parsed; both retired public pin-exchange routes
return 404 at the real TCP boundary.

This moves initial durable session resolution into the target request and removes one
HTTP round trip plus one ticket-map insertion/consumption per remote pin operation. It
adds no state, cache, timer, polling, retry, fallback, or compatibility path; no latency
claim is made beyond the removed operations. All 90 server unit tests, the four-test real human-invite TCP
suite, the three-test message-pin TCP suite, nine focused and 650 full frontend tests,
the production CSS-verified build, warning-denied server Clippy, formatting, and the
architecture/source-growth/policy gates passed.

### D-03 local socket/profile authority separation: 2026-09-01

Repository-wide authority tracing found that the private-control `IssueTicket`
credential used by `/ws?ticket=...` was also interpreted as local profile HTTP
authority. The copied frontend and control protocol already use a distinct
`IssueOperatorHttpTicket`/`runtime_operator_ticket` for local profile operations, so
the second interpretation had no current product caller and violated the documented
wrong-transport/scope consume-and-reject contract. An unused socket credential could
therefore have read or durably changed the server-wide profile before WebSocket use.

Commit `f5ddb91` rejects `TicketAuthority::Room` in the profile/attachment dispatcher
and removes the corresponding profile authority and persistence branches. It adds no
replacement credential, fallback, compatibility path, state, timer, polling, retry,
or new abstraction. Local profile operations retain the one-use server-operator
credential; remote profile operations retain direct reusable session authorization;
message/appearance uploads and WebSocket admission retain their exact authorities.

All 90 server unit tests, the three-test profile TCP suite, the four-test human-invite
TCP suite, warning-denied server Clippy, formatting, and the architecture,
source-growth, and policy gates passed. The profile TCP suite also proves that a socket
ticket presented to profile HTTP receives 401 and is consumed before a later WebSocket
attempt, while the positive local profile flow uses the server-operator ticket.

### D-03 direct remote message-search authorization: 2026-09-01

Before `ae6fe7a`, each remote search or context read first resolved the reusable room
session, allocated a short-lived `MessageSearchRead` grant under the shared ticket-map
lock, returned it through a separate HTTP response, and then consumed and revalidated
the same session authority at the target. Both requests used the same HTTPS origin, so
that second credential did not establish a distinct trust boundary.

The search target now classifies canonical `aas1.` bearers before local-ticket lookup,
resolves the exact durable room principal, and leaves current `room.history`
revalidation inside the owning search or context persistence transaction. Local desktop
search tickets remain exact-purpose and one-use. A malformed session-shaped credential
does not fall through to local tickets, and the retired remote exchange route returns
404. The frontend sends the reusable remote session directly and retains its strict
bounded response parser, request invalidation, no-store requirement, custom-channel
rejection, and no-fallback behavior.

The change removes one HTTP round trip and one ticket-map insertion/consumption per
remote search operation, plus the now-unused mixed local/session ticket resolver and
generic frontend exchange helper. It adds no state, cache, process, timer, polling,
heartbeat, retry, fallback, compatibility path, or speculative abstraction; no latency
claim is made beyond the removed operations. The four-test real message-search TCP
suite, 13 focused ticket-store tests, 15 focused frontend HTTP/search tests, the
production CSS-verified frontend build, warning-denied server Clippy, Rust formatting,
and the architecture/source-growth/policy gates passed.

### D-03 direct remote message-attachment authorization: 2026-09-01

Before `e0622cb`, each remote upload or bound read first resolved the reusable room
session, allocated a short-lived attachment grant under the shared ticket-map lock,
returned it through a separate HTTP response, and then consumed and revalidated the
same session authority at the target. Both requests used the same HTTPS origin, so the
second credential established no distinct trust boundary.

The remote browser now sends its durable session directly to the target. A dedicated
`POST /api/message-attachments` authenticates the exact upload operation before reading
the bounded base64 body, then the persistence transaction revalidates current writable
room authority before storing bytes. This separate route is required because the shared
profile/pre-join/appearance POST cannot infer message-upload authority from a session
before parsing a caller-supplied purpose. The existing shared
`GET /api/attachments/{id}` continues to dispatch the canonical `ma_` namespace once
and revalidates current `room.history` plus exact visible-message binding before loading
the BLOB. Local desktop upload and asset-bound read tickets remain exact-purpose and
one-use. Malformed session-shaped bearers never fall through to local tickets, the two
remote exchange routes return 404, the old generic POST rejects a message ticket before
body admission, and the new strict body rejects the retired `room_attachment` payload.

The change removes one HTTP round trip and one ticket-map insertion/consumption per
remote transfer, the two public session-grant variants, and an unused frontend
`room_attachment` compatibility branch. It adds no durable state, cache, timer, polling,
heartbeat, retry, fallback, or generic transport abstraction. The 728-line shared
profile-and-asset HTTP adapter was reviewed at the 500-line structure warning:
extracting only this branch would add shared error/CORS/response interfaces while the
canonical ID dispatcher still requires one owner, so no line-count-only split was
made. No latency or memory claim is made beyond the removed operations and grant state.

The two-test real TCP attachment suite covers local one-use consumption, crossed and
retired routes, strict payload rejection, malformed-session auth-before-body, direct
writable remote upload/read reuse, exact message deletion, read-only upload denial,
and post-leave revocation. Nine real control-pipe tests, 12 focused ticket-store tests,
12 focused frontend attachment/profile tests, the production CSS-verified frontend
build, warning-denied server Clippy, Rust formatting, diff checks, and the
architecture/source-growth/policy gates passed. No provider or packaged frontend was
started because this correction changes only the already-verified human HTTP authority
hop. The later cumulative D-03 review closed at `5693e13`.

### D-03 direct remote room-appearance authorization: 2026-09-01

Before `9bfee34`, each remote bound-appearance read first resolved the reusable room
session, allocated a short-lived exact-asset grant under the shared ticket-map lock,
returned it through a separate HTTP response, and then consumed and revalidated the
same session at the asset target. Both requests used the same HTTPS origin, so the
second credential established no distinct trust boundary.

The shared `GET /api/attachments/{id}` target now classifies a durable human session
directly and accepts it only for the canonical `ra_` asset namespace and exact `view=1`
query. The owning persistence transaction verifies the current session, joined room
membership, exact room appearance reference, metadata, and size before loading the
BLOB. Read-only remote members retain read authority; leave or revocation fails closed.
Malformed session-shaped credentials never fall through to local tickets, malformed
reserved `ra_` identifiers never fall through to profile assets, and the retired
exchange route returns 404. Local pending-preview and bound-read tickets remain exact,
one-use authorities crossing the desktop private-control boundary; a remote session
cannot read pending assets.

The correction removes one HTTP round trip and one ticket-map insertion/consumption per
remote read, the public appearance-grant variant and generic purpose state, and the now
unused frontend session-ticket parser. The remaining human-session ticket map is named
and typed only for the WebSocket upgrade it owns. It adds no durable state, cache,
process, timer, polling, heartbeat, retry, fallback, compatibility path, or speculative
abstraction. No latency or memory claim is made beyond the removed operations and grant
state. The 749-line shared profile-and-asset HTTP adapter was reviewed at the 500-line
structure warning: extracting only the appearance branch would increase shared
error/CORS/response interfaces while canonical asset-ID dispatch and authentication
remain one cohesive owner, so no line-count-only split was made.

The real TCP appearance suite covers direct reusable reads, target-room denial, local
one-use consumption, retired exchange rejection, malformed-session fail-closed behavior,
reserved-ID dispatch, and post-leave revocation. Twelve focused ticket-store tests, one
focused profile appearance test, 18 focused frontend appearance/hook tests, the
production CSS-verified frontend build, warning-denied server Clippy, Rust formatting,
diff checks, and the architecture/source-growth/policy gates passed. No provider or
packaged frontend was started because this correction changes only the already-verified
human HTTP authority hop. The later cumulative D-03 review closed at `5693e13`.

### F-04 executable capability and frontend authority correction: 2026-09-01

Before `1a87de8`, the signed snapshot advertised four capabilities with no matching
registered action. The strict client rejected the copied room-delete,
participant-kick, and provider-response commands before send, so the observed threat
was not an authorization bypass but a signed false product promise and permanently
unusable UI. The capability owner now omits `room.delete`, `participant.kick`,
`provider.request.resolve`, and `bridge.report`; it retains `bridge.publish` because
the current vote authorization path uses it.

Commits `5842f8a` and `e711def` remove three unusable control paths and their dead
command, state, model, test, and presentation branches. First pushed cross-review
found that `agent.readd` still had a reachable selector and locally rejected command,
while provider requests, kicked events, and room deletion retained state or callbacks
without a production owner. `cccf513` removes that selector/command/ACK path, the
always-empty provider-request snapshot and frontend wire vocabulary, the producerless
kicked-event projection, and the never-invoked room-delete callback chain. The current
server kicked-participant start denial and OpenCode's interactive-request fail-closed
test remain because they enforce reachable failure contracts rather than UI authority.
Repository-wide searches found no remaining frontend caller for the absent commands.
No route, placeholder, fallback, compatibility branch, retry, timer, polling,
heartbeat, or generic feature abstraction was added.

The observed production bundle changed from 791.52 kB to 784.33 kB minified
JavaScript (239.04 kB to 237.09 kB gzip) and from 169.18 kB to 165.72 kB CSS
(29.60 kB to 29.06 kB gzip). These are reductions of 7.19/1.95 kB JavaScript and
3.46/0.54 kB CSS, not a CPU or memory benchmark. The owning-boundary intent was to
remove unreachable React normalization and state updates rather than introduce a
second capability framework. The exact product/security invariant preserved is that
only a registered action can be advertised or rendered as executable, while current
vote authorization and server-originated lifecycle/read data remain intact.

The focused domain serialization test, domain/protocol warning-denied Clippy, and two
production frontend builds passed before this documentation record. One first full
frontend run exposed that the exact-command retry test depended on `vi.waitFor` to
advance the final half of a documented 2,000 ms backoff at its timeout boundary. The
test now advances the owned retry delay explicitly; this changes no production timer
or retry policy. The pre-review candidate passed a fresh complete `make verify`:
architecture/source-growth/policy/diff gates, Rust format/check, the CSS-verified
production frontend build, all 103 frontend files and 646 tests, the desktop build,
warning-denied Clippy and 25 tests, and the full Rust workspace test and
warning-denied Clippy suites. After `cccf513`, 55 focused frontend tests and four
focused domain tests passed, followed by a fresh complete `make verify` on the
documented correction: all gates above, 103 frontend files and 643 tests, desktop
Clippy and 25 tests, and the full Rust workspace tests and warning-denied Clippy.
Critical ChatGPT Pro approved exact `5b1b331..fe8ffbf`, cumulative
`dd1e99d..fe8ffbf`, and HEAD `fe8ffbf` at `C0/H0/M0/L0`. Daybreaker's same review
found one Low: these verification instructions and two server rejections still
presented removed re-add as an available action. `d874d92` makes the rejection text
authority-neutral, and the active verification contract now defers re-add to its
complete participant/session transition. Focused verification for that correction
passed all 242 persistence tests and warning-denied persistence Clippy. Exact
`fe8ffbf..7b2168f`, cumulative `dd1e99d..7b2168f`, and HEAD `7b2168f` then received
`APPROVE — C0/H0/M0/L0` from both Critical ChatGPT Pro and Daybreaker Blue High.

### F-05 absent-service frontend exposure, first batch: 2026-09-01

The observed pre-change costs were concrete: normal authenticated startup mounted
`useFriendsDirectory` against absent `/api/room-friends`; every active room mounted
`useRoomSideChat` against absent `/api/side-chat`; and selecting a copied custom
channel mounted a 2.5-second absent-message poll plus deferred five-second voice
presence polling and a 20-second join heartbeat. Permanent 404s and unsupported voice
state are not transient failures, so adding retries, dummy routes, or fallback data
would preserve the wrong owner.

Commits `f687667`, `eecdb20`, and `d058af8` remove those three active composition
boundaries independently. Startup now uses the implemented lobby. The right panel
keeps one room-connection-information control and content owner. Custom-channel
creation, selection, and lazy rendering are absent, so `CustomChannelView` is outside
the production import graph. No backend route, fake state, compatibility branch,
retry, timer, heartbeat, or feature framework was introduced. Dormant copied hooks and
components, external-AI/AI-friend/operator-pairing/companion controls, and public
Google controls are not claimed complete by this batch.

The production build regrouped its chunks after removing the custom-channel lazy
boundary. Total emitted JavaScript changed from 871.84 kB to 854.80 kB minified and
from 265.89 kB to 256.70 kB gzip; CSS changed from 169.71 kB to 169.23 kB minified and
from 30.42 kB to 29.63 kB gzip. These are bundle observations, not CPU, memory, or
runtime-latency claims. The CSS provenance gate pins the new single production CSS
artifact and its exact hash rather than weakening the comparison.

A fresh complete `make verify` passed: architecture, source-growth, policy, and diff
gates; Rust formatting and checks; the exact-CSS production frontend build; all 103
frontend files and 644 tests; desktop preparation/build, warning-denied Clippy, and
tests; all Rust workspace tests including real TCP boundary suites; and workspace
warning-denied Clippy. The exact pushed range then entered Critical ChatGPT Pro and
Daybreaker Blue High manual review.

Initial cross-review differed. Critical ChatGPT Pro found no actionable issue and
approved each commit, exact `8903445..cf71db4`, and HEAD `cf71db4` at
`C0/H0/M0/L0`. Daybreaker returned three Low findings: reachable mobile side-chat
mode state and branches remained although no caller supplied content; canonical room
socket types and projection retained a callback with no decoder/producer; and active
message search retained a permanently-false custom-channel routing branch.

Commit `a2b2f41` removes those three owners rather than adding feature flags or future
abstractions. Mobile room information now has only its current information state,
the producerless canonical side-chat callback is absent, and search maps its two
current scopes directly to `all` or `lobby`. The correction production build passed
with unchanged exact CSS, 853.72 kB total emitted JavaScript minified and 256.36 kB
gzip after chunk regrouping; all 103 frontend files and 644 tests passed, followed by
a fresh complete `make verify`: architecture/source-growth/policy/diff gates, Rust
format/check, the CSS-verified production build, desktop build/Clippy/25 tests, all
Rust workspace tests including TCP boundary suites, and workspace warning-denied
Clippy. Correction re-review is pending.

The next manual pass confirmed those original three findings closed. It found two
additional Low source remnants: `RoomSocketHandlers` still declared producerless
`onLobby`/`onRoster` callbacks, and the app controller still exported raw mobile
setters, viewport state, a members setter, and a computed viewer label with no
consumer. Critical ChatGPT Pro separately found one Low documentation overclaim: the
record said every canonical callback had a producer even though the deferred plugin
bridge remains source-only. That finding does not authorize moving the user-deferred
RimWorld/plugin scope forward.

Commit `87d3d0d` removes only the two producerless handler fields and unused public
projections/computation while retaining internal mobile gestures, `toggleMembers`,
current room-event decoding, and the deferred plugin source. This record now names
only the exact removed side-chat callback. The final candidate production build emits
853.34 kB total JavaScript minified and 256.30 kB gzip with unchanged exact CSS. A
fresh complete `make verify` passes the architecture/source-growth/policy/diff gates,
Rust format/check, CSS-verified production build, all 103 frontend files and 644
tests, desktop build/Clippy/25 tests, all Rust workspace and TCP boundary tests, and
workspace warning-denied Clippy. Critical ChatGPT Pro and Daybreaker Blue High each
found no remaining actionable issue and approved exact `778d761..f74af57`, cumulative
F-05 `8903445..f74af57`, and HEAD `f74af57` at `C0/H0/M0/L0`.

### F-05 absent external-admission exposure, second batch: 2026-09-01

The reachable original exposes three distinct flows whose complete Rust owners do not
yet exist: Room Connector admission for an already-running external AI session,
operator-pairing issuance, and an admitted human's AgentBridge companion packet. The
copied frontend instead sent the first two through the obsolete moderator/host-token
client and sent the companion to an absent HTTP route. Widening the implemented human
invite owner or adding placeholder routes would merge distinct principals and failure
contracts.

Commit `762ba40` removes the Room Connector card, installation instructions, and its
controller state/callbacks. Commit `11e167b` removes the operator-pairing issuer UI,
state, callback, response type, and client call while preserving the separate incoming
pairing redemption state machine. Commit `7159c2d` removes the guest companion card,
packet state, client call, and its now-unreferenced duplicate clipboard helper. Human
invite creation/revocation, managed public-ingress start/stop, room membership, and
ordinary member/Agent Session presentation remain unchanged. Dormant AI-friend source
and public Google controls are not claimed complete.

The prior reviewed candidate emitted 853.34 kB JavaScript minified/256.30 kB gzip and
169.23/29.63 kB CSS. The second candidate's emitted 5.18/2.15 kB `AdminPanel` chunk and
839.15/251.87 kB main chunk total 844.34 kB after summing raw chunk bytes and rounding
the aggregate; its displayed per-chunk gzip figures total 254.02 kB. CSS is
169.13/29.62 kB. This is a measured 9.00 kB minified and 2.28 kB gzip JavaScript
reduction.
This batch removes user-triggered failing requests and unused React state; it makes no
steady-state CPU, memory, or latency claim because these controls did not poll. The CSS
gate pins the changed single production artifact and exact SHA-256 after the pairing
panel styles leave the emitted cascade.

Focused controller/modal verification passed 22 tests after Room Connector and pairing
removal; focused room-connection verification passed 18 tests after companion removal.
A fresh complete `make verify` passes architecture/source-growth/policy/diff gates,
Rust format/check, the exact-CSS production build, all 103 frontend files and 643 tests,
desktop build/warning-denied Clippy/25 tests, the complete Rust workspace including real
TCP boundary tests, and workspace warning-denied Clippy. Both manual reviewers found one
Low documentation error: the first record counted only the main JavaScript chunk as the
emitted total. Corrections through `7f2e878` record the raw-byte aggregate and displayed
per-chunk gzip sum separately. Critical ChatGPT Pro and Daybreaker Blue High each
approved correction `9759d73..7f2e878`, the original batch as corrected, cumulative
F-05 `8903445..7f2e878`, and HEAD `7f2e878` at `C0/H0/M0/L0`.
