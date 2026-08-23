# OpenCode control-plane verification — 2026-08-24

Scope: remediation of the unauthenticated loopback control plane and port-owner
readiness race reported by the separate Daybreak manual security review. RimWorld
is a separate plugin slice and is outside this provider-runtime verification.

## Published implementation

- Original comparison commit: `d5046473010d1353a81ee38337360e6d98f7bd6f`.
- Rust implementation commit: `bcff0b5058d6c928d2330a7de284694a8f8fbfa3`.
- Each OpenCode runtime generates a fresh 64-hex-character password and supplies
  it with the fixed private username only to the exact sanitized child environment.
- The shared loopback client rejects missing or malformed credentials and applies
  HTTP Basic authentication to every JSON request and SSE stream.
- On Unix the launch environment crosses the guardian boundary through its
  anonymous inherited manifest descriptor rather than argv or guardian variables.
- The driver sends no credential until the byte-bound child's stdout emits the
  exact bounded ready line for the selected `127.0.0.1` port. An authenticated
  health check must then pass before RoomPortal registration sends its bearer.

## Automated evidence

- Complete `make verify`: passed, including architecture/source-growth policy,
  generated TypeScript, copied React production build and CSS provenance, 65
  frontend files with 332 tests, 13 Tauri tests, all Rust tests, warning-denied
  Clippy, and whitespace checks.
- Warning-denied full-workspace/all-target `x86_64-pc-windows-gnu` source check:
  passed using the installed rustup stable compiler. This is cross-platform source
  evidence, not a Windows real-provider claim.
- Focused regressions verify fresh credential shape and environment ownership,
  rejection of a ready line for any other port, and an Authorization header on
  both the JSON and SSE client paths.

## Real copied-UI evidence

- Build: exact debug macOS application bundle produced from the implementation
  commit's source; the exact `.app` path was targeted so an older installed bundle
  with the same bundle identifier could not be mistaken for this run.
- Entry point: the copied room UI resumed the durable OpenCode
  `opencode/hy3-free` Agent Session and sent one real room-composer message.
- Network boundary: unauthenticated direct requests to `/global/health` and
  `/event` on the observed provider port both returned HTTP 401.
- Product result: the real Hy3 session published
  `OPENCODE_AUTH_CONTROL_OK` through RoomPortal and returned to idle.
- Cleanup: the copied stop control reached stopped; the exact debug desktop,
  sidecar, guardian, anchor, and OpenCode process were shut down and confirmed
  absent. The verification-only temporary directory was moved to recoverable
  Trash. Existing original-project and unrelated processes were left untouched.

## Review state

The implementation is pushed before review. The same Daybreak task will now
perform a manual, read-only re-review of the published fix and this evidence. No
additional Deep Scan or automated scanner is authorized for that review.
