# Agent profile and avatar ownership

Status: Phase 3 local implementation and acceptance complete; whole-phase Daybreak review pending.

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

## Avatar storage contract

Schema 58 adds `agent_avatar_assets` with a room/session foreign key, unique
room/session/state custody, PNG byte/size limits and pending expiration. Older
schemas remain rejected by the existing explicit schema boundary; no user database
is migrated. The public Agent Session now includes required `avatar_image_url`.

The exact current local room manager may upload for an existing Agent target. Each
upload replaces that target's prior pending image and expires pending images after
15 minutes on the next upload; there is no timer. Shared raster preparation retains
its existing decode bounds, and the existing absolute asset count/byte accounting
includes this table. `agent.profile.update` accepts name and/or avatar reference;
current replacement, pending promotion and public Session/participant projection
commit in its existing transaction. Clear removes the exact current reference;
an unrelated pending image remains bounded by its existing expiry. Public reads
require both current asset custody and the exact Session reference. Foreign human,
appearance, other-session, malformed or expired references cannot bind. Session
removal cascades only that session's assets. Upload and frontend connections use the transport and editor contracts below.

## Avatar transport

`POST /api/agent-avatars/upload/{session_id}` consumes a one-use local-manager
credential whose purpose includes that exact Session ID. The stored grant also
binds server identity, authority lineage and room incarnation; storage revalidates
that manager before committing bytes. Wrong target/purpose consumes and rejects
the ticket. Existing body/PNG bounds and no-store/error response rules apply.
`GET /api/agent-avatars/{asset_id}` serves only a bound current PNG with the existing
safe attachment headers. The private control command and bundled-only Tauri command
have dedicated request/response variants and a registered capability. The confirmed local manager can crop and upload from desktop/mobile Agent details;
`agent.control` gates the editor. The existing canonical WebSocket command binds the
returned pending reference, and its ACK reports save success. Name-only saves retain
the avatar; clear is explicit. Unmount cancels upload, and the existing current-socket
authority rejects a bind after room change. Session identity projects consistently
into timeline/history/search, roster, mobile and mention surfaces. Reference constants
are generated from Rust; human upload references retain their separate parser.
