# Human Invite, Admission, and Room Session Slice

Status: revised design candidate; no Rust admission route is active

## Definition

This slice establishes one durable authority for a human browser invite, admission,
profile binding, room membership, and expiring room session. It then lets that live
session exchange for exact one-use WebSocket, profile, attachment, and preference
grants. Invite management remains local-operator authority. This slice does not make
an external invite reachable until the separate trusted public-ingress owner is
complete.

The behavior comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`. The Rust baseline for this design is
`7905a0b`. Current behavior below was confirmed from the original invite routes,
admission coordinator, invite/session repositories, attachment routes, public-ingress
checks, copied frontend controller, and real product flows. Old product markdown is
not an authority.

## Current reachable contract

- A current human invite is room-bound, expiring, and either read/write or read-only.
  The copied UI offers configured `max_uses` values 1, 5, and 0 and defaults to one
  use and 24 hours, but the moderator API also currently accepts other integers such
  as 2 and 3. Negative input normalizes to 0. Configured value 1 is one-use; every
  other nonnegative value is reusable. Its effective distinct-principal ceiling is
  `min(configured_max_uses or 128, 128)`, while public JSON retains the configured
  value. Rust preserves that split rather than narrowing the reachable API to the UI
  presets.
- Invite preflight does not write. It distinguishes invalid or expired invites,
  agent-only links, a live same-room session, a known reusable-link device, an
  already joined member, and a browser that must supply a profile.
- Admission requires a canonical nonzero UUID request ID and one canonical durable
  browser credential. Exact retry returns the same admitted identity and bearer.
  Reusing the same admission identity with different admitted input conflicts.
- A successful admission consumes invite capacity, creates or resolves the human
  identity, joins the participant, creates the session, and emits the canonical
  participant-joined event. A collision with a different profile-bound participant
  is rejected before invite consumption.
- Browser session bearers use the `aas1.` prefix, expire after one hour, are stored
  only as SHA-256 fingerprints, and are revoked on exact session revoke, participant
  leave or kick, room close, or credential revoke. A connected WebSocket stops being
  authorized when its backing session is revoked or expires.
- A normal session can post; a read-only session cannot. Both can read the canonical
  room and read or patch every existing person-owned `UserProfile` field. Read-only
  cannot upload new avatar bytes, but profile patch may clear the avatar or select an
  available attachment already owned by that user. Only display name and avatar are
  projected into room participants. Profile `mic_muted` and `deafened` remain
  person/device presentation state: they never alter room mute, role, membership, or
  command capabilities, which remain room-owned.
- A pre-join avatar upload is optional and bounded. Custody belongs to the exact
  invite-and-browser-credential subject, while all browsers using one invite share
  that invite's quota. It supersedes only that custody subject's older pending
  avatar, expires after one hour, and becomes the admitted user's profile avatar
  only during successful admission. An invalid request reference, missing row,
  custody mismatch, or expired optional avatar is treated as omitted: admission
  still commits, but it cannot claim unrelated media or create partial profile state.
  A stored invariant violation or corrupt canonical image is an authority failure,
  not optional absence.
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

- `room_invites` owns the current public 16-character lowercase-hex invite ID,
  complete 32-byte fingerprints for both current invite credentials, room, base
  participant and display identity, scope, configured maximum uses, use count,
  expiry, revocation, creator, and creation time. The public ID is the first 16 hex
  characters of the signed-token SHA-256, while the full digest remains the unique
  lookup authority. A generated classification derives `one_use` exactly
  when configured maximum uses is 1 and `reusable` otherwise; it is not a second
  writable policy value. The effective use ceiling is the deterministic capped
  function above, not a second stored policy value. Raw invite tokens are never
  persisted.
- `human_device_credentials` maps only reusable-link device fingerprints to one
  stable human user. One-use admissions do not create this mapping.
- `human_room_sessions` owns both the admission result and session: unique admission
  key, first canonical request UUID, invite, payload hash, session fingerprint,
  exact room/user/participant, browser client kind, invite scope, browser credential
  fingerprint, optional reusable-identity fingerprint, bounded public result,
  admission/expiry time, and active/ended state. Raw session tokens are never
  persisted. A partial unique constraint over stored `active` rows permits at most
  one live human session for each `(room_id, participant_id)`. Wall-clock expiry is
  authoritative even before its stored state is materialized as `ended`; admission
  materializes only the expired rows relevant to its exact key or resolved
  participant before the partial unique constraint can be reached. Keeping the retry
  result in this row avoids a fourth admission-results table and prevents
  session/result lifecycle drift.
- A completed but later expired, replaced, or revoked admission remains a terminal
  `admission_session_unavailable`; exact retry never creates a replacement session.
  The deterministic issuer reproduces the bearer only while that exact row is live.
  A one-use request-key row is never deleted by routine cleanup in this slice: its
  admission key, payload hash, and terminal outcome remain the authority for exact
  unavailable and changed-payload conflict results even after its invite is used,
  expired, or revoked. A reusable row may be removed after its invite is terminal
  because the original reusable path applies that current-invite gate before lookup.
- Composite schema keys bind every session to one existing invite's exact
  `(invite_id, room_id, scope, generated key kind)`, one profile's exact
  `(user_id, participant_id)`, and, for reusable rows, one device credential's exact
  `(fingerprint, user_id)`. Separate existence foreign keys are insufficient because
  a cross-bound durable row would otherwise become the authority used by every later
  target revalidation. The generated parent value prevents a child from classifying
  a reusable invite as one-use and thereby bypassing the reusable credential binding.
- `profile_attachments` remains the single human-avatar asset owner. Its state
  constraint permits either a user-owned pending/bound image or an admission-pending
  image. The latter stores separate fixed-size custody and invite-quota fingerprints.
  Admission atomically transfers a valid pending image to the new user and binds it,
  while retaining immutable invite accounting provenance. The committed image stops
  counting against pending-room quota, continues to count once against room/runtime
  totals, and is not retroactively charged to the user's ordinary uploader quota,
  matching the original metadata transition. No second filesystem store or duplicate
  image decoder is introduced.

Token fingerprints, admission keys, payload hashes, and pending-upload subjects are
fixed 32-byte blobs. IDs used in public JSON remain their canonical text forms. The
browser credential is exactly `aad1_` plus 32 WebCrypto/operating-system-random bytes
encoded as unpadded base64url. The browser persists that single value in durable
origin storage; missing WebCrypto, failed durable storage, a weak/malformed value,
or per-page regeneration makes admission and pre-join upload visibly unavailable.
There is no date, `Math.random`, short-token, or ephemeral-memory fallback. A one-use
admission binds this credential for proof/retry custody but never registers it as a
reusable identity.

Human invite creation preserves both current credentials. `invite_token` is the
signed `aai1.<claims>.<HMAC-SHA256>` value, while `join_code` is `aaj1_` plus exactly
24 operating-system-random bytes encoded as unpadded base64url; `join_url` carries
the latter. Both are accepted by browser admission, resolve the same durable invite,
and remain distinct opaque values. The signed claims retain the current schema,
room and display identity, URLs, expiry, nonce, and permission fields; successful
verification must also find the exact current row and match its canonical authority
fields before admission. No rowless signed-token or old-token compatibility path is
introduced.

The existing permission-checked host-key owner supplies the persisted 32-byte HMAC
key for both current invite signatures and deterministic sessions, matching the
original single invite-secret owner. The fixed `aai1.` signing input and fixed
session-bearer context are disjoint HMAC message domains; no second secret, cache, or
derivation layer is needed. A small non-serializable, non-debuggable session issuer
uses that key without reusing the Ed25519 host key or process-local host-control
secret. Raw invite/session credentials and key material never enter logs, events, or
idempotency JSON; only ordinary SHA-256 fingerprints are durable.

The bearer is exactly `aas1.` plus unpadded base64url of the complete 32-byte
`HMAC-SHA256(session_key, "agentsassemble-human-session-bearer-v1\0" ||
admission_key)`. The admission key is always one fixed 32-byte value, so this
transcript has no variable-field ambiguity. The 43-character encoded body and
48-character complete bearer preserve the actual human-admission path, which calls
the original deterministic `ensure_for_request()` rather than the generic random
`issue()` method. The stored session fingerprint is SHA-256 of the complete ASCII
bearer including `aas1.`. The issuer runs only for a newly
committing or exact live admission row; terminal rows never receive a replacement
bearer.

The persistent host-identity envelope creates that session HMAC key only with a
fresh host identity. Loading an existing envelope with a missing, short, or invalid
session key fails closed; it never regenerates or derives a key and thereby silently
invalidates retry results. Existing file ownership, canonical-path, `0600`, symlink,
and non-serialization protections apply to the combined envelope.

Invite create and revoke use separate, one-use local-private-control grants bound to
the exact room and exact operation. The route consumes the grant before reading a
bounded body, and the room transaction revalidates the current local user/profile,
room membership, and `room.manage` capability. A configured or trusted public
ingress proves transport custody only and is never invite-management authority.

## Admission transitions

Preflight reads the invite, optional current bearer, and canonical browser credential
without allocating durable state. A presented session counts as `existing_session`
only when its durable row, expiry, room, active membership, participant/profile
binding, client kind, and scope all remain valid.

The copied browser keeps one room session globally and sends it while preflighting a
new invite whose room is not known yet. Therefore a bearer with no durable row or a
valid row for another room is not current-room session authority; preflight continues
with the independently durable browser credential exactly as the original route did.
Once the exact bearer resolves to a row for the current room, an expired, ended, or
inactive-membership row is `session_unavailable` and never falls through to the
browser credential. A valid existing-session result returns the immutable stored
session scope, not the scope of the invite currently being inspected.

Every authorization and capacity decision treats a row as live only when its stored
state is `active` and `expires_at` is later than the transaction's fixed current
time. SQLite cannot put the moving wall clock into the partial unique predicate, so
the admission transaction materializes expiry only where it matters: an expired
exact retry row is changed to `ended` before returning
`admission_session_unavailable`, and expired active rows for a newly resolved
`(room_id, participant_id)` are changed to `ended` before capacity and insertion.
An exact-retry expiry transition commits even though its product result is terminal
rejection; a database/infrastructure failure still rolls it back. A distinct new
admission commits the relevant expiry transition and new session together.
Preflight remains read-only and rejects time-expired rows without depending on that
materialization. Ticket exchange, WebSocket validation, and all target units also
reject time-expired rows immediately; cleanup is never required for authorization.

After bounded envelope validation, the admission route submits one request to the
existing bounded `RoomRuntime` writer. Queue acceptance transfers custody to the
runtime: disconnecting or cancelling the HTTP handler cannot cancel an accepted
admission. The room runtime then performs one SQLite transaction:

1. validate the bounded request, derive its payload hash and one-use candidate key,
   and load invite metadata without yet treating current invite usability as a new
   admission decision;
2. if that exact one-use row exists, reject a payload conflict; materialize an
   expired active row as `ended` and reject any terminal backing session, otherwise
   return its live result before current invite expiry,
   revocation, used-nonce, maximum-use, or new-session capacity checks. This retains
   the original request-workflow recovery after a response was lost;
3. for every new admission and every reusable retry, enforce the active room plus
   current invite expiry, revocation, client kind, scope, and maximum-use gates;
4. derive the reusable invite/credential key when applicable. Reject a conflicting
   row; materialize an expired active row as `ended` and reject any terminal row, or
   return its exact live result without another use, capacity slot, identity, or
   event. The current invite gate remains before this lookup, as in the original
   reusable-device path;
5. resolve the reusable credential user or allocate an invite-scoped one-use user,
   keeping participant/profile collisions fail-closed; materialize expired active
   rows for that exact `(room, participant)` as `ended`, then enforce global/per-room
   public-session capacity using the live predicate while excluding an existing
   same-participant session;
6. consume one invite use when this is a new invite principal, upsert the joined
   human participant and matching profile, and mark any different live session for
   that `(room, participant)` replaced without charging another capacity slot;
7. omit an optional pending avatar whose reference is invalid or whose row is absent,
   expired, or owned by another custody subject; fail on a persisted invariant or
   content-integrity violation; otherwise transfer the exact valid row, create the
   session/result, and append at most one `participant_joined` event. Its sequence is
   pending whenever it is newer than the room's existing durable publication cursor;
8. commit before publishing the event, notifying displaced sessions, or returning
   the bearer.

Any database/infrastructure failure rolls back every step. Optional-avatar semantic
invalidity omits only that avatar as described above; it is not converted into a
database-success fallback. Persisted corruption is never classified as semantic
invalidity. Because all affected records share one SQLite transaction,
the Python coordinator's separate JSON invite
repository, identity database, room repository, workflow journal, compensation,
and resume saga are not reimplemented.

For a one-use invite the admission key binds the exact presented invite-credential
fingerprint, browser credential fingerprint, and canonical request UUID. This
credential binding prevents a second invite holder who guesses or obtains only the
request UUID from recovering the deterministic session bearer. For a reusable invite
the key binds only the exact presented invite-credential and browser-credential
fingerprints: request UUID is deliberately excluded, while the first request UUID
remains audit/result data. Thus the same credential/device and same payload returns
the original identity, result, and live bearer even with a new request UUID, without
consuming another use; deliberately switching between the separately exposed
`aai1` and `aaj1_` credentials retains the original distinct admission identity. A
changed payload conflicts and a different device is a distinct reusable principal.
The payload hash covers every
field that can change identity or membership, including display name, client ID,
participant type, and optional avatar reference. Client input never chooses user ID,
participant ID, capabilities, role, mute state, or session expiry.

At most one human room session is live for `(room_id, participant_id)`. A new
admission through another invite for the same stable participant replaces the old
row in the transaction. Capacity excludes that same-participant row; after commit
the displaced fingerprint's grants and sockets fail durable revalidation and receive
a server-owned revocation notification.

The current public capacity is preserved: at most 448 live public sessions globally
and 112 per room, with at most 128 distinct reusable-link principals. The remaining
original 64 global and 16 per-room slots are reserved for later separately owned
operator and external-agent sessions, not borrowed by this human slice. Capacity
queries use the same stored-active-and-unexpired live predicate as authorization;
an unmaterialized expired row never consumes a slot.

## Session-derived grants and revocation

Raw room-session bearers are accepted only by the admission preflight, leave/revoke,
and typed session-exchange endpoints. Target profile, attachment, preference, and
WebSocket routes never interpret a raw session bearer as a purpose ticket.

The session exchange surface is a closed set of typed routes rather than a
client-selected purpose string:

- WebSocket connect;
- own profile access, used by profile read/update and normal-scope avatar upload;
- room preference read and, for normal scope only, preference write;
- room attachment upload and exact private-attachment read when the corresponding
  message behavior is active.

Read-only room scope denies posting, preference mutation, profile-avatar upload, and
every room attachment upload. It still permits the human to read and patch their
complete existing person profile because that profile is not room role or posting
authority. An `avatar_image_url` patch can clear the avatar or bind only an available
attachment already owned by the same user; it cannot create or claim media. Banner,
accent, status, `mic_muted`, and `deafened` remain person-profile fields and never
become room authority. Only display name and avatar changes are projected into room
participants.
Bound profile-avatar reads retain the current unguessable public attachment URL and
do not mint or consume a session-derived avatar-read grant, so other room members can
render the avatar. Unexpired pre-admission preview remains exact invite/credential
custody rather than admitted-session authority.

Every derived grant retains immutable session-fingerprint provenance plus the exact
room, user, participant, client kind, scope, and purpose. Grant consumption removes
the in-memory item first and then revalidates the durable session and current
room/profile/membership binding. Wrong purpose, wrong room, replay, expiry, or
session-only revocation consumes and rejects the grant. Existing local-operator
typed grants remain separate and unchanged.

A derived grant never outlives its backing session. A target write consumes the
grant before reading its bounded body, then revalidates the session, membership,
profile binding, and operation capability inside the same SQLite transaction that
commits the write. A target read performs the same durable validation in the read
unit immediately before selecting its result. The in-memory provenance is a lookup
key and claimed ceiling, never authority on its own.

The existing 4,096-item grant store remains one implementation owner, not a second
session cache. Grants from this public-human slice have hard sublimits of 1,792 total
and 8 outstanding per session fingerprint, leaving at least 2,304 entries for
local/private authority. Expired and consumed grants are reclaimed by the existing
store owner. No new request-rate limiter is introduced in this slice: the existing
128-connection HTTP admission bound, 4,096-item store capacity, public/private
partition, and per-session outstanding bound are observed controls, while an
additional requests-per-minute threshold would be an unmeasured product restriction.

A session WebSocket subscribes to revocation notification before its connect grant
is consumed, then revalidates the database after subscription. It revalidates before
each client command and before every outbound product frame. Its connection task owns
an expiry-deadline timer that performs durable revalidation and closes at expiry; the
timer never extends validity. A post-commit session/participant/room revocation
broadcast closes an idle socket. Broadcast lag or closure triggers durable
revalidation and fails closed. The database remains authority; the broadcast and
deadline are only revalidation triggers, not independent validity sources.

The final durable outbound check is the authorization linearization point. A revoke
or replacement committed before that check denies the frame. If it commits after the
check, only the already-authorized in-flight frame may complete; notification closes
the socket and every later frame revalidates. The implementation does not hold a
SQLite transaction across socket I/O or add a second per-session lock/cache to claim
an impossible atomic database-and-network send.

Frame-level validation is not commit authority. Every session-originated command
carries the immutable session fingerprint and admitted scope beside the resolved
principal through the bounded `RoomRuntime` queue. The exact SQLite mutation unit
revalidates that fingerprint as active and unexpired and checks its room, user,
participant, profile, membership, and scope before any command state transition.
Local/private commands retain their existing separately typed authority. Therefore a
revoke or replacement committed after frame validation but before dequeued mutation
causes that mutation to fail closed even when the participant remains joined.

Leaving performs membership-left transition, revokes that participant's room
sessions, and appends the canonical event in one transaction. Exact session revoke
does not remove membership. Kick and room close revoke affected sessions in their
own canonical room transactions. Notifications happen only after commit.

The room runtime, not an HTTP request task, owns post-commit publication and
revocation notification. Admission reuses `room_event_publication_cursors` as the
existing per-room published-sequence watermark, not as a per-event outbox row. The
transaction appends the canonical event; runtime/startup drains select newer event
sequences and advance the cursor only after broadcast offer. Handler cancellation
can lose a response
but cannot lose an accepted commit-to-publication handoff; exact retry recovers the
stored result. A crash between broadcast offer and cursor acknowledgement may offer
the same sequence again after restart; sequence-aware subscribers tolerate that
at-least-once delivery, and cursor replay never creates a second durable event.

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

### Dual current credentials without duplicate durable authority

- Prior cost and observed behavior: original browser invite create exposes a signed
  `aai1.<claims>.<HMAC>` `invite_token` and an independent `aaj1_` `join_code`; the
  UI copies the latter through `join_url`, but the browser `/join` endpoint also
  accepts the former. Removing `aai1` would contract a reachable direct-HTTP path,
  not merely delete an unused native-client implementation. The original separately
  stores copied claims and another `join_nonce` beside those credentials.
- Change intent: preserve both raw credential formats and response fields, but let
  one canonical invite row own current policy. Store the complete signed-token and
  join-code SHA-256 fingerprints in separate unique BLOB columns. The signed token
  uses the already persisted HMAC key with its disjoint `aai1.` message domain; the
  join code retains exactly 24 random bytes. The signed token's own nonce remains in
  its claims, while the canonical one-use row and terminal admission result replace
  the redundant stored `join_nonce` replay authority.
- Preserved contract: `invite_token` remains signed `aai1`, `join_code`/`join_url`
  remain independent `aaj1_` with a 32-character body and 37-character total shape,
  both can drive browser admission, and the public invite ID remains the first 16 hex
  characters of the signed-token digest. Binding idempotency to the exact presented
  credential preserves the current distinction when a reusable caller switches
  between them. Current room, expiry, revocation, scope, client kind, and use limits
  still come from the durable row; no previous token is imported or reinterpreted.
- Observed cost: creation performs the current signed-claims nonce fill and 24-byte
  join-code fill, one HMAC-SHA256, two token SHA-256 hashes, and two indexed BLOB
  inserts. It avoids a third random `join_nonce`, copied durable claims, and another
  replay set. `aaj1_` preflight performs one indexed lookup. `aai1` additionally
  performs bounded decode plus one HMAC verification and then the same indexed
  current-row lookup. No CPU, disk, or latency improvement is claimed until the
  actual route is measured.
- Verification: fixed vectors lock signed claim names, `aai1` signature input,
  constant-time signature comparison, `aaj1_` canonical 24-byte decoding, both
  fingerprints, public ID derivation, and distinct response values. Database
  inspection finds neither raw credential nor copied signed claims. Browser tests
  admit through each create response field and the join URL, while tamper, malformed
  encoding, row/claim mismatch, revoke, expiry, wrong room, and every configured
  use-limit boundary fail from their exact current authority. No generic token parser,
  legacy reader, or fallback branch is added.

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
  ambiguity while retaining maintained SHA-256/HMAC implementations. Every authority
  column also checks `typeof(value) = 'blob'`; SQLite BLOB affinity alone accepts
  TEXT, and TEXT/BLOB values compare as different storage classes under uniqueness.
- Preserved contract: only fingerprints are durable and comparisons remain exact;
  public IDs and bearer formats do not change.
- Verification: schema constraints reject the wrong length and a 32-character TEXT,
  accept exactly 32 BLOB bytes, database inspection finds no raw bearer, and
  restart/exact-retry tests reproduce the same token.

### Dedicated persisted session issuer key instead of key reuse or regeneration

- Prior threat and cost: the current permission-checked host envelope owns only its
  Ed25519 signing key. Reusing that private key for HMAC would violate key separation;
  using process-local randomness would make a committed admission's deterministic
  bearer unrecoverable after restart. Generating a missing key while loading an
  existing envelope would silently create the same failure across every durable
  session row. Syntax and length checks alone also accept a different canonical
  32-byte key substituted into an otherwise valid envelope; the unchanged Ed25519
  public-key binding would not detect that authority change.
- Change intent: create one independent 32-byte operating-system-random HMAC key only
  with a fresh host identity and store it in the same versioned private envelope.
  Loading an envelope without one exact canonical 32-byte key fails closed. There is
  no derivation from Ed25519 material, migration, compatibility reader, or secondary
  secret file. The existing `runtime_host_identity` row stores only SHA-256 of that
  key beside the Ed25519 public key, and every reopen compares both bindings before
  admitting the store.
- Preserved contract: the existing initialization-nonce binding, create/reuse policy,
  canonical-path and symlink checks, single-link regular file, private-directory and
  `0600` requirements, 512-byte envelope bound, and one write plus `fsync` remain the
  authority. The key never enters the database, public identity, events, logs,
  serialization, or generic debug output.
- Observed cost: the live host material grows by exactly 32 secret bytes. Canonical
  unpadded base64url adds 43 payload characters plus one JSON field inside the
  existing bounded envelope; the database singleton grows by one 32-byte non-secret
  fingerprint. Creation adds one system-random fill and one SHA-256, reopen adds one
  SHA-256 and reads the fingerprint in the existing identity query, with no extra
  file open, database query, write, or disk synchronization. No CPU or latency
  improvement is claimed.
- Verification: fresh creation and exact reopen return the same private issuer key;
  two fresh hosts differ; a missing, malformed, noncanonical, or wrong-length field
  and an older envelope version are rejected without rewriting the file. Replacing
  only the field with another canonical 32-byte key also fails the database binding
  without rewriting the envelope. Existing permission, symlink, hard-link,
  nonce-binding, and interrupted-initialization tests continue to pass. A later
  issuer test proves the same durable session input yields the same bearer across
  store reopen.

### Deterministic bearer recovery without persisted raw tokens

- Prior cost and correctness threat: the reachable original human-admission path
  derives a complete 32-byte HMAC from its workflow identity and stores only the
  final token fingerprint. Rust must return the exact bearer after a lost committed
  response without persisting that bearer or adding a second result table. Using the
  generic random issuer, or truncating the MAC to that issuer's 24-byte random shape,
  would change the reachable bearer contract and no longer match the durable session
  fingerprint.
- Change intent: use the fixed transcript above with the dedicated persisted HMAC
  key, retain the actual admitted-human full-MAC/43-character body shape, and hash
  the final ASCII bearer once for the durable lookup fingerprint. A small issuer and
  issued-bearer value implement neither `Debug` nor serialization; no generic token
  framework or configurable transcript is introduced.
- Preserved contract: the public prefix and token shape, one-hour session expiry,
  opaque bearer treatment, fingerprint-only persistence, exact live retry, and
  terminal unavailability remain unchanged. A database-only copy still cannot mint
  or recover a bearer, while the matching host envelope can recover exactly one.
- Observed cost: each new or exact-live response performs one HMAC-SHA256 over one
  fixed context plus 32 bytes, encodes 32 bytes, and performs one SHA-256 over the
  resulting 48 ASCII bytes. It allocates only the returned bearer string and fixed
  stack buffers; there is no RNG call, database read, cache, file I/O, or additional
  durable column in the issuer itself. No performance gain is claimed without
  measurement.
- Verification: fixed vectors lock the transcript, complete 32-byte MAC,
  43-character body, 48-character full token, and fingerprint; different keys or
  admission keys differ; malformed input
  cannot enter the fixed-size API. Reopening the same store reproduces the exact
  bearer and fingerprint, while a fresh host differs. Tests and diagnostics never
  format or serialize bearer/key-containing values.

### Composite authority bindings instead of repository-only correlation

- Prior threat: independent invite, room, scope, user, participant, key kind, and
  reusable credential fields prove only that each value exists. A writer bug could
  combine a read-only invite from one room with read/write scope or another room's
  participant. It could also label a reusable invite as one-use, use SQLite's nullable
  composite-FK rule to omit the device credential, and make cleanup and uniqueness
  trust that false classification. Every later authorization would then trust the
  corrupt session row consistently.
- Change intent: add only the redundant composite unique parent keys required by
  SQLite and composite session foreign keys for invite/room/scope/generated key kind,
  user/participant, and reusable credential/user. The invite key kind is generated
  directly from configured maximum uses, so no second writable state or trigger is
  introduced. Repository validation remains but is no longer the sole durable
  cross-binding defense.
- Preserved contract: admission still creates the same invite, profile, participant,
  and stable reusable identity; one-use rows remain independent of the reusable
  credential table. Room deletion keeps its explicit cascade/purge behavior.
- Trade-off: the redundant unique indexes consume a small fixed amount per authority
  row and add index writes at admission. That cost protects the concrete privilege
  and identity cross-binding threat without triggers or a second authority model.
- Verification: schema tests reject invite/room, invite/scope, invite/key-kind,
  user/participant, and reusable credential/user mismatches before repository code
  runs; matching one-use and reusable rows still insert and obey the active-participant
  partial unique key. Invite limits 1 and 0/2/5/>128 exercise both generated classes.

### Event-driven revocation without periodic session polling

- Prior cost and threat: polling every live socket adds database reads and latency
  even when no revocation occurs, while notification alone has a subscribe/consume
  race and is not durable authority.
- Change intent: subscribe before grant consumption, revalidate after subscription,
  revalidate each inbound command and outbound product frame, own an expiry-deadline
  revalidation, and broadcast only after durable revocation commit.
- Preserved contract: revocation after ticket issue or during an idle connection
  invalidates the exact session promptly; a missed/lagged notification fails closed.
- Trade-off: outbound validation adds one indexed session/membership lookup per
  recipient frame. That cost is accepted for the concrete threat of a revoked or
  replaced human receiving a queued event; no unmeasured cache is introduced.
  Process restart closes the socket and reconnect must revalidate the database.
- Verification: deterministic barriers cover revoke-before-consume,
  revoke-after-connect idle close, deadline expiry, notification lag/closure,
  inbound-command and outbound-delivery races; tests do not sleep or inspect private
  maps. Query count and end-to-end fanout latency are recorded before and after.

### Shared grant-store limits instead of a second session ticket cache

- Prior cost and threat: the current grant store is globally bounded at 4,096, but
  an exchange endpoint without provenance sublimits lets public sessions occupy all
  slots and starve private control. The server already bounds admitted HTTP
  connections at 128; no measured exchange-rate or CPU/lock-latency result supports
  a second per-minute threshold.
- Change intent: keep the existing store and add only provenance accounting, pending
  sublimits, and a fixed local/private reserve.
- Preserved contract: grants stay opaque, short-lived, exact-purpose, one-use, and
  consume-on-wrong-purpose; local/private issuers keep a reserved capacity floor;
  ordinary clients gain no new requests-per-minute rejection behavior.
- Trade-off: a stolen live session may keep churning grants within the existing HTTP
  work bounds, but cannot hold more than eight or cross the public partition. A rate
  limiter requires measured CPU/lock/latency evidence and a separately reviewed
  product limit rather than a speculative threshold in this migration slice.
- Verification: boundaries prove 8-per-session, 1,792-public total, the 2,304-entry
  private reserve, and expiry/consumption reclamation. Before/after issue latency and
  lock time are recorded; a later limiter is considered only if those measurements
  demonstrate a concrete exhaustion path not covered by the existing bounds.

### Targeted expiry materialization instead of a session sweeper

- Prior cost and correctness threat: the original session owner deletes expired
  records during verification and active-session enumeration. Rust retains terminal
  admission results, but a partial unique index over stored `active` state cannot
  observe wall-clock expiry. Merely filtering reads would leave an expired row able
  to block the same participant's later admission, while counting state alone would
  also charge expired rows to capacity.
- Change intent: all authority and capacity queries use `state = active AND
  expires_at > now`; an admission write changes only its expired exact-retry row and
  expired rows for its resolved `(room, participant)` to `ended` before terminal
  return, capacity, or insertion. The update uses the admission-key and
  room/participant indexes already required by those operations.
- Preserved contract: preflight performs no writes; an expired bearer resolved to
  the current room fails immediately; an expired completed admission remains terminal
  and exact retry never creates a replacement; a different valid admission for the
  same stable participant is not blocked or charged by stale time-expired state.
- Trade-off: unrelated tombstones remain durable and bounded by invite lifecycle.
  This avoids periodic database reads, a background task, and full-table cleanup in
  the latency path. No claim of lower CPU or disk cost is made until measurements
  exist.
- Verification: a controlled clock expires a stored-active same-room session without
  cleanup, proves preflight and ticket exchange reject it, then admits the same participant
  through a distinct valid admission and proves the old row is `ended`, the partial
  unique constraint does not fail, capacity is unchanged, and exact retry of the old
  admission remains `admission_session_unavailable`. Query count and transaction
  latency are recorded for the targeted update.

### One-use terminal authority instead of cleanup-induced gate drift

- Prior cost and correctness threat: the original request-key workflow is checked
  before current invite gates. Deleting its terminal record after a one-use invite
  becomes used, expired, or revoked changes an exact retry from the stored
  `admission_session_unavailable` or payload conflict to a later invite-gate error.
  A one-use invite can be terminal immediately after its first successful use, so
  invite terminality is not a safe deletion condition.
- Change intent: retain the same `human_room_sessions` row as the one-use terminal
  authority. Routine cleanup never deletes it. Reusable tombstones remain separately
  eligible only after their backing invite is terminal because their current gate is
  evaluated before lookup in the original reachable path.
- Preserved contract: exact one-use retries and changed-payload retries return the
  same terminal result across restart and unrelated cleanup-triggering writes; no
  deleted tombstone can reopen admission or shift the deciding authority.
- Trade-off: durable row count grows by one fixed-size row per one-use admission,
  matching the original retained workflow rather than inventing a retention window.
  No compaction table or expiry policy is added without measured page growth. A
  later in-row compaction is acceptable only if it retains admission key, payload
  hash, key kind, and terminal outcome and demonstrates lower disk cost without a new
  lifecycle owner.
- Verification: after session expiry, revoke, and replacement, trigger every bounded
  cleanup write and restart the process; the exact key still returns unavailable,
  the same key with a changed payload still conflicts, and SQLite page/row growth per
  terminal admission is recorded.

No additional cache, repository interface, background cleanup framework, generic
credential provider, multi-database saga, or future agent-session abstraction is
authorized by this slice. Expired rows are rejected authoritatively and only the
request-relevant rows described above are materialized as ended. Admission
tombstone cleanup is key-kind aware: routine work never removes a one-use
request-key tombstone, while a reusable tombstone becomes removable only after its
backing invite is terminal so its current gate prevents re-admission. Expired pending
attachments may be reclaimed by the same bounded write-path work. Exact attachment
authorization checks only its target row and never invokes a global expired-row
delete. A separate cleanup task or smaller batch requires measured transaction and
disk evidence; the current store-wide worst case remains capped by the existing
4,096-item runtime limit rather than described as unbounded.

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
  failure; exact retry is stable across restart; a one-use exact retry is governed by
  its stored backing session rather than later invite expiry/revocation/use state; a
  conflicting or unavailable stored session is terminal; a reusable exact retry
  with a different request UUID returns the same identity/live bearer without
  another use or event only while the current invite gate remains valid; changed
  payload conflicts and a different device consumes a distinct principal; collision
  and all capacity edges fail before consumption; and no raw invite, device, or
  session bearer reaches SQLite, events, logs, or fixtures.
- Controlled-clock persistence tests leave an expired session stored as `active`,
  prove every read/authorization/capacity path treats it as unavailable, then admit
  the same participant through a distinct valid admission and prove the admission
  transaction changes only the relevant expired row to `ended` before capacity and
  insertion. The stale row neither consumes capacity nor violates the partial unique
  constraint, and its exact retry remains terminal.
- Lost-response tests consume the final one-use invite, drop the first HTTP result
  after commit, and prove the exact request-key retry succeeds while its backing
  session remains live. Separate reusable tests prove the original ordering: an
  existing device-key result is rejected after invite expiry, revocation, or use
  ceiling, while a new admission receives the same current-gate error.
- Invite-limit tests preserve configured `max_uses` values 0, 1, 2, 3, 5, 128, and
  greater than 128 in public results while enforcing effective ceilings 128, 1, 2,
  3, 5, 128, and 128 respectively. Negative create input normalizes to configured 0;
  the UI presets remain 1, 5, and 0 without becoming a database allow-list.
- One-use tombstone tests end the backing session by natural expiry, exact revoke,
  and replacement, then trigger unrelated bounded cleanup and restart. Exact retry
  remains `admission_session_unavailable`, changed-payload retry remains a conflict,
  and no cleanup path removes the request-key authority. Separate reusable tests
  prove removal is allowed only after the backing invite is terminal and can never
  reopen admission because its current gate precedes device-key lookup.
- Replacement tests admit one stable participant through different reusable invites,
  prove only the new session remains live, the old bearer/grants/socket fail, and the
  same-participant replacement does not increase capacity.
- Pre-join avatar tests prove exact invite/credential custody, replacement, one-hour
  expiry, safe-raster limits, failed-admission custody, and atomic ownership transfer
  to the same profile rendered in the room and lower-left panel. Boundary tests fix
  10 MiB per asset, 8 files/32 MiB per invite, 64/128 MiB pending per room,
  64/128 MiB per ordinary uploader, 512/1 GiB total per room, 4,096/8 GiB runtime,
  and prevent quota reset by changing browser credentials.
- Real Axum tests exercise create, preflight, admission, every typed ticket exchange,
  target-ticket replay/wrong-purpose/wrong-room, raw-bearer rejection, read-only
  full person-profile patch success, read-only profile-avatar upload/room-upload denial,
  same-user existing-avatar bind/clear, foreign-avatar rejection, public bound
  profile-avatar read, and proof that profile mic/deaf fields do not mutate room mute
  or capability; normal posting, profile SSoT, preferences, leave, exact
  revoke, kick, and room close. Invite management tests prove consume-before-body,
  room/purpose binding, transactional capability revalidation, and that ingress
  custody is not management authority.
- Real WebSocket tests prove initial snapshot readiness, normal/read-only command
  behavior, revoked-ticket denial, expiry close, outbound denial when revoke commits
  before final outbound validation, and immediate connected-session close. Races use
  barriers or controlled channels,
  never arbitrary sleeps. A barrier revokes or replaces the exact session between
  frame validation and the SQLite mutation UOW and proves no durable command state
  or event commits. Handler-cancellation and restart tests prove one durable
  canonical event and eventual publication-cursor acknowledgement after an accepted
  admission commit; replay may re-offer only the same sequence.
- Browser-unit tests prove exactly one `aad1_` credential is reused by preflight,
  pre-join upload, and admission; malformed stored values, unavailable WebCrypto,
  and failed durable storage stop before network I/O without generating a fallback.
  Host-identity tests prove a missing/invalid persisted session HMAC key fails closed
  and never regenerates over existing admission state.
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
