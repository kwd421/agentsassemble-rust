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
