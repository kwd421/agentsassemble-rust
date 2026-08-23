# Verification Contract

Status: current real-client verification owner

## Scope

Verification claims only the boundary actually observed. Build, lint, unit tests, simulated sockets, responsive browser emulation, and real provider runs are separate evidence classes and cannot substitute for one another.

The active comparison baseline is original
`d5046473010d1353a81ee38337360e6d98f7bd6f` and public Rust
`3d0257532d7cb706b349fb11b15ca7709e1672b3`. Local uncommitted behavior
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

## Real Agent matrix

Frontend flows that require real Agent Sessions use exactly this matrix:

- Codex: Terra;
- Antigravity: Flash;
- OpenCode: the free Hy3 model.

The verification records the exact provider/model identifiers exposed by the installed runtime at execution time. Missing login, unavailable capability, unsupported model, or provider failure remains visible as failed or `unknown`; it never triggers model substitution, a mock pass, or a fallback provider.

Provider credentials, private conversation state, hidden reasoning, and provider-private identifiers are excluded from screenshots, logs, fixtures, public events, and committed artifacts.

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

## Published Windows transport and helper-binding evidence: `3d02575`

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

This is Windows OS transport evidence, not a claim that an authenticated
Antigravity Flash account or the copied desktop UI was exercised on Windows. The
published macOS real-client matrix above remains the provider/model evidence; a
future Windows real-provider run must be recorded separately rather than inferred
from the ConPTY fixture.

## API verification scope

When a reachable flow specifically needs an API-backed provider, the allowed paid/provider-specific candidates are the official DeepSeek API and the designated Flash provider path. Every other API-backed verification uses only an explicitly free API or free model. Missing credentials, exhausted free quota, or unavailable models fail visibly; they do not trigger a paid substitution or a fallback provider.
