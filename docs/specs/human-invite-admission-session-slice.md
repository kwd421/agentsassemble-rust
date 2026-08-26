# Human Invite, Admission, and Room Session Slice

Status: design candidate; no Rust admission route is active

## Definition

This slice establishes one durable authority for a human browser invite, admission,
profile binding, room membership, and expiring room session. It then lets that live
session exchange for exact one-use WebSocket, profile, attachment, and preference
grants. It does not make an external invite reachable until the separate trusted
public-ingress owner is complete.

The behavior comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`. The Rust baseline for this design is
`7905a0b`. Current behavior below was confirmed from the original invite routes,
admission coordinator, invite/session repositories, attachment routes, public-ingress
checks, copied frontend controller, and real product flows. Old product markdown is
not an authority.

## Current reachable contract

- A current human invite is room-bound, expiring, either read/write or read-only,
  and bounded to 1, 5, or the reusable-link limit of 128 distinct principals. The
  copied UI defaults to one use and 24 hours.
- Invite preflight does not write. It distinguishes invalid or expired invites,
  agent-only links, a live same-room session, a known reusable-link device, an
  already joined member, and a browser that must supply a profile.
- Admission requires a canonical nonzero UUID request ID. Exact retry returns the
  same admitted identity and bearer. Reusing that request with different admitted
  input conflicts.
- A successful admission consumes invite capacity, creates or resolves the human
  identity, joins the participant, creates the session, and emits the canonical
  participant-joined event. A collision with a different profile-bound participant
  is rejected before invite consumption.
- Browser session bearers use the `aas1.` prefix, expire after one hour, are stored
  only as SHA-256 fingerprints, and are revoked on exact session revoke, participant
  leave or kick, room close, or credential revoke. A connected WebSocket stops being
  authorized when its backing session is revoked or expires.
- A normal session can post; a read-only session cannot. Both can read the canonical
  room. Room role, mute, membership, and command capabilities remain room-owned and
  are never taken from the person's profile.
- A pre-join avatar upload is optional and bounded. It is temporarily owned by the
  exact invite-and-device subject, supersedes that subject's older pending avatar,
  expires, and becomes the admitted user's profile avatar only during successful
  admission. Invalid or expired optional avatar data is treated as an omitted
  optional avatar: admission still commits, but it cannot claim unrelated media or
  create partial profile state.
- The lower-left human profile, room member projection, profile editor, and the
  user's preferences must resolve the same `user_profiles` row. An Agent Session
  profile remains a separate Agent Session authority.
- The original one-use browser path leaves `principal_user_id` empty, which makes
  its admitted lower-left profile and preferences unauthenticated. This contradicts
  the required human-profile SSoT. Rust corrects it by creating an admission-scoped
  user for a one-use invite without registering a reusable device credential.
  Therefore two unrelated one-use invites do not silently merge one person, while
  every admitted human still has one authenticated profile.

## Authority and persistence

The existing `SqliteStore` remains the only durable writer and continues to use one
SQLite connection. The clean current schema adds these records; no previous schema
is migrated, imported, or interpreted.

- `room_invites` owns invite UUID, token fingerprint, room, base participant and
  display identity, scope, maximum uses, use count, expiry, revocation, creator, and
  creation time. Raw invite tokens are never persisted.
- `human_device_credentials` maps only reusable-link device fingerprints to one
  stable human user. One-use admissions do not create this mapping.
- `human_room_sessions` owns the session fingerprint and exact room, user,
  participant, browser client kind, invite scope, optional reusable credential
  fingerprint, admission time, and expiry. Raw session tokens are never persisted.
- `human_admission_results` binds an admission key, canonical request UUID, invite,
  payload hash, session fingerprint, and bounded public result without the raw
  bearer. The deterministic issuer can reproduce the exact bearer for a live exact
  retry. A completed but later expired or revoked admission is a terminal
  `admission_session_unavailable`; it never creates a replacement session.
- `profile_attachments` remains the single human-avatar asset owner. Its state
  constraint permits either a user-owned pending/bound image or an admission-pending
  image bound by invite ID plus a fixed-size opaque subject fingerprint. Admission
  atomically transfers a valid pending image to the new user and binds it. No second
  filesystem store or duplicate image decoder is introduced.

Token fingerprints, admission keys, payload hashes, and pending-upload subjects are
fixed 32-byte blobs. IDs used in public JSON remain their canonical text forms.
Invite tokens use operating-system randomness and the existing `aaj1_` prefix; the
human response exposes that one value through the current `invite_token` and
`join_code` aliases rather than retaining a second signed LAN bearer. A small
non-serializable, non-debuggable session issuer uses a separate operating-system-
random 32-byte HMAC-SHA256 key stored by the existing permission-checked persistent
host-key owner. Invite and session fingerprints remain ordinary SHA-256. The issuer
does not reuse the Ed25519 host key or process-local host-control secret, log key
material, or put bearer material in events or idempotency JSON.

## Admission transitions

Preflight reads the invite, optional current bearer, and reusable device fingerprint
without allocating durable state. A presented session counts as `existing_session`
only when its durable row, expiry, room, active membership, participant/profile
binding, client kind, and scope all remain valid.

Admission validates bounded input and then performs one SQLite transaction:

1. load the active room and invite and enforce expiry, revocation, client kind,
   scope, maximum use, and global/per-room public-session capacity;
2. derive the idempotency key and payload hash, returning only an exact live retry
   and rejecting a conflicting or terminal retry;
3. resolve the reusable credential user or allocate an invite-scoped one-use user,
   keeping participant/profile collisions fail-closed;
4. consume one invite use, upsert the joined human participant and matching profile,
   claim the optional exact pending avatar, create the session/result, and append at
   most one `participant_joined` event;
5. commit before publishing the event or returning the bearer.

Any validation or database failure rolls back every step. Because all affected
records share one SQLite transaction, the Python coordinator's separate JSON invite
repository, identity database, room repository, workflow journal, compensation,
and resume saga are not reimplemented.

For a one-use invite the idempotency key binds invite and request. For a reusable
invite it also binds the required device fingerprint. The payload hash covers every
field that can change identity or membership, including display name, client ID,
participant type, and optional avatar reference. Client input never chooses user
ID, participant ID, capabilities, role, mute state, or session expiry.

The current public capacity is preserved: at most 448 live public sessions globally
and 112 per room, with at most 128 distinct reusable-link principals. The remaining
original 64 global and 16 per-room slots are reserved for later separately owned
operator and external-agent sessions, not borrowed by this human slice.

## Session-derived grants and revocation

Raw room-session bearers are accepted only by the admission preflight, leave/revoke,
and typed session-exchange endpoints. Target profile, attachment, preference, and
WebSocket routes never interpret a raw session bearer as a purpose ticket.

The session exchange surface is a closed set of typed routes rather than a
client-selected purpose string:

- WebSocket connect;
- own profile read, update, avatar upload, and avatar read;
- room preference read and, for normal scope only, preference write;
- room attachment upload/read when the corresponding message behavior is active.

Read-only room scope denies posting, preference mutation, and room attachment
upload. It still permits the human to read and edit their own person profile because
that profile is not room role or posting authority.

Every derived grant retains immutable session-fingerprint provenance plus the exact
room, user, participant, client kind, scope, and purpose. Grant consumption removes
the in-memory item first and then revalidates the durable session and current
room/profile/membership binding. Wrong purpose, wrong room, replay, expiry, or
session-only revocation consumes and rejects the grant. Existing local-operator
typed grants remain separate and unchanged.

A session WebSocket subscribes to revocation notification before its connect grant
is consumed, then revalidates the database after subscription. It revalidates before
each client frame, and a post-commit session/participant/room revocation broadcast
closes an idle socket. Broadcast lag or closure triggers durable revalidation and
fails closed. The database remains authority; the broadcast is only prompt
notification. No timer or cache independently decides session validity.

Leaving performs membership-left transition, revokes that participant's room
sessions, and appends the canonical event in one transaction. Exact session revoke
does not remove membership. Kick and room close revoke affected sessions in their
own canonical room transactions. Notifications happen only after commit.

## Transport and frontend activation

The transport split follows the verified Discord-style ownership rule:

- bounded request/response operations use HTTP: invite creation, preflight,
  admission, pre-join avatar, session-ticket exchange, own profile/preferences,
  and leave/revoke;
- canonical snapshot, room events, and room commands use WebSocket.

There is no HTTP-to-WebSocket or WebSocket-to-HTTP fallback. A browser is not shown
as joined until the canonical room socket has authenticated, subscribed, and
received its initial snapshot. Normal and read-only UI capabilities come from the
verified session principal and room state, not query flags or local storage.

The copied frontend's `localPreviewInviteUrlForRoom` query-only guest path and
`secureInviteCopyTarget` fallback are removed when real invite creation activates.
Until trusted public ingress exists, the external-invite controls remain explicitly
unavailable; a local preview is not presented as admission parity. The token is
removed from browser history after it is captured, and stored session state cannot
override a failed durable verification.

## Trusted ingress boundary

Managed Cloudflare tunnel custody, explicit reverse-proxy proof, manual public URL,
host control, and operator pairing are a separate slice. Forwarding headers or a
configured URL alone are not ingress authority. This slice may be exercised through
the real local Axum server, but that does not prove an external browser flow.

External invite creation remains disabled until the ingress slice proves the exact
origin/host/protocol and either a process-owned managed-tunnel origin or configured
reverse-proxy secret. No raw legacy host token, local-development bypass, or query
flag is added meanwhile.

## Evidence-driven simplifications and costs

### Single transaction instead of the original admission saga

- Prior cost and symptom: the original coordinator spans 740 lines and, together
  with its saga/workflow records, performs repeated durable workflow updates around
  separate invite, identity, membership, and session writes. Those writes and
  compensations exist because the authorities are in different stores.
- Change intent: put the same product transition under the already canonical single
  SQLite writer and one transaction. This removes intermediate disk syncs and crash
  windows without adding a generic workflow framework.
- Preserved contract: exact idempotency, atomic invite consumption, collision
  rejection, durable restart behavior, one membership event, and no partial success.
- Trade-off: admission write concurrency is serialized by the existing one-connection
  store. That is accepted because room mutations already have this owner and the
  public capacities are bounded. No unmeasured throughput claim is made.
- Verification: rollback injection, exact/conflicting retry, restart, capacity, and
  event-count tests query only committed public/durable results.

### Binary digests instead of encoded digest text

- Prior cost: the original stores 32-byte digests as 64-character hex strings and
  its workflow journal serializes repeated state as JSON.
- Change intent: fixed BLOB checks halve digest column bytes and eliminate parsing
  ambiguity while retaining maintained SHA-256/HMAC/HKDF implementations.
- Preserved contract: only fingerprints are durable and comparisons remain exact;
  public IDs and bearer formats do not change.
- Verification: schema constraints reject the wrong length, database inspection
  finds no raw bearer, and restart/exact-retry tests reproduce the same token.

### Event-driven revocation without periodic session polling

- Prior cost and threat: polling every live socket adds database reads and latency
  even when no revocation occurs, while notification alone has a subscribe/consume
  race and is not durable authority.
- Change intent: subscribe before grant consumption, revalidate after subscription,
  revalidate each client frame, and broadcast only after durable revocation commit.
- Preserved contract: revocation after ticket issue or during an idle connection
  invalidates the exact session promptly; a missed/lagged notification fails closed.
- Trade-off: an idle socket depends on the process-local broadcast for prompt close,
  but process restart closes the socket and reconnect must revalidate the database.
- Verification: deterministic barriers cover revoke-before-consume,
  revoke-after-connect idle close, notification lag/closure, and client-frame races;
  tests do not sleep or inspect private maps.

No additional cache, repository interface, background cleanup framework, generic
credential provider, multi-database saga, or future agent-session abstraction is
authorized by this slice. Expired rows are filtered authoritatively and removed by
bounded work piggybacked on relevant writes unless measured evidence later proves a
separate cleanup task necessary.

## Non-goals

- external-agent/RoomConnector admission, managed Agent Sessions, companion invites,
  operator pairing, account-provider login, server directory/account authentication;
- trusted public tunnel/reverse-proxy lifecycle and operator access from that origin;
- room appearance activation, custom channels, voice, activity plugins, or RimWorld;
- old database conversion, Python compatibility, legacy host-token behavior,
  transcript scraping, fallback transport, local query-flag admission, or placeholder
  identities;
- automatic demo meetings, private pre-research, forced rounds, automatic agenda,
  synthesis, decisions, task assignment, or their v0-only models/artifacts/contracts.

## Acceptance and verification

- Fresh-schema tests prove constraints, cascade/revoke behavior, and rejection of
  every non-current schema without migration.
- Persistence tests prove preflight has no writes; admission is atomic under injected
  failure; exact retry is stable across restart; conflicting/expired/revoked retry is
  terminal; collision and all capacity edges fail before consumption; and no raw
  invite, device, or session bearer reaches SQLite, events, logs, or fixtures.
- Pre-join avatar tests prove exact invite/device ownership, replacement, expiry,
  safe-raster limits, failed-admission custody, and atomic ownership transfer to the
  same profile rendered in the room and lower-left panel.
- Real Axum tests exercise create, preflight, admission, every typed ticket exchange,
  target-ticket replay/wrong-purpose/wrong-room, raw-bearer rejection, read-only
  denial, normal posting, profile SSoT, preferences, leave, exact revoke, kick, and
  room close.
- Real WebSocket tests prove initial snapshot readiness, normal/read-only command
  behavior, revoked-ticket denial, and immediate connected-session close. Races use
  barriers or controlled channels, never arbitrary sleeps.
- Each implementation commit remains independently buildable, verifiable, and
  rollbackable. `git diff --stat` is checked before commit and a 1,000-line change is
  split unless one documented invariant makes that impossible.
- Before a public-invite UI is called complete, packaged frontend Computer Use runs
  the actual host and a separate real browser through one-use and reusable normal and
  read-only flows, including avatar, reload, profile edit, preferences, posting
  denial, leave, revoke, and restart. All started verification resources are stopped
  and temporary outputs are removed or moved to recoverable Trash.
- Every material optimization appends its observed prior cost or threat, intent,
  preserved product/security contract, trade-off, and measured verification to this
  file or `docs/VERIFICATION.md`. Unsupported surfaces remain listed in the exposure
  map rather than reported as parity.
