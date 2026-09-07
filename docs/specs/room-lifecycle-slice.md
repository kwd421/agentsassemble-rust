# Room lifecycle and moderation

Status: Phase 4 active after Phase 3 approval at `b8fd15b`.

## Contract and dependency order

Retain original canonical participant kick/export and room close/archive/delete,
including their reachable frontend or bounded HTTP entry points. The room owner
controls deletion and must supply the current room name. Room moderation remains
server-authorized; a target cannot replace a principal or remove the local owner.
Host-device claim belongs to the existing Rust local bootstrap authority rather
than a second browser-token identity. Verify that entry point as part of this phase.

First consolidate the repeated participant row codec (C-01). One connection-scoped
load-by-key decodes an optional row; one exact save checks the affected row count.
Transactions, missing-row policy, identity/authority checks, state transitions and
canonical events remain at their existing owners. Preserve query keys and rollback
behavior. Do not introduce a generic repository or cache.

Before more settings, remove the future-only `activity_plugin` field (C-09): its
only frontend consumer is the explicitly deferred RimWorld surface. Keep that
surface unreachable. Use an explicit clean schema boundary without migrating user
data. Export the authoritative 128-character room label limit to its existing UI
(F-19); preserve existing channel/profile ownership and server validation.

Then connect moderation and room lifecycle to the existing command, durable replay,
provider runtime custody, human-session revocation and room directory owners.
Kick/export removes exact membership/access, stops or retains explicit unresolved
custody for the exact Agent runtime, and publishes the canonical removed state.
Close prevents further writes/admission and cleans owned running work and access;
archive/unarchive preserves data and an authenticated management path. Archive revokes
live access and stops owned work while preserving membership and settings; restoring
requires completed cleanup and never reopens a closed room. Delete
requires exact-name confirmation and a durable result/tombstone so retry cannot
retarget a recreated room. Cleanup failure must remain visible and recoverable;
never claim runtime termination or deletion from only a submitted effect.

Removal commits access revocation before external cleanup. A pending cleanup row
references the existing Agent Session rather than copying its runtime identity or
creating another provider supervisor. It fences new launch/re-add effects until
the existing exact runtime/turn reconciliation owners prove absence. The current
server recovery watcher consumes bounded pending cleanup work; no second timer is
introduced. Removed membership survives normal stop and recovery. Room rows and
owned assets remain present while cleanup is unresolved; physical deletion follows
confirmed cleanup and retains an exact command tombstone outside the room cascade.

Positive runtime absence must remain durable even when it precedes moderation.
Packaged archive-after-restart exposed the old Gone transition clearing custody
while retaining disconnected/recovery-required state. The reconciliation owner
must checkpoint stopped/no-recovery before clearing that custody, so a subsequent
archive, kick or delete can complete without re-observing an erased identity.
Ambiguous observations retain their exact identity and recovery fence. No inference
from empty fields, old error text or an external process PID repairs prior records.
Verify the ordinary start → confirmed shutdown → database reopen → archive/restore
sequence, alongside the existing uncertain-absence negative case and packaged flow.

Deletion records one exact request and the terminal closed event before cleanup.
Until completion its HTTP resolution is unresolved, retaining the same retry intent.
The existing recovery watcher finalizes bounded pending deletions after the close
event has entered canonical publication and all runtime cleanup rows are gone.
Only that transaction deletes room-owned rows/assets and commits the immutable
success result outside the room cascade. Replay authenticates the current local
bootstrap/profile owner and the stored request/hash/UID, without requiring deleted
membership or touching a new incarnation. No request can revise pending deletion;
the closed room cannot be restored. No new timer or filesystem asset owner is needed.

Lifecycle management uses the authenticated HTTP directory boundary because archive
and close invalidate ordinary room admission, and restoration must work without a
room socket. Both transports delegate to the same bounded room command owner; the
HTTP response is not the broadcast authority. Existing sequence-coupled commands
and all real-time updates remain WebSocket-owned. The user's 2026-09-07 transport
clarification prioritizes stability and replaceable transport adapters, not a
wholesale Discord-protocol copy. No duplicate HTTP moderation wrapper is retained
without a distinct integration consumer.

No client orchestration substitutes for server lifecycle. No real providers or
user-room deletion runs during implementation. No new fallback, gate exception,
plugin framework, scheduler, or periodic cleanup is implied by this phase.

## Acceptance and verification

- Existing participant mutation, admission, profile, lifecycle and recovery tests
  preserve their public results with the shared codec; missing exact save fails and
  transaction rollback remains observable.
- Room label hints derive from the server limit; unsupported plugin state is absent
  from live schema, wire and UI projections.
- Each advertised moderation/lifecycle action has exact principal/target checks,
  canonical ACK/events, deterministic replay/conflict and restart recovery. Guest,
  Agent Bridge, wrong-room, stale incarnation and owner-removal attempts fail.
- Runtime cleanup is owned by the existing server runtime lifecycle; in-flight
  turns, reservations, human sessions, invite/ticket authority and asset custody
  cannot survive room/participant removal with usable authority.
- Desktop/mobile settings and roster controls report failure, reflect events and
  reconnect, and expose only implemented capabilities. Preserve archive management
  and deletion replay without weakening active-room admission for ordinary clients.
- Run affected local TCP/WebSocket, persistence and frontend proof plus unchanged
  architecture, source, format, Clippy and CSS gates. Obtain whole-phase Daybreak
  approval before Phase 5. Direct packaged app manipulation is required before the
  phase review, including earlier Phase 2 controls and Phase 3 profile UI. Real-provider
  execution remains at final closeout.
