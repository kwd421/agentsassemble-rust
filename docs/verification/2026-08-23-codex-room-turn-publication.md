# Codex ordered room-turn publication verification — 2026-08-23

Scope: durable ordered-floor queueing and assignment, provider-effect isolation from the room mutation owner, canonical Codex final publication, failure recovery, and browser projection of live Agent Session state. This evidence uses an owned Codex app-server fixture; it is not a real-provider or Computer Use claim and does not cover the final Terra/Flash/Hy3 matrix.

## Automated evidence

- `make verify`: passed, including architecture and 800-line source gates, generated TypeScript, React production build and 29 frontend tests, Tauri checks and 12 tests, 52 persistence tests, 57 provider tests, 9 server unit tests, 14 server boundary/integration tests, warning-denied workspace Clippy, doc tests, and diff whitespace checks.
- The persistence boundary commits a host `message_final`, one ordered target queue entry, and the first assignment together. Replay cannot issue a second assignment. A second message received while the provider is busy stays pending; completion atomically publishes one provider final, terminal turn state, provider-sync cursor, idle state, and the next assignment. Reusing the old turn ID fails closed.
- Failure coverage proves that an unknown provider failure becomes a stable public code, removes local paths and secret-shaped values, restores inflight input to pending, clears the active source/input authority, and marks the session recovery-required without assigning an error-state session.
- Corrupt provider-sync cursors, incomplete active/inflight fields, and multiple active owners are rejected as inconsistent durable authority instead of being reinterpreted as an empty floor; the source-message transaction rolls back without an event.
- Routing regressions preserve the original final-direct-mention rule across unique session IDs, display names, split aliases, and bracketed mentions, while ambiguous aliases cannot select an Agent Session.
- The real TCP/WebSocket boundary creates and starts an Agent Session against an owned persistent Codex app-server fixture, blocks its first `turn/start`, and receives a second room-message ACK before releasing the provider. It then observes exactly one canonical Agent Session final for each turn and verifies that each provider input contains the correct bounded canonical room update. Provider-process boundary scenarios are serialized by an explicit test mutex instead of depending on scheduler timing.
- The browser accepts validated public `agent_session_state` events into the Agent Session projection, rejects events containing private runtime authority, and excludes `turn_started`, `turn_state`, and session-state coordination events from the visible message timeline.
- The workspace and Tauri shell passed `check` and warning-denied Clippy for `x86_64-pc-windows-gnu` using the installed rustup stable compiler and an isolated target directory.

## Authority and recovery boundaries

- SQLite is the only ordered-floor authority. Pending/inflight event IDs, active source event, provider-input cursor, and active turn ID are private durable fields; public session and turn events derive from them.
- The room task owns mutation order but not the provider wait. One owned child task performs provider I/O, and its result must re-enter the mutation owner before publication. Stop or replacement makes a late result stale, and a stale result cannot publish.
- Startup reconciliation and stop merge inflight input back into pending before clearing active authority. A successful or replayed `agent.start` explicitly attempts the recovered pending assignment, so recovery does not wait for an unrelated new message.
- Provider output and diagnostics remain untrusted. Only one bounded final enters the room; failures cross the shared redaction boundary.

## Cleanup and remaining scope

- Every mock provider process created by the tests was reaped by its adapter/server owner.
- The isolated Windows cross-check target was moved from `/tmp` to Trash after completion, and its temporary Tauri Windows sidecar placeholder was removed from the repository.
- No user-owned provider, browser, original-project, or desktop process was signalled or modified.
- Actual frontend Computer Use and real Agent Session verification remain pending until the exact Codex Terra, Antigravity Flash, and OpenCode Hy3 free matrix is implemented and available. No mock or automated browser result is counted as that evidence.
