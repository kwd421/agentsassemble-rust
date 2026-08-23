# OpenCode control-plane verification — 2026-08-24

Scope: remediation of the unauthenticated loopback control plane and port-owner
readiness race reported by the separate Daybreak manual security review. RimWorld
is a separate plugin slice and is outside this provider-runtime verification.

## Published implementation

- Original comparison commit: `d5046473010d1353a81ee38337360e6d98f7bd6f`.
- Initial authentication commit: `bcff0b55a8f082f77d69623528e4711882121b20`.
- Post-readiness custody commit: `3929e31f3407c60d829a030a527266daacaf9197`.
- Exact Unix child-handle commit: `642f27250e966ffaf6070a8a3e7503dc961c0999`.
- Correlated health-probe commit: `5c31ccf1cf33146a4e91431df7400b8508aca82d`.
- Each OpenCode runtime generates a fresh 64-hex-character password and supplies
  it with the fixed private username only to the exact sanitized child environment.
- The shared loopback client rejects missing or malformed credentials and applies
  HTTP Basic authentication to every JSON request and SSE stream.
- On Unix the launch environment crosses the guardian boundary through its
  anonymous inherited manifest descriptor rather than argv or guardian variables.
- The driver sends no credential until the byte-bound child's stdout emits the
  exact bounded ready line for the selected `127.0.0.1` port. An authenticated
  health check must then pass before RoomPortal registration sends its bearer.
- Every initial and later request creates a raw TCP connection first, transmits no
  bytes until exact guardian/child liveness is revalidated, and then uses Hyper on
  that already connected socket without automatic reconnection. A replacement
  listener reached after child death receives EOF and no credential; an
  established connection cannot migrate after later child death.
- On every Unix target, post-connect liveness includes a bounded request to the
  guardian, which synchronously checks the exact provider `Child` handle with
  `try_wait`. An exited macOS zombie cannot be approved by its retained PID/PGID,
  and the guardian keeps custody until the normal stop path performs cleanup.
- Every guardian health request carries a strictly increasing nonzero identity;
  the response must echo that identity and the exact provider PID. A timeout,
  cancellation, malformed response, or mismatch permanently poisons observation
  before another probe can consume buffered output. Normal stop bypasses health
  observation and still closes the guardian input to drive exact-owner cleanup.

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
  both the JSON and SSE client paths. A replacement-peer fixture also proves that
  a failed post-connect child-custody check sends zero bytes.
- The exited-leader regression runs on macOS, keeps a live descendant while the
  exact provider leader exits, and proves common async health rejects that leader
  before the existing stop path cleans up or fails closed on fork history.
- Focused regressions bind health responses to both request identity and provider
  PID and prove that an incomplete probe permanently poisons the channel.
- The six real WebSocket Agent Session boundary tests passed three consecutive
  runs with their existing semantic and frame-count assertions unchanged. Their
  per-frame receive deadline is five seconds so normal guardian startup is not a
  machine-load-dependent two-second failure.

## Real copied-UI evidence

- Build: exact debug macOS application bundle produced from the implementation
  commit's source; the exact `.app` path was targeted so an older installed bundle
  with the same bundle identifier could not be mistaken for this run.
- Entry point: the copied room UI resumed the durable OpenCode
  `opencode/hy3-free` Agent Session and sent one real room-composer message.
- Network boundary: unauthenticated direct requests to `/global/health` and
  `/event` on the observed provider port both returned HTTP 401.
- Product result: the real Hy3 session published
  `OPENCODE_PEER_CUSTODY_OK` through RoomPortal and returned to idle. After the
  exact-child fix, the rebuilt bundle repeated the flow and published
  `OPENCODE_MAC_CHILD_HANDLE_OK`. After correlated probes were added, a third
  rebuilt-bundle run published `OPENCODE_HEALTH_NONCE_OK`.
- Cleanup: the copied stop control reached stopped; the exact debug desktop,
  sidecar, guardian, anchor, and OpenCode process were shut down and confirmed
  absent. The post-fix run also confirmed the observed provider listener was
  absent. The earlier verification-only temporary directory was moved to
  recoverable Trash. Existing original-project and unrelated processes were left
  untouched.

## Review state

The implementation is pushed before review. The same Daybreak task will now
perform a manual, read-only re-review of the published fix and this evidence. No
additional Deep Scan or automated scanner is authorized for that review.
