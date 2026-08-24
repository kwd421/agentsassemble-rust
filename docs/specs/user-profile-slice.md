# Human User Profile Slice

Status: active implementation owner

## Definition

The copied left-bottom user panel reads and updates one authenticated human's
server-wide profile through the Rust runtime. The same display name and avatar
are projected into every current room membership and update every connected
viewer through canonical room events.

## Authority boundaries

- The durable user profile is the SSoT for human display name, handle, presence
  preference, custom status, avatar label/image, banner, accent, local microphone
  preference, and local deafening preference.
- Room membership remains the SSoT for role, joined/left/kicked/exported state,
  room mute, invite scope, and derived capabilities. A profile update may not
  overwrite any of those fields.
- Each Agent Session remains the SSoT for its own agent display name, avatar,
  provider/model/runtime state, permissions, and ownership. It is never merged
  with its human owner's profile.
- Only human memberships whose durable participant identity belongs to the
  authenticated user receive the profile display-name/avatar projection.

## Reachable contract

- `GET /api/user-profile` and `POST /api/user-profile` use an authenticated
  server-derived principal. The packaged desktop obtains a fresh one-use local
  principal ticket through the existing private Tauri/runtime control boundary
  for each HTTP operation; it never exposes the host secret.
- The profile is stored once by user ID. Its participant ID is stable and is not
  supplied by the profile payload.
- Profile input is bounded and normalized with the original accepted status,
  banner, accent, text, and avatar-reference shapes. Unknown fields do not grant
  authority. Invalid credentials, bodies, or stored data fail visibly.
- A content-changing save advances one durable profile revision. An identical
  retry returns the existing revision without changing `updated_at` or emitting
  duplicate projection events. A profile mutation and every affected participant projection commit before the
  response. Each affected room receives one ordered `participant_updated` event
  containing only the human participant ID, display name, and avatar projection.
- Snapshots and reconnects read the updated participant rows, so live delivery is
  not a second authority.
- The copied profile-avatar flow uses `POST /api/attachments` with exact purpose
  `profile_avatar`, bounded base64 data, and only PNG/JPEG/GIF/WebP. Rust stores
  opaque attachment IDs and serves only validated safe raster content from
  `/api/attachments/{id}?view=1`. Other attachment purposes remain explicitly
  unsupported until their own migration slice.

## Failure and retry semantics

- A failed SQLite transaction changes neither the user profile nor any room
  participant projection and emits no room event.
- Retrying an identical profile save is allowed and returns the canonical stored
  profile. A changed display name or avatar produces a new room projection event;
  changing only local profile preferences does not create room events.
- The response may race live event delivery, but both describe already committed
  state. A reconnect recovers the same participant projection without client-side
  synthesis.
- Attachment bodies, counts, and bytes are bounded. Unsupported content, invalid
  base64, path-shaped IDs, ownership mismatch, or quota exhaustion fail closed.

## Verification

- persistence tests cover profile normalization, restart durability, atomic
  multi-room display/avatar projection, preservation of room-owned fields, and
  separation from Agent Session profiles;
- server boundary tests cover one-use authentication, body limits, invalid
  profile input, safe avatar upload/read, unsupported purpose rejection, and
  canonical event publication;
- copied frontend tests cover desktop runtime routing and UserPanel state;
- the exact packaged app changes the left-bottom profile, observes matching room
  roster/message attribution after reconnect and runtime restart, verifies modal
  stacking, and shuts down every verification-owned resource.

RimWorld, account providers, invites/admission, general message attachments,
room appearance uploads, friends, channels, and new console/profile-management
surfaces are outside this slice.
