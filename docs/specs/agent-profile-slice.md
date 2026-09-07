# Agent profile and avatar ownership

Status: Phase 3 implementation in progress after Phase 2 approval at `5c8d17b`.

## Retained behavior and boundary

Original reachable `AgentIdentitySettings.tsx` and `RoomAgentProfileService` allow
Agent name and cropped avatar editing. The Rust Agent Session owns that identity;
its room Participant owns role, mute and membership. Human profile, pre-join avatar,
room appearance and message attachments retain their existing owners.

`agent.profile.update` is a canonical room WebSocket command authorized by active
room admission and `agent.control`, excluding Agent Bridge callers. It changes only
explicit name/avatar fields, independently of runtime configuration and provider
availability. Name is trimmed, nonempty, at most 80 Unicode characters, without
control characters. Unknown or wrongly typed fields fail. Session identity updates
and the participant identity projection commit atomically with the canonical event
and request replay result. Room-turn principal construction consumes participant
names, so this derived projection must update; role, mute, membership, runtime
custody, persona and human identity remain untouched. The runtime profile key does
not depend on cosmetic identity. No provider launch, restart or polling is added.

Agent avatar bytes have separate room/session custody with one current and one
pending asset. Upload authorization is bound to the exact room and Agent Session;
only a validated owned reference can become current. Replacement, clear and later
session deletion remove only exact owned references. Reuse bounded raster decode
and global storage accounting, not human profile upload credentials or pending
custody. Current public avatar reads preserve existing opaque profile-image URL
semantics; pending assets are not public. The frontend editor is enabled only for
implemented fields and confirmed server permission, with visible save failures.

## Acceptance and verification

- Name mutation passes durable replay/conflict, permission, malformed field,
  cross-room and corrupt identity rejection; restart retains identity while room
  membership and runtime/private state remain unchanged.
- Agent avatar upload, bind, replace and clear enforce exact target custody and
  storage limits. Foreign human, other-session and stale pending references fail.
  Human current/pending, pre-join transfer, appearance and attachments remain intact.
- TCP/WebSocket ACK and event/reconnect projections agree with stored identity.
  Copied member/editor flows use the canonical callback, show failed saves and do
  not persist client profile authority. Timeline, roster, search and mobile views
  project Agent identity from the Session and human identity from the human owner.
- Run affected local verification and mandatory architecture/source/CSS gates.
  Daybreak reviews the complete phase after local acceptance. Real provider and
  packaged frontend proof remain scheduled together at full reimplementation exit.
