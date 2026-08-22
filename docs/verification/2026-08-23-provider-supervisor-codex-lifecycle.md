# Provider supervisor and Codex lifecycle verification — 2026-08-23

Scope: recoverable lifecycle command authority, startup reconciliation, the provider-neutral runtime adapter, and Codex `app-server --stdio` process start/reuse/stop. This bundle does not yet implement Codex thread attachment or turns, Antigravity PTY/ConPTY, OpenCode HTTP/SSE, or the final three-provider live matrix.

## Automated evidence

- `make verify`: passed after the final recovery-authority hardening, including architecture and 800-line source gates, generated TypeScript, React build and 27 tests, Tauri checks and 8 tests, 47 persistence tests, 24 provider tests, 9 server unit tests, 13 server boundary/integration tests, warning-denied Clippy, doc tests, and diff whitespace checks.
- Provider regressions overwrite the selected inode and then atomically replace its path after verification, proving that only the bound verified bytes execute. They also prove aggregate notification-byte limits, guardian/anchor cleanup after a Codex leader exits, fresh-adapter lease observation, and cancellation during initialization without abandoned process custody.
- The lifecycle provider test launches an owned mock Codex executable, verifies the initialize/initialized JSONL handshake, proves exact runtime reuse, mutates the selected executable while the child remains alive to prove an uncertain exact-handle failure, then stops and reaps only that runtime.
- The real TCP/WebSocket integration creates a Codex Agent Session, starts the owned mock app-server before committing the ACK, proves reuse, confirms stop without repeating it, starts a new generation, shuts the server down, and verifies no mock runtime remains. A cancellation regression aborts a still-initializing room task, proves server shutdown checkpoints the confirmed runtime absence into SQLite, then reopens the database and resumes the original request without a duplicate provider effect. Reopening a formerly live database reconciles its candidate before any snapshot is admitted.
- Persistence recovery tests prove exact candidate CAS, no repeated stop after `effect_applied`, `Gone` stop finalization, ambiguous-start lease retention, owner-loss terminalization, rejection of competing pending lifecycle authority, and atomic removal of the pending reservation when a pre-effect start fails safely. An ambiguous start with an exact lease preserves its handle/owner and only the same operation may retry it; an ambiguous pre-effect start without a handle cannot spawn again until absence is proved.
- The full workspace and the Tauri shell compiled for `x86_64-pc-windows-gnu` with all targets/features and warnings denied using the stable rustup compiler. The provider crate also type-checked for `aarch64-linux-android`, exercising the sealed-`memfd` launch path. Isolated cross-check target directories were moved to Trash.

## Process and authority boundaries

- One common `ProviderAdapter` owns per-session slots, a supervisor identity, exact handle/profile correlation, confirmed-stop tombstones, and bounded shutdown. Provider drivers report transport facts only.
- Codex binds verification to process creation: Linux/Android copy verified bytes into an executable `memfd` sealed against mutation, macOS and other Unix targets execute a byte-verified `0500` copy held in a private `0700` staging directory for the runtime lifetime, and Windows denies write/delete sharing while the selected image is held open. It also uses a credential-free environment allowlist, a Unix guardian/anchor group or Windows Job Object, bounded JSONL/stderr drains and a 2 MiB aggregate notification budget, and default-denies provider-initiated requests.
- Each current-profile runtime creates an exact room/session lease before provider spawn. The Unix anchor keeps both the lease and process-group identity alive after the provider leader exits; guardian/server death closes its control pipe and kills the whole anchored group. Once process creation succeeds, initialization and health failures remain uncertain with their exact handle/owner until the anchored tree or Job Object is confirmed stopped.
- Startup observation revalidates executable and workspace authority before `Adopted`. A fresh adapter uses the exact lease rather than a leader PID to distinguish active, gone, and unknown custody; an exact owned runtime with uncertain health or filesystem authority becomes `LeaseUncertain`, which persistence commits as disconnected/recovery-required without discarding its handle, owner, or start intent. Runtime adoption cannot claim a provider conversation active; that will require the separate provider-session checkpoint.
- The local executable-race tests cover application and updater races under a trusted OS account. A malicious process already running as the same account is outside the threat boundary; this is especially relevant on macOS, which lacks Linux-style sealed descriptor execution.
- Startup closes the persistence read transaction before driver observation, then reloads the complete session and reservation set and applies only an exact-CAS observation before network admission.
- Antigravity and OpenCode return a visible unavailable error in this slice; they do not fall back to print, exec, Python, or another provider.

## Cleanup

- Verification-owned mock Codex processes were reaped by their adapter/server owner.
- Verification-created runtime lease files were removed after their durable absence checkpoints; the private empty lease directory may remain for reuse.
- No user-owned Codex, original-project, desktop-app, or provider process was signalled or modified.
- Isolated Windows check targets were moved to recoverable Trash locations.
