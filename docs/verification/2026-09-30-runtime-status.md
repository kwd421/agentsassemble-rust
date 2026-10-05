# Runtime record revalidation — 2026-09-30

User requested direct judgment and tests of whether old runtime findings remain,
without another external review. Base: `68121f63` on `codex/recovery-and-sonnet`,
with the pending avatar correction. This is macOS evidence, not Windows/Linux
execution evidence. No real provider, user database or frontend was changed.

## Current disposition

| Earlier item | Current evidence | Judgment |
| --- | --- | --- |
| Completed result cannot be committed and typing remains stuck | `completion_recovery_retains_result_until_publication_target_is_unmuted` passes, 9.78s; retains result across forced target mute, commits after unmute, asserts one provider start | Recovery correction is present and this managed failure path passes |
| Managed search integration fails parsing at `managed_tools.rs` | Reproduced on macOS in 5.09s. MCP returns the current compact text; test still parsed JSON. Updated only that test to consume its event ID and verify context content. Same integration passes in 5.24s | Stale test, not demonstrated search-runtime failure; corrected |
| Code-mode companion fails to start / cleanup evidence | `codex_code_mode_host_start_failure_preserves_cleanup_authority` passes in 13.02s | Current macOS failure contract passes; historical Linux failure is not disproved |
| ACP new/load rejection loses cause | Temporary duplex ACP diagnostic returns `fixture authentication rejected` from both requests. Both become `provider_protocol_invalid: The ACP provider returned an invalid protocol message.` | Current diagnostic defect reproduced for both paths |
| Reconciliation overwrites original start error | Temporary assertions in `startup_gone_terminalizes_old_start_and_unblocks_a_new_request` observe original `launch outcome was uncertain`, then `runtime_start_recovered_gone` and generic retry message after Gone reconciliation | Current diagnostic defect reproduced; not merely an old note |
| MCP helper exits but participant remains | `serve_stdio` calls `connector.close`; client `close` cancels transport and explicitly does not perform room leave | Current contract separates disconnection from explicit leave; this observation alone is not proof of a cleanup bug. No fresh EOF/UI reproduction in this pass |
| Claude/Grok/Codex long-idle failures, original retained Grok session | Short controlled tests do not reproduce the historical long-idle conditions or inspect the original session's custody | Still unverified; no claim of repair or causal diagnosis |

The macOS companion test deliberately expects uncertain custody, retains the
runtime handle, blocks duplicate start and expects shutdown uncertainty. Its pass
does **not** prove successful cleanup, and it does not certify Linux or Windows.

## Execution and limits

Only the named paths were run; no broad suite rerun. Cargo environment:
`CARGO_PROFILE_DEV_DEBUG=line-tables-only`, `CARGO_PROFILE_TEST_DEBUG=0`,
`CARGO_PROFILE_DEV_BUILD_OVERRIDE_STRIP=none`,
`CARGO_PROFILE_TEST_BUILD_OVERRIDE_STRIP=none`.

- Server tests: `cargo test -p agentsassemble-server --test integration agent_session_boundary::<name> -- --nocapture`.
- Companion: `cargo test -p agentsassemble-provider codex_code_mode_host_start_failure_preserves_cleanup_authority -- --nocapture`.
- ACP probe: `cargo test -p agentsassemble-provider diagnostic_acp_rejection_loses_cause -- --nocapture`.
- Persistence probe: `cargo test -p agentsassemble-persistence startup_gone_terminalizes_old_start_and_unblocks_a_new_request -- --nocapture`.

Temporary diagnostic assertions were removed after execution rather than retaining
tests that bless cause loss. The existing search integration correction remains.
Local logs: `/tmp/aa-runtime-{completion,companion,search,search-fixed,acp,cause}.log`.
Architecture/source-growth, 19 policy checks, formatting, diff, artifact checks
and Clippy for the affected server integration target pass.
The two cause-loss paths need an owner-level correction preserving safe diagnostic
detail; this revalidation does not claim that correction has been implemented.

Old Windows startup-suite incompleteness, actual window-close behavior and
Windows/Linux process cleanup require evidence on those operating systems.
The older Windows cache/tag cleanup failure is not a current macOS runtime failure.

## User-requested repeat reproduction

Repeated both cause-loss probes on the same `68121f63` base. ACP test passed
in 0.01s, recording both wire rejections (`fixture authentication rejected`) and
both returned generic `provider_protocol_invalid` errors. Persistence test passed
in 0.04s: snapshot cause changes from `runtime_start_unconfirmed` /
`launch outcome was uncertain` to `runtime_start_recovered_gone` / generic retry
text. Here a passing diagnostic means the loss was observed, not that it is fixed.

Exact test-only patch and full logs are retained in
`/tmp/aa-cause-repro-20260930/{reproduction.patch,acp.log,recovery.log}`.
Both instrumented test files were restored byte-for-byte after the run. No
production code changed. This reproduces error handling with a controlled ACP
peer and isolated database; it does not reproduce a real provider outage or UI.

## Actual packaged-app reproduction (00:35–00:39 KST)

The user rejected fixture-only reproduction. Reopened the signed isolated
`AgentsAssemble UI Check.app` and operated its UI through Computer Use. Created
`Grok Runtime Reproduction` with installed real Grok 1.0.34, model grok-4.6,
Medium, room-read-only permission, and `/tmp/aa-runtime-live/workspace`.
Initial start, Stop/Resume of the same provider session and a real published
`OK` response succeeded. No provider update was applied.

After stopping through the UI, saved the original permissions and temporarily
removed read/traverse access to **only this newly created test provider session
directory**. Clicked Resume in the app. Real persisted room events show:

- seq 33 at 00:38:18.193912: `provider_protocol_invalid`, generic invalid ACP
  protocol message. Seq 34 stores that cause with recovery required.
- seq 35 at 00:38:19.049077: `runtime_start_recovered_gone`; seq 36 replaces
  last_error with the generic pre-server-recovery/retry message.
- The running app displays the latter under Error cause. No server restart was
  performed between these events. The initial error is overwritten in <1 second.

This is a real provider/app failure induced by a reversible filesystem-access
condition, not a fabricated ACP response or direct DB mutation. It establishes
the actual UI/state overwrite chain. The raw provider ACP rejection payload was
not captured, so it does not establish its exact native error wording.

Evidence: `/tmp/aa-runtime-live/evidence/app-error.png`, `error-events.json`,
`state-transitions.jsonl`. Original directory permissions were restored after
capture. Production code and the user's original application data were unchanged.

After restoring permissions, UI Resume succeeds, shows Idle and provider-session
continuity, and clears the error (`app-recovered.png`). Stopped through UI and
quit normally; the exact app/supervisor/server processes are absent. Computer Use
reset. The isolated app/profile/cache returned to
`~/.Trash/aa-avatar-verification-20260930`; reproduction evidence remains in /tmp.

## Original incident investigation, distinct from induced reproduction

The original repeated Grok failure in conversation-context section 7 occurred
on Windows in `room-20260916T154644`, after the earlier login-path correction.
Read-only inspection of the seven local macOS `app.agentsassemble*/runtime.sqlite3`
databases did not find that room/session. Available app project hosts are local
only; no SSH connection is configured. Windows incident database and provider
state/log access have been requested from the user. No original Windows session
was reset or restarted from this Mac.

The separate Windows missing-login cause from WINDOWS section 8-1 was already
corrected by `e88def14` (2026-09-17): use USERPROFILE rather than HOME on Windows,
and pass the resolved existing auth path into the isolated Grok home. Current
source contains that correction. It does not establish why the older session
continued failing on September 22. The induced permission failure above also
does not establish that historical cause. No speculative product patch applied.
