# Final parity and integration

Status: Phase 9 begins after Daybreak approved Phase 8's correction, cumulative
range and exact `f776e2e` at C0/H0/M0/L0. Final acceptance is still pending.

## Definition and boundary

Close the retained product against original `d5046473010d1353a81ee38337360e6d98f7bd6f`
and the completed Phase 1–8 contracts. The exposure inventory remains
`docs/FRONTEND_BACKEND_GAPS.md`; measured runs and reviewer dispositions remain
`docs/VERIFICATION.md`. Prior exact reviewed evidence is reused, with current
integration coverage rather than repeating every isolated case.

The existing Rust owners retain authority over identity, room participation,
messages, Agent Sessions, provider custody, the three external-agent admission
paths, operational state and assets. The frontend must not invent a room or select
a deferred game through copied startup parameters. Voice, Mafia and RimWorld remain
unimplemented and inactive; their deferred source is preserved. This phase adds no
legacy fallback, compatibility shim, new provider or architecture-gate exception.

First confirmed gap: `createStartupRoute` still invokes `roomFromMafiaParams`,
fabricating a game-labelled room and selecting `live` for a Mafia URL. Disconnect
that startup branch while preserving canonical direct-room and admission routes.
Inspect the emitted production module graph and code for deferred mount/request/
poll/heartbeat paths, then exercise the packaged entry points.

The emitted API audit also finds a copied `/api/local/workspace-picker` branch.
Original `providers.py` explicitly limits that operation to the local operator.
The Rust owner is Tauri's `choose_local_workspace`; remove the absent HTTP branch
and preserve the native bridge's explicit unavailable error for browser callers.
Do not expose host filesystem selection through an admitted browser credential.

The baseline full Rust verification passes 867 tests but retains 31,892,033,536
artifact bytes, exceeding the unchanged 18 GiB gate. Fifty-four test dSYM bundles
alone occupy 20.37 GiB. Use Cargo's `line-tables-only` debug information in the root
test profile, preserving file/line backtraces and the existing test assertions,
overflow checks, optimization settings and SHA-256 override. The tradeoff is no
default type/variable inspection in test binaries; full debug remains an explicit
Cargo profile override. Ordinary development and release profiles stay unchanged.
After all baseline work stops, use the existing artifact owner to retire the
replaced artifacts, then verify the same full suite and measure the resulting size.

Actual Codex startup exposes an upstream transport mismatch: the installed native
Code Mode Host accepts a `grpc://127.0.0.1:0` listener and publishes a canonical
`http://127.0.0.1:<port>` gRPC endpoint. Its former `ws://` invocation exits before
readiness. Update the existing host launcher and strict readiness parser to that
observed contract, preserving the same executable bundle, loopback-only binding,
process group, descriptor isolation and cleanup authority. Do not add transport
fallback or infer successful cleanup from a closed pipe. Verify the real CLI
readiness, existing guardian/transport cases and a complete packaged Codex turn;
distinguish failed-start cleanup evidence from the existing macOS uncertainty
contract. A controlled companion exit reproduces launcher fork/exit before cleanup;
the guardian deliberately cannot publish absence in that state. Preserve its
uncertain generation and blocked retry instead of inferring absence from dead PIDs.

Actual Cursor `auto` startup exposes another executable boundary: the installed
shell launcher resolves its sibling Node, JavaScript chunks and native modules.
Single-file staging makes those dependencies disappear. Bind the installed Cursor
package at its executable owner, include its dependencies in the durable identity,
and stage the verified package with the launcher's relative paths intact. Preserve
bounded filesystem work, immutable launch bytes, exact process custody and the ACP
driver's permission/model contract. Do not execute mutable original package paths,
add a wrapper/environment fallback or weaken the existing cleanup proof. Verify
dependency mutation/missing-file rejection, the installed launcher and complete
packaged `auto` response/stop. The failed original generation remains uncertain.

## Acceptance and verification

- Reconcile every retained exposed feature with its original entry, Rust owner,
  state/failure semantics and frontend or explicit service-only consumer. Report
  remaining partial, indirect and intentionally unavailable operations accurately.
- Preserve local and admitted-user desktop/mobile flows, room lifecycle,
  restart/reconnect, headers/right panels/search, persona import and explicit
  selection, ordinary ordered/ambient conversation and permitted tools.
- At final integration, run only configured DeepSeek, Codex, OpenCode, external
  Antigravity through Room Connector, Grok and Cursor `auto`. Verify the separate
  external and managed AgentBridge paths using the allowed provider matrix. All
  other providers retain source/contract evidence without actual execution.
- Collect CPU, memory, disk, process/task count and latency at their owners;
  distinguish measurements from unavailable instrumentation. Preserve approximately
  20 percent CPU headroom and use sequential bounded heavy builds. Optimize only
  a measured cost or concrete failure, recording before/after evidence.
- Run affected checks and unchanged mandatory gates. Directly operate the packaged
  app at desktop and 390px/low-height sizes; local tests do not replace that proof.
- Quit only each verification app and its owned children, reset Computer Use, and
  remove exact isolated data/artifacts after use. Preserve user data and unrelated
  processes; use the existing artifact owner after active builds have ended.
- Obtain final GPT-6 Pro and Daybreak Blue xhigh approval for the complete retained
  product, original-to-Rust inventory, unreviewed individual commits, cumulative
  implementation and exact pushed HEAD, correcting supported findings first.

Unavailable external dependencies or authorization gaps remain explicit limits;
they do not become successful or simulated real-provider evidence. At the accepted
parity exit, stop and report before any repository-wide 500+ LOC cleanup.
