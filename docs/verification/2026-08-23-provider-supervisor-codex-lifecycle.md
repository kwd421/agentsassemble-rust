# Provider supervisor and Codex lifecycle verification — 2026-08-23

Scope: recoverable lifecycle command authority, startup reconciliation, the provider-neutral runtime adapter, and Codex `app-server --stdio` process start/reuse/stop. This bundle does not yet implement Codex thread attachment or turns, Antigravity PTY/ConPTY, OpenCode HTTP/SSE, or the final three-provider live matrix.

## Automated evidence

- `make verify`: passed after the final recovery-authority hardening, including architecture and 800-line source gates, generated TypeScript, React build and 27 tests, Tauri checks and 8 tests, 47 persistence tests, 20 provider tests, server integration tests, warning-denied Clippy, doc tests, and diff whitespace checks.
- Provider regressions atomically replace a selected executable after it is verified and prove that only the bound verified bytes execute. They also prove that stop kills descendants after a Codex leader exits and that cancellation during initialization leaves the process tree owned for exact adapter shutdown.
- The lifecycle provider test launches an owned mock Codex executable, verifies the initialize/initialized JSONL handshake, proves exact runtime reuse, mutates the selected executable while the child remains alive to prove an uncertain exact-handle failure, then stops and reaps only that runtime.
- The real TCP/WebSocket integration creates a Codex Agent Session, starts the owned mock app-server before committing the ACK, proves reuse, confirms stop without repeating it, starts a new generation, shuts the server down, and verifies no mock runtime remains. Reopening the database reconciles the formerly live session before its snapshot is admitted.
- Persistence recovery tests prove exact candidate CAS, no repeated stop after `effect_applied`, `Gone` stop finalization, ambiguous-start lease retention, owner-loss terminalization, rejection of competing pending lifecycle authority, and atomic removal of the pending reservation when a pre-effect start fails safely. An ambiguous start with an exact lease preserves its handle/owner and only the same operation may retry it; an ambiguous pre-effect start without a handle cannot spawn again until absence is proved.
- The full workspace and the Tauri shell compiled for `x86_64-pc-windows-gnu` with all targets/features and warnings denied using the stable rustup compiler. Isolated cross-check target directories were moved to Trash.

## Process and authority boundaries

- One common `ProviderAdapter` owns per-session slots, a supervisor identity, exact handle/profile correlation, confirmed-stop tombstones, and bounded shutdown. Provider drivers report transport facts only.
- Codex binds verification to process creation: Linux and other non-macOS Unix targets execute the verified open file descriptor, macOS executes a byte-verified copy held in a private staging directory for the runtime lifetime, and Windows denies write/delete sharing while the selected image is held open. It also uses a credential-free environment allowlist, an owned Unix process group or Windows Job Object, bounded JSONL and stderr drains, and default-denies provider-initiated requests.
- Once process creation succeeds, initialization and health failures remain uncertain with their exact handle/owner until a whole-tree stop is confirmed; a dead Unix leader is never treated as proof that its process group is gone.
- Startup observation revalidates executable and workspace authority before `Adopted`. An exact owned runtime with uncertain health or filesystem authority becomes `LeaseUncertain`, which persistence commits as disconnected/recovery-required without discarding its handle, owner, or start intent. Runtime adoption cannot claim a provider conversation active; that will require the separate provider-session checkpoint.
- Startup closes the persistence read transaction before driver observation, then reloads the complete session and reservation set and applies only an exact-CAS observation before network admission.
- Antigravity and OpenCode return a visible unavailable error in this slice; they do not fall back to print, exec, Python, or another provider.

## Cleanup

- Verification-owned mock Codex processes were reaped by their adapter/server owner.
- No user-owned Codex, original-project, desktop-app, or provider process was signalled or modified.
- Isolated Windows check targets were moved to recoverable Trash locations.
