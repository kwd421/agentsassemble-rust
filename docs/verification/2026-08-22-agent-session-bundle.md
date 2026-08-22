# Agent Session first-bundle verification — 2026-08-22

Scope: live provider catalog plus creation and restart recovery of one durable stopped Agent Session. This is not evidence for provider conversation lifecycle or the three-provider conversation matrix.

## Observed product flow

- A separately built packaged macOS Tauri app connected to its owned Rust sidecar and showed `Codex`, `Antigravity`, and `OpenCode` as `ready`.
- Live CLI discovery exposed the required exact model identifiers: `gpt-5.6-terra`, `gemini-3.6-flash`, and `opencode/hy3-free`.
- The full initial snapshot carrying the live catalog encoded to 17,743 bytes, below the 256 KiB WebSocket message ceiling, and had a nonempty `provider-catalog-v1-*` revision.
- Computer Use selected `gpt-5.6-terra` with the repository workspace, created a stopped `Terra` Agent Session, and observed both the stopped roster entry and canonical `agent_session_created` event.
- After the packaged app, supervisor, and sidecar exited, relaunch recovered the same stopped Terra session and creation event from the Rust SQLite authority.

## Automated evidence

- `make verify`: passed after the feature implementation, including architecture, 800-line source growth, generated TypeScript bindings, React build/tests, Tauri checks/tests, Rust workspace checks/tests, Clippy with warnings denied, and diff whitespace checks.
- `cargo check --workspace --all-targets --all-features --target x86_64-pc-windows-gnu`: passed with the installed rustup stable compiler and an isolated target directory.
- The Tauri Rust shell passed the same Windows GNU check with `TAURI_CONFIG='{"bundle":{"externalBin":[]}}'`. This proves the Windows shell source compiles; it does not prove a Windows sidecar executable was built or packaged.
- The real TCP/WebSocket boundary test commits `agent.create`, observes the correlated event and ACK, deduplicates an exact retry, rejects changed-payload request-id reuse, stops the server, reopens the same SQLite file, and recovers the same session identity.

## Cleanup

- The verification-owned packaged app, supervisor, Rust sidecar, provider discovery probes, and stopped Agent Session runtime resources were no longer running after the final close.
- The test-only desktop data directory and isolated cross-check build directories were moved to recoverable Trash locations.
- Pre-existing `/Applications/AgentsAssemble.app` processes and original-project provider fixtures remained running and were not signalled or modified.
