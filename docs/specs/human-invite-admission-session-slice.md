# Human Invite, Admission, and Room Session Slice

Status: atomic SQLite, bounded RoomRuntime admission, local HTTP preflight/join,
pre-join avatar flow, fail-closed browser credential custody, the live-session
profile/preferences/WebSocket exchanges, and exact participant leave are implemented
and production-browser verified; configured-manual and direct managed public ingress
are implemented and verified; stable entry, manager invite management, remaining typed
exchanges, and frontend activation remain incomplete

## Definition

This slice establishes one durable authority for a human browser invite, admission,
profile binding, room membership, and expiring room session. It then lets that live
session exchange for exact one-use WebSocket, profile, attachment, and preference
grants. Invite management remains local-operator authority. This slice does not make
an external invite reachable until manager control and frontend activation are
complete against one ready ingress snapshot.

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
- When a reusable admission actually changes the stable person's display name or
  avatar, the transaction reuses the existing concrete
  `project_profile_into_rooms` behavior: update only active joined human projections
  in every room, preserve each room's role/mute/owner/join state, and append each
  required `participant_updated` event. Admission does not add a generic projection
  trait or a second fan-out owner. Its target room still emits `participant_joined`
  only when the previous participant state was not joined.
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
- Profile and pre-join avatar ownership no longer share a state space. Their target
  tables, atomic admission transfer, cleanup, and common safety arithmetic are owned
  by [`asset-custody-lifecycle-slice.md`](asset-custody-lifecycle-slice.md). Invite,
  browser, and room provenance authorize the pre-join row only and do not remain as
  profile ownership after admission. No second filesystem store or duplicate image
  decoder is introduced.

Token fingerprints, admission keys, payload hashes, and pending-upload subjects are
fixed 32-byte blobs. IDs used in public JSON remain their canonical text forms. The
browser credential is exactly `aad1_` plus 32 WebCrypto/operating-system-random bytes
encoded as unpadded base64url. The browser persists that single value in durable
origin storage; missing WebCrypto, failed durable storage, a weak/malformed value,
or per-page regeneration makes admission and pre-join upload visibly unavailable.
There is no date, `Math.random`, short-token, or ephemeral-memory fallback. A one-use
admission binds this credential for proof/retry custody but never registers it as a
reusable identity.

Admission identity uses ordinary SHA-256, not the private session HMAC key. The two
fixed, unambiguous transcripts are:

```text
SHA256("agentsassemble-human-admission-key-one-use-v1\0"
       || presented_invite_fingerprint[32]
       || browser_credential_fingerprint[32]
       || canonical_request_uuid_bytes[16])

SHA256("agentsassemble-human-admission-key-reusable-v1\0"
       || presented_invite_fingerprint[32]
       || browser_credential_fingerprint[32])
```

The complete presented-credential fingerprint already distinguishes `aai1.` from
`aaj1_`, so no credential-kind byte or variable-length framing is added. Request UUID
input is outer-trimmed and then must equal its parsed lowercase hyphenated canonical
form and be nonzero; no UUID-version restriction is invented. The database retains
that 36-character form while the digest receives the UUID's 16 raw bytes. With an
invite fingerprint of 32 `0x11` bytes, browser fingerprint of 32 `0x22` bytes, and
UUID `123e4567-e89b-12d3-a456-426614174000`, the one-use digest is
`febebeab644e0344588b8c81db98427d32b8afdf4053cceadb92f8097ee24648` and the
reusable digest is
`dd65660dafacc7aad639e45d2236713262bccc4148710e264960e92306eca539`.

The admitted-input payload digest uses one fixed context and an unsigned 32-bit
big-endian UTF-8 byte length before every text field:

```text
SHA256("agentsassemble-human-admission-payload-v1\0"
       || field(meeting_id_assertion)
       || field(display_name_input)
       || field(participant_type_input)
       || field(owner_display_name)
       || field(client_id)
       || avatar_presence[1]
       || (field(attachment_id) when present))
```

The direct HTTP fields remain part of retry identity even when the copied frontend
does not currently use all of them. `meeting_id_assertion` is not authority: when
nonempty it must match the durable invite room. Each text value follows the original
room cleaner—replace CR/LF with spaces, trim, truncate by Unicode scalar count, trim
again—with limits 128/128/32/64/128 in the order above. Tabs and repeated internal
spaces are not collapsed and no Unicode normalization is introduced. The cleaned
request participant-type token is hashed before the human-only authority decision,
so accepted aliases do not silently merge distinct retry payloads. The original
browser join treats only `agent`, `ai`, `bot`, `subscription_ai`, `api`, `local`,
`remote`, and `unknown` as known nonhuman values. Tokens such as `browser`, `people`,
or any other cleaned unknown token therefore retain the original human coercion,
while their distinct cleaned input remains in the payload hash. A syntactically
invalid avatar reference canonicalizes to absent; a syntactically valid reference
hashes its attachment ID even when later optional custody lookup omits it. Absence is
byte `0x00`; presence is `0x01` followed by the framed ID. The vector
`general`, `홍길동\tGuest`, `human`, `Host`, `client-α`, and `avatar_1234` hashes to
`243c9e5901c07a27c4bd10abc081a1e6283e6a3f14c5c7a996d010a2ea375e65`.

Pre-join avatar custody is
`SHA256("agentsassemble-human-prejoin-avatar-custody-v1\0" ||
presented_invite_fingerprint[32] || browser_credential_fingerprint[32])`. Its quota
fingerprint remains the invite row's signed-token fingerprint so switching between
the two current invite credentials does not split one invite's quota. No raw
credential or secret enters any of these transcripts.

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

`SqliteStore` already owns the session HMAC key and derives a bearer only after its
admission transaction has selected a newly committing or exact-live branch. It does
not expose an open transaction, callback, issuer trait, second secret, or key cache.

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
   session/result, and append at most one `participant_joined` event containing the
   canonical full participant projection required by the existing frontend event
   contract. Its sequence is
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
The payload hash covers every field that can change identity or membership,
including the optional meeting assertion, display name, participant-type token,
owner display name, client ID, and optional avatar reference. Client input never
chooses user ID, participant ID, capabilities, role, mute state, or session expiry.

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
an expiry-deadline timer that closes at the durable expiry; the timer never extends
validity. A post-commit session/participant/room revocation
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
Until manager invite controls and frontend activation use the ready ingress snapshot,
the external-invite controls remain explicitly unavailable; a local preview is not
presented as admission parity. The token is removed from browser history after it is
captured, and stored session state cannot override a failed durable verification.

## Trusted ingress boundary

Managed Cloudflare direct-tunnel custody and startup-configured reverse-proxy proof
are implemented by the separate ingress slice. Stable entry, manager host controls,
and operator pairing remain incomplete. Forwarding headers or a configured URL alone
are not ingress authority. This slice may be exercised through the real local Axum
server, but that does not prove an external browser flow.

External invite creation remains disabled until manager controls consume one exact
ready origin/host/protocol snapshot from either the process-owned managed tunnel or
configured reverse-proxy owner. No raw legacy host token, local-development bypass,
query flag, or client-side readiness authority is added meanwhile.

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

### Fixed binary admission transcripts without secret-key coupling

- Prior cost and threat: the original workflow hashes JSON containing encoded
  fingerprints and request text. Reusing JSON would retain serialization work and
  leave field representation as implicit authority. Keying the admission locator
  with the private session secret would add HMAC work and couple durable idempotency
  identity to a secret even though possession of the locator grants no capability.
- Change intent: use the fixed SHA-256 transcripts and vectors above. Fixed-width
  fingerprint/UUID fields need no framing; variable payload fields use one explicit
  32-bit byte length. The private key remains confined to bearer derivation.
- Preserved contract: exact invite, browser credential, request identity, admitted
  inputs, and current `aai1.`/`aaj1_` distinction remain bound. Only a caller with the
  exact presented credentials can reproduce a locator, and reproducing it still
  cannot mint a bearer without the host's HMAC key.
- Observed cost: one-use identity hashes a fixed context plus 80 bytes; reusable
  identity hashes its context plus 64 bytes. Payload hashing streams five bounded
  text fields, one presence byte, and at most one bounded attachment ID. The design
  adds no RNG call, heap-built JSON, secret lookup, index, cache, or durable column.
  CPU or latency improvement is not claimed until the completed admission route is
  measured.
- Verification: fixed vectors pin contexts, NUL separators, field order, UUID raw
  bytes, UTF-8 byte lengths, tabs, non-ASCII input, and avatar presence. Boundary
  tests prove outer-trimmed canonical nonzero UUID acceptance, malformed avatar
  omission, distinct presented credentials, and changed payload conflict without
  logging raw inputs or secrets.

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
- Current implementation evidence: commits `6f91ab7`, `ee747ab`, and `5f44cec` place
  profile/device/membership/invite-use/session/result/event writes and deterministic
  bearer ownership under one `SqliteStore` transaction. A trigger-injected final
  session-insert failure leaves invite use, profile, device, participant, event, and
  session state unchanged. Exact one-use retry, reusable replacement, changed input,
  terminal expiry, event count, and raw-bearer exclusion tests inspect committed
  durable/public results. RoomRuntime dispatch, post-commit publication, and local
  HTTP activation are implemented by later separate commits.
- Security correction: manual review found that malformed durable participant or
  profile JSON could be mistaken for liveness loss and committed as `ended`.
  Commit `c99a031` validates exact room/participant bindings, human type, and profile
  revision before liveness, reuses that validated profile decoder before reusable
  profile patching, and returns `invalid_state` so the transaction rolls back.
  Commit `28d5d56` further makes only wall-clock expiry eligible for exact-row
  `ended` materialization. An active session paired with inactive room/non-Joined
  membership is impossible under the atomic lifecycle contract and fails closed
  without repairing state. Controlled corruption tests prove exact retry and
  reusable identity repair leave the live row and invite count unchanged.
- Observed implementation cost: the durable owner is split by responsibility into a
  664-line transaction module and a 276-line identity/avatar module, both below the
  mandatory 800-line source gate without exceptions. Optional avatar resolution
  selects metadata plus SQLite `length(content)` and does not return or decode the
  attachment BLOB in Rust, avoiding a Rust heap copy of an asset up to the existing
  10 MiB bound while still checking stored length equality. Bearer issue/recovery
  performs exactly one HMAC-SHA256, one unpadded base64url encoding, and one SHA-256
  fingerprint. No end-to-end CPU, latency, memory, or disk improvement is claimed
  before the complete browser flow is active and measured.

### Bounded HTTP adapter without transport-owned admission state

- Prior cost and threat: before `b32c2b7`, the complete SQLite and RoomRuntime owners
  had no reachable Rust HTTP entry point, so the copied browser's preflight and join
  requests received 404. Raw invite, browser, and optional session credentials must
  cross this boundary, but allowing them into a serializable command, generic error,
  log, or durable row would disclose replay authority. Reading an arbitrary request
  body before checking the fixed credential domains would also spend bounded body
  work on a request that cannot enter admission.
- Change intent: add only the two original request/response routes under one 16 KiB
  JSON limit and `private, no-store`. Preflight authenticates fixed-size `aad1_` and
  optional `aas1.` shapes before making owned copies or reading the body, then calls
  the existing read-only snapshot. Join authenticates the invite and browser
  credential in the adapter, constructs the existing non-debuggable prepared input,
  and submits it to the existing shared room queue. The HTTP task owns no workflow,
  identity, membership, event, retry, bearer, or publication state.
- Preserved contract: both current `aai1` and `aaj1_` invite credentials remain
  accepted; public preflight statuses retain `profile_required`, `known_user`,
  `existing_member`, `existing_session`, `invite_invalid`, and `invite_expired`;
  read/write scope remains public `room` and read-only remains `read_only`. Admission
  preserves the original response fields and adds the exact recovered `session_token`
  only after the room owner commits and publishes. One-use lost-response retry still
  precedes current invite gates, changed input is 409, capacity is 429, a missing room
  is 410, and queue saturation is an explicit 503 rather than a fallback path.
- Observed cost: every HTTP connection remains inside the existing process-wide
  128-connection admission bound and ten-second request-body deadline. Each request
  buffers at most 16 KiB once through the shared Axum decoder. The fixed header check
  hashes at most the 48-byte browser credential and optional 48-byte session bearer
  before copying them; the existing raw-authentication owner repeats those tiny hashes
  while producing the only fingerprints passed to persistence. A successful join
  moves the bounded result and bearer out of the post-publication commit rather than
  cloning them. No queue, cache, rate-limit map, task, database write, or transport
  fallback was added. No CPU, heap, disk, throughput, or latency improvement is
  claimed without representative browser measurements.
- Verification: a real loopback Axum test proves missing/malformed browser authority
  fails before persistence, malformed invite preflight is uniformly non-disclosing,
  the Tauri CORS header set includes only the needed identity header, responses are
  non-cacheable, and valid preflight is read-only. It then admits through the actual
  RoomRuntime and SQLite UOW, proves the 48-character `aas1.` result, retries the
  consumed one-use invite to receive the byte-identical JSON/bearer with use count
  still one, and proves changed payload returns 409 without another use. The complete
  server run passed 52 unit and 32 integration tests; warning-denied all-target Clippy,
  architecture/source-growth gates, and `make check` passed. The route module is 434
  lines; commit `b32c2b7` is 792 insertions/2 deletions across seven files, below the
  1,000-line review threshold.
- Review correction: critical line-by-line review found two reachable transport
  mismatches. Before `888084e`, the shared body collector collapsed the existing
  ten-second `tower-http` deadline error into 413, although the original returned
  408 for a stalled bounded body; the join adapter also shortened the original
  `participant_identity_conflict` code to `identity_conflict`. The correction walks
  the maintained body's error chain only far enough to recognize
  `tower_http::timeout::TimeoutError`, maps it to 408 `request_timeout`, and leaves
  actual declared/collected length failures at 413. All routes using the shared
  decoder preserve that distinction, and the host-ticket empty-body path now reuses
  the same owner instead of duplicating collection. No timer, timeout layer, queue,
  retry, or body buffer changed. A deterministic `DeadlineBody` test proves timeout
  and length-limit separation, and an adapter test pins the exact collision status
  and code. This adds only bounded error-chain inspection on a failed body read; no
  success-path performance improvement is claimed. Warning-denied Clippy, all 54
  server unit tests, every server integration test, and `make check` pass.

### One durable browser admission credential without fallback

- Prior cost and threat: the copied frontend accepted any trimmed stored value of at
  least eight characters. New values preferred `randomUUID`, but missing crypto or
  failed `localStorage` silently fell back to `Date.now` plus `Math.random` and could
  return a page-lifetime value without durable confirmation. That value is admission
  and pending-upload custody input: weak generation permits guessing, while silent
  regeneration changes exact-retry identity and can reset browser-bound quota
  subjects. It also cannot pass the Rust boundary's exact `aad1_` parser.
- Change intent: commit `caf9e37` replaces that owner with exactly one fresh-only
  `agentsassemble.browserCredential.v1` value: `aad1_` plus 32 bytes from
  `crypto.getRandomValues`, encoded as canonical unpadded Base64url. First use performs
  one storage lookup, one 32-byte random fill, one write, and one exact readback.
  Later use performs one storage lookup and canonical decode/re-encode of 32 bytes.
  The old device-token key is neither read, changed, imported, nor migrated.
- Preserved contract: every current browser identity call site obtains the same
  durable value; preflight, join, pre-join avatar upload, recovery, pairing, host
  claim, startup identity, and local profile/preferences continue to use the existing
  request fields and headers. Missing WebCrypto, inaccessible/non-durable storage, or
  a malformed stored value now produces a visible hard stop. Admission and pre-join
  upload catch that failure before invoking their network adapters. No malformed
  value is silently deleted or replaced.
- Design and resource bound: the existing identity module remains the sole owner.
  There is no credential context, memory cache, second storage copy, timer, task,
  dependency, compatibility reader, or future credential abstraction. The fixed
  32-byte arrays and 48-character string are the complete live data. This removes a
  concrete weak/ephemeral-authority threat; it does not claim lower CPU, memory,
  disk, or latency, and the first-use readback intentionally adds one tiny storage
  read to prove durability.
- Verification: focused browser tests prove one random fill and exact reuse,
  canonical 48-character shape, no old-key import, malformed-value preservation,
  WebCrypto failure, and failed write confirmation. Hook/component tests prove
  credential failure performs no preflight, join, or pre-join upload request and is
  rendered as an error. All 77 frontend test files (385 tests) pass. TypeScript/Vite
  production build passes, including the exact original CSS cascade/hash check; the
  workspace architecture, source-growth, policy, formatting, and Rust check gates
  pass. The implementation/test commit is 361 insertions and 47 deletions across 11
  files, below the mandatory split threshold.

### One bounded pre-join avatar owner in the existing attachment store

This section records the historical implementation and review evidence for commits
`facaaab` and `81c04e7`. Its combined-table and generic quota decisions are
superseded by the active asset-custody correction; they are not current target
architecture.

- Prior cost and threat: the original filesystem owner acquires its process lock,
  enumerates and parses every live attachment record, deletes an exact-custody
  predecessor, enumerates the directory again, then writes image bytes and metadata
  separately before a directory rename. Admission later performs another metadata
  read and rewrite to transfer the asset. More importantly, a single pre-join check
  before image work would let a revoked or exhausted invite consume the shared
  decoder, while a check only before decoding would allow an invite-state race to
  commit bytes after authority changed. Review later found that commit `cc57217`
  parsed canonical invite evidence before Base64 decode but did not prove a join-code
  row was current until after that decode. With 128 admitted connections, arbitrary
  canonical join codes could therefore retain up to 1.25 GiB of additional decoded
  output while awaiting the durable lookup, besides spending synchronous decode CPU.
- Change intent: commit `facaaab` adds the durable write to the existing `SqliteStore`
  and `profile_attachments` table. Commit `81c04e7` makes the pre-decode check explicit:
  the persistence owner returns one opaque, privately constructible authorization
  only after the indexed current invite/room/use-limit read. The decoder/store accepts
  that authorization and checks its immutable credential evidence again in the final
  transaction. That transaction removes expired pending rows,
  evaluates all quota dimensions in one conditional aggregate, excludes only the
  exact custody predecessor, then deletes and replaces that row atomically.
- Preserved contract: custody remains the exact presented invite credential plus
  browser credential, while both credential forms and every browser for one invite
  share the signed-invite quota. The limits remain 10 MiB per image, 8 files/32 MiB
  per invite, 64 files/128 MiB pending per room, 512 files/1 GiB per room, and
  4,096 files/8 GiB per runtime. Pending lifetime remains one hour. A successful
  replacement removes only its exact predecessor; failure leaves it intact. Assets
  transferred by admission keep invite/room/runtime provenance without being
  retroactively charged to the admitted user's ordinary 64-file uploader quota.
- Observed resource bound and trade-off: invalid current authority performs one
  indexed invite/room read and no Base64 output allocation, image decode, or BLOB
  write. The bounded JSON envelope is still admitted before its body-carried
  credentials, matching the reachable client contract; the 14,046,552-byte route
  limit, ten-second body deadline, and 128-connection process bound remain explicit
  residual costs. A valid attempt performs that precheck, at most one existing decoder
  job under the process-wide two-permit semaphore (10 MiB input, 4,096-pixel
  dimension, 16 Mi-pixel, and 72 MiB decoder allocation bounds), one final indexed
  invite/room read, one expired-pending delete, one aggregate over the post-cleanup
  live attachment set bounded by 4,096 rows, one exact-custody delete, and one
  canonical PNG insert. The second authority read is intentional TOCTOU protection.
  No cache, new table, filesystem store, decoder, queue, task, trait, or future
  provider abstraction was added. This is operation and allocation evidence, not a
  measured CPU, memory, disk, or latency improvement.
- Verification: three focused persistence tests prove exact-custody replacement and
  isolation, one-hour canonical PNG metadata, rejection after preauthorization is
  made stale by revoke without mutation, shared eight-item invite quota across
  browsers, replacement at the quota boundary, admission-provenance exclusion from
  ordinary user quota, and inclusion of 4,096 live pre-join rows in the shared runtime
  cap. All 159 persistence tests pass; warning-denied workspace Clippy and
  architecture, source-growth, policy, formatting, and workspace-check gates pass.
  The original persistence implementation commit is 537 insertions and 8 deletions;
  the pre-decode correction is 198 insertions and 108 deletions across four files.
  The current 575-line owner and 737-line canonical attachment owner remain below the
  mandatory 800-line source gate.

### Pre-join upload and preview through the existing attachment route

The reachable HTTP behavior in this section remains current. References to the old
combined row state are superseded by `asset-custody-lifecycle-slice.md`: preview now
reads an unexpired `prejoin_avatar_assets` row and admission removes that exact row
while promoting the same opaque ID through the profile lifecycle.

- Prior gap and threat: before `cc57217`, the copied guest profile panel sent its
  current `invite_token`, canonical `device_token`, and cropped avatar to the existing
  `/api/attachments` route, but that route unconditionally required a one-use profile
  ticket and returned 401. Returning metadata without a readable pre-admission URL
  would also leave the real panel's immediate image preview broken. Conversely,
  treating a malformed supplied Authorization header as absence would let a caller
  bypass consume-before-body ticket failure and fall into public invite handling.
- Change intent and smallest design: the existing handler checks header presence. If
  Authorization is supplied it consumes the existing profile ticket before reading
  the body, with no public fallback. With no Authorization, it accepts only
  `profile_avatar`, parses the two invite credential forms and canonical browser
  credential immediately after bounded JSON decode, obtains current durable
  invite/room authorization from persistence, and only then performs Base64 and image
  work. The raw upload type no longer derives `Debug` and raw credentials never cross
  the HTTP module. No route, ticket kind, table, store, queue, task, or client
  orchestration was added.
- Preserved contract and preview boundary: authenticated local/session profile
  uploads keep their existing ticket and authority paths. Pre-join upload returns the
  same attachment metadata shape used by the copied UI. The existing opaque UUID URL
  can read a live pre-join image until its one-hour expiry, matching the
  original immediate preview capability; ordinary 15-minute pending profile uploads
  remain hidden. Exact-custody replacement makes the previous URL 404. Admission
  still rechecks invite, room, attachment ID, exact custody, invite fingerprint,
  integrity, and TTL in its transaction before binding that image to the human-profile
  SSoT. A leaked live opaque preview URL can render only that bounded avatar and grants
  no invite, profile, room, or mutation authority; responses remain `private,
  no-store` and `nosniff`.
- Observed resource cost: every upload remains under the existing 14,046,552-byte
  JSON-body and ten-second deadline, so the adapter buffers at most one bounded
  Base64 envelope. An unknown, revoked, expired, exhausted, or inactive-room invite
  now stops after one indexed durable read and allocates no Base64 output. A valid
  payload then allocates one decoded input bounded just above the 10 MiB
  binary ceiling before the shared decoder enforces the exact 10 MiB and raster
  limits documented above. A preview performs one primary-key/state/expiry lookup and
  returns the stored canonical PNG BLOB. The two fingerprints and opaque
  authorization are request-local; no retained state or success-path retry exists. No
  CPU, memory, disk, or latency improvement is claimed without representative
  measurement.
- Verification: a real loopback Axum flow proves an invalid supplied profile ticket
  returns 401 before malformed JSON is decoded; an arbitrary canonical unknown join
  code carrying a valid ten-MiB Base64 payload returns `invite_invalid` through the
  pre-decode durable gate; two browsers retain separate custody; exact same-browser
  replacement makes only the old URL 404; both live previews render canonical PNG
  with `private, no-store`; admission binds only the exact selected avatar; exact retry
  returns the same result; and both the bound avatar and unrelated pending preview
  remain reachable afterward. The pre-existing profile boundary test still proves
  ordinary pending avatars return 404. All 159 persistence tests, 54 server unit
  tests, and every server integration test pass, together with warning-denied
  workspace Clippy and `make check`. The original HTTP commit is 182 insertions and 12
  deletions; correction `81c04e7` is 198 insertions and 108 deletions across four
  files. Current touched production modules are 390, 575, and 737 lines under the
  unchanged 800-line gate.

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
- The connection owner reuses one persistence method that runs the existing exact
  resolver and immutable-provenance comparison in a read transaction, returning a
  refreshed opaque authorization so mutable profile display data is current. Cloning
  that raw-free value only carries it into the bounded command owner; it creates no
  session cache or independently constructible authority.
- The only room commands available to a non-operator admitted browser—message send
  and room randomness—share their existing transaction bodies with session-specific
  entry points. Those entry points revalidate the exact fingerprint and scope in the
  same SQLite transaction before replay admission, write-budget reservation, event,
  result, routing, or turn assignment. Local/private callers retain their existing
  entry points; no generic authority framework or duplicated SQL transition was added.
- The bounded room command carries one optional opaque human-session authorization
  beside its current resolved principal. Only the runtime admission owner can create
  that pairing. Human dispatch accepts only message and room-random actions; every
  operator/Agent lifecycle/settings action fails before external effects, while the
  existing local/private dispatch remains unchanged.
- The authorization is boxed only when a human-session command enters the 128-item
  room queue. The first value layout made every `RoomInput::Mutation` slot at least
  456 bytes while the next-largest variant was at least 224 bytes; warning-denied
  Clippy rejected that cost. Indirection restores the bounded local/private queue
  layout while charging one allocation only to session-originated commands that
  need provenance custody. No throughput or latency gain is claimed without a
  benchmark.
- Public-human grant policy and resolution now occupy the 393-line
  `ticket/human_session.rs` responsibility, while the shared one-use map, lock,
  insertion, expiry removal, and non-session purposes remain in the 504-line
  `ticket.rs` owner. This is a source-structure split only: it adds no map, lock,
  cache, trait, configuration layer, or state transition.
- Preserved contract: revocation after ticket issue or during an idle connection
  invalidates the exact session promptly; a missed/lagged notification fails closed.
- Trade-off: outbound validation adds one indexed session/membership lookup per
  recipient frame. That cost is accepted for the concrete threat of a revoked or
  replaced human receiving a queued event; no unmeasured cache is introduced.
  Process restart closes the socket and reconnect must revalidate the database.
- Current verification: real Axum/SQLite/WebSocket tests cover typed no-store
  exchange, one-use consumption/replay rejection, authenticated subscription and
  snapshot, normal message commit, read-only message denial, and immediate idle
  socket close after a second reusable admission replaces the exact session.
  Persistence tests separately prove an invalid/replaced authorization commits no
  message or random-command result/event. Deadline-expiry, notification-lag/closure,
  and controlled inbound/outbound race tests remain required before this slice's
  packaged verification is complete; no query-count or latency result is claimed.

### Public browser entrance and server-surface binding

- Prior observed defects: the Rust static owner served the copied application only
  below `/app`, while the reachable original registers exact `/join`, `/join/`,
  `/pair`, and `/pair/` entrances. A real browser therefore received 404 at the URL
  copied by the invite UI. After those routes were exposed, admission succeeded but
  the guest had no server-directory authority because the host-only `/api/rooms`
  response was its only source. The canonical socket consequently stayed unready.
  The copied admission hook also restored a stored session after any failed join,
  allowing an unrelated transport or validation failure to appear successful.
- Change intent and smallest design: the static router serves the same production
  index at the four exact original entrances and exposes the same Vite asset
  directory at `/assets`, `/join/assets`, and `/pair/assets` so both slash forms
  resolve the copied bundle's `./assets/*` URLs; no copy, catch-all, or redirect
  fallback was added. One router-owned `no-cache` layer covers all static responses,
  and the signed surface derives wildcard paths from the exact entrance and asset
  prefixes used by that router rather than maintaining a second route list. A
  successful admission response now carries the existing immutable server ID,
  authority lineage, and `ServerProductSurface`. The existing room-directory validator owns exact shape,
  digest verification, and lifetime binding for both host directory and guest session
  sources. The guest session owns one verified surface projection; it does not add a
  directory cache, authority trait, compatibility reader, or second socket state.
- Preserved security and product contract: the raw human bearer remains confined to
  typed ticket exchange. Preflight and admission variants reject missing, extra, or
  mistyped fields. Fresh join binds the echoed request ID, preflight room, and
  requesting client; recovery binds the requested room and client. The server-returned
  avatar is the only avatar
  persisted after admission. A join, pairing, recovered, or persisted session cannot
  expose its bearer to the socket until the surface structure and digest bind to the
  current origin and any existing lifetime pin. Invalid surfaces fail terminally,
  remain unpersisted, do not update the remembered person profile, and do not clear
  the invite URL. Stored sessions without the current surface contract are invalid;
  failed admission is not converted into stored-session success. Per-attempt generation
  fencing rechecks after asynchronous digest verification and before the lifetime pin
  or any persistence/UI side effect, so a changed entrance cannot commit stale state.
  The later signed
  WebSocket receipt still pins the exact surface digest and room/participant.
- CPU, memory, disk, and latency cost: a successful admission response performs one
  existing bootstrap-status SQLite read so server ID and lineage are not copied into
  a new process state owner. It serializes one bounded product-surface object and the
  frontend stores one copy with the room session. Digest verification performs one
  WebCrypto SHA-256 over the small sorted registry. No table, index, cache, task,
  timer, retry, fallback, trait, or configuration layer was added, and no performance
  improvement is claimed without representative measurement.
- Verification: exact-route integration resolves `./assets/app.js` from all four
  entrance response URLs, requests each resulting asset path, and requires the same
  browser-security and `no-cache` headers on every response. Admission tests reject
  malformed or loose response variants, another client's join response, and a stale
  post-digest attempt before persistence/token exposure, including the
  identity-recovery path. Computer Use ran
  the production frontend against a disposable canonical Axum/SQLite server in
  isolated real browsers: a normal guest admitted, removed the URL token, received the
  canonical snapshot/roster, and published one durable message; a separate read-only
  guest received the same snapshot and rendered disabled posting controls, and SQLite
  contained no read-only message. This proves the normal/read-only browser connection
  boundary only; controlled expiry/lag/final-outbound races and the remaining full
  invite matrix remain open.

### Room command dispatch separated from room ownership

- Prior structure and intent: `room_runtime.rs` was 798 lines because it owned both
  the bounded room task and every action-dispatch branch. Adding session provenance
  there would mix queue custody with command selection at the absolute source limit.
  The unchanged dispatcher now has its own 200-line module, while the room owner is
  607 lines and retains queues, task lifetime, publication, and replies.
- Preserved contract and cost: the extracted function bodies are identical apart from
  the module-visible entry point. No queue, allocation, task, branch, trait, state,
  configuration, retry, fallback, or runtime call was added, so no performance claim
  is made. Direct old/new function diff, all 58 server unit tests, warning-denied
  workspace Clippy, and the architecture/source/check gates pass.

### WebSocket lifecycle separated from HTTP admission

- Prior structure and intent: `web.rs` combined listener/HTTP routing with the
  authenticated socket select loop. Human-session revocation, expiry, and outbound
  validation belong to the connection lifetime rather than the HTTP adapter, so the
  unchanged loop now has one 166-line owner and `web.rs` is 463 lines.
- Preserved contract and cost: the loop body is identical apart from its module-visible
  name. Upgrade limits, connection lease lifetime, ingress accounting, ordering,
  command replies, publication, and catalog delivery are unchanged. No allocation,
  task, branch, timer, trait, fallback, or synchronization owner was added, and no
  performance improvement is claimed. Direct function diff and the mandatory server
  tests, warning-denied Clippy, architecture, source, and workspace checks verify the
  move before session-specific behavior is introduced.

### One durable human-session resolver instead of repeated partial checks

- Prior cost and threat: the original session service verifies a fingerprinted
  session record, while individual routes separately resolve its user, membership,
  and room. Before commit `28babe8`, Rust invite preflight also carried its own
  session SQL and liveness/profile decoder. Adding ticket exchanges beside that copy
  would create two authorities that could disagree on corruption, room activity, or
  membership state.
- Change intent and smallest design: one 189-line persistence module now performs an
  indexed session-fingerprint lookup with left joins to the exact room, user profile,
  and room participant in one SQLite read transaction. It returns a private-field,
  non-serializable `HumanSessionAuthorization` containing only the raw-free
  fingerprint, derived principal/scope/capabilities, and expiry. Existing invite
  preflight calls the same internal resolver and preserves its foreign-room
  `NotApplicable` behavior before inspecting unrelated session state.
- Preserved security and product contract: authority requires stored `active` state,
  wall-clock expiry, browser client kind, canonical read-write/read-only scope, an
  Active exact room, a Joined exact human participant, and an exact revisioned person
  profile binding. Profile display name remains the person-profile SSoT; participant
  role, room mute, and membership remain room state. Invite revocation still blocks
  future admissions without retroactively ending a committed session. Read-only
  derives non-posting capabilities and never becomes operator authority.
- Observed cost and trade-off: one authorization performs one transaction begin, one
  indexed session lookup with three primary-key joins, JSON decode of exactly one
  room, participant, and profile, and one read commit. Compared with the prior
  preflight query, this adds the room join/decode but removes a second implementation;
  no CPU, memory, disk, or latency improvement is claimed. No session cache, table,
  index, task, timer, fallback, grant, or route was added. The opaque type is the
  minimum enforcement needed to prevent the next in-memory issuer from constructing
  provenance without persistence.
- Verification: the existing five preflight tests still prove same-room live,
  same-room unavailable, unknown, and foreign-room behavior. One real-admission test
  proves exact fingerprint/principal/profile projection, read-only capability
  derivation, participant-left rejection, and corrupt profile-revision failure. All
  160 persistence tests, warning-denied persistence Clippy, and `make check` pass.
  Commit `28babe8` is 387 insertions and 35 deletions across four files; production
  and test modules are 189 and 168 lines, with no source-gate exception.

### Shared grant-store limits instead of a second session ticket cache

- Prior cost and threat: the existing grant store was globally bounded at 4,096, but
  an exchange endpoint without provenance sublimits lets public sessions occupy all
  slots and starve private control. The server already bounds admitted HTTP
  connections at 128; no measured exchange-rate or CPU/lock-latency result supports
  a second per-minute threshold.
- Change intent and smallest design: commit `7af1345` keeps the existing mutex and
  `HashMap`. A public issuance performs the same expired-entry retain pass while also
  counting live public grants and entries with the exact 32-byte session fingerprint.
  It adds no second map, cache, index, timer, task, rate limiter, or synchronization
  owner. Structure-only commit `294b239` first moved the pre-existing ticket tests out
  of the 696-line implementation owner; the resulting implementation is 757 lines and
  the focused test module is 418 lines under the unchanged source gate.
- Preserved contract: grants stay opaque, short-lived, exact-purpose, one-use, and
  consume-on-wrong-purpose; local/private issuers keep a reserved capacity floor;
  ordinary clients gain no new requests-per-minute rejection behavior. Each public
  entry owns the non-serializable persistence-issued authorization rather than copied
  identity strings. Its monotonic deadline is capped by the backing session expiry,
  and consumption also rechecks the authorization's absolute expiry before returning
  it. A read-only session cannot mint a preference-write grant. Existing local/private
  issuance and purpose behavior are unchanged.
- Trade-off: a stolen live session may keep churning grants within the existing HTTP
  work bounds, but cannot hold more than eight or cross the public partition. A rate
  limiter requires measured CPU/lock/latency evidence and a separately reviewed
  product limit rather than a speculative threshold in this migration slice.
- Observed CPU, memory, disk, and latency cost: public issuance is one `O(n)` pass over
  at most 4,096 in-memory entries, one mutex critical section with no nested await,
  four UUID generations, and one insertion. The private path retains its prior one
  expiry pass, length check, UUID work, and insertion. Temporary same-toolchain size
  measurement found the authorization and public grant are 168 and 176 bytes; adding
  the inline variant changes `TicketAuthority` from 120 to 176 bytes and each stored
  entry from 160 to 216 bytes. That is at most 4,096 × 56 = 229,376 bytes of
  additional inline value size. It excludes heap capacity owned by strings,
  `HashMap` allocation, and allocator overhead, so no total-heap upper bound is
  claimed. Keeping the value inline avoids a separate heap allocation for every
  public grant; no memory-performance improvement beyond that measured
  representation choice is claimed. This slice performs no disk I/O.
  Five warmed debug-build runs of the real 16-public/2,304-private boundary test
  measured the first 16 uncontended public issue calls, excluding durable
  authorization, at 9.1–10.1 microseconds average and 18.6–23.5 microseconds maximum.
  The call includes mutex acquisition, sweep/count, UUID work, and insertion, so it is
  not presented as a production benchmark or a separately instrumented lock metric.
  The existing private `O(n)` sweep reached 26.5–56.0 microseconds average while the
  test filled entries 17 through 2,320, with noisy maxima of 0.74–6.42 milliseconds;
  this evidence supports keeping the fixed bound and does not justify another index.
- Verification: real persisted admissions supply every test authorization; no test
  constructor can manufacture provenance. Tests prove exact purpose and
  consume-on-mismatch, read-only write denial, eight-per-session enforcement,
  consumption reclamation, a 16-entry public partition beside the exact 2,304 private
  reserve, full-store rejection, private/public reclamation, and the production
  4,096 → 1,792 capacity calculation. All 57 server unit tests, every server
  integration test, warning-denied server Clippy, and `make check` pass. Public
  exchange and target routes remain explicitly unmounted until their durable
  post-consumption revalidation is implemented.

### Profile targets reuse one durable session snapshot

- Prior cost and threat: accepting the principal snapshot carried by an in-memory
  grant would allow a revoked, expired, left, foreign, or corrupt session to read or
  mutate the person profile. Re-running the generic room-principal profile path would
  still not prove the exact human-session fingerprint, expiry, client, scope, room,
  user, and participant captured by the grant. A separate profile lookup after the
  human-session join would also reread profile state already decoded by that join.
- Change intent and smallest design: commit `8efaa25` adds one internal
  `revalidate_human_session` function to the existing persistence owner. It resolves
  the exact fingerprint in the target transaction and compares every immutable
  provenance field plus the server-derived capability ceiling. Display name is
  deliberately excluded from the equality check because the revisioned person
  profile owns that mutable value. The resolver returns its already decoded profile
  to the profile target, avoiding a second indexed profile query. `UserProfile` is
  boxed only in the internal resolution enum to keep its small failure variants under
  the warning-denied large-enum gate; this is one explicit heap allocation, not a
  cache or retained owner.
- Preserved contract: a profile read revalidates immediately before returning its
  result. A profile patch revalidates and commits the profile, avatar binding, room
  projection, and events in the same SQLite transaction. Read-only room scope may
  still read and patch the person profile, but cannot acquire new upload authority.
  Current profile name/status/avatar changes do not invalidate an otherwise exact
  grant. Session end/expiry, inactive room, participant leave, missing/corrupt profile,
  or changed immutable provenance fails closed. Existing local/private profile methods
  and projection semantics are unchanged.
- Observed CPU, memory, disk, and latency cost: a target performs one read transaction
  and the existing indexed session lookup with three primary-key joins, JSON decoding
  for one room, participant, and profile, plus one temporary profile box. Read commits
  without another query. Write adds only the pre-existing profile patch, optional
  avatar authorization/rebinding, and active-human room projection work. No cache,
  table, index, timer, task, retry, route, or fallback was added, and no latency
  improvement is claimed without a representative HTTP measurement. Reusing the
  decoded profile removes one otherwise certain primary-key query rather than adding
  speculative state.
- Verification: a real read-only admission reads and updates its full profile, then
  reuses the same grant provenance after the mutable display name changes. A changed
  durable expiry is rejected as `invalid_state`; participant leave rejects both read
  and write as `session_revoked`; the rejected patch leaves no value after membership
  restoration; a corrupt profile revision still fails. All 160 persistence tests,
  warning-denied persistence Clippy, and `make check` pass. The implementation is 166
  insertions and 8 deletions across four files; production owners are 228 and 639
  lines under the unchanged gate. Public exchange and profile HTTP consumption remain
  unmounted and therefore are not yet reachable parity.

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

## Exact participant leave cutover

The current copied guest UI reaches `participant.leave` over its authenticated room
WebSocket. The original attendee/connector contract also retains its separate
`POST /api/room-invite/leave` entry point; HTTP is not a fallback for WebSocket.
Both transports enter the same `RoomRuntime` command owner and the same SQLite
mutation.

- Authority and state: a joined non-operator human with `participant.leave` may
  leave whether its invite is read-only or writable. The local room owner is rejected
  with `owner_must_transfer_or_delete`. Persistence alone owns exact `{}` payload
  validation. One transaction changes the exact participant to `left`, ends the exact
  live `human_room_sessions` row, inserts one `participant_left` event, and stores the
  command result. Person profile, room preferences, messages, and every asset custody
  table remain unchanged.
- Session boundary: after commit, the runtime broadcasts only the returned session
  fingerprint. Other sockets for that session close through durable revalidation. The
  command socket receives its one authenticated committed ACK directly after commit
  and then closes before any later product frame. No socket I/O is performed while a
  SQLite transaction or room mutation lock is held. Lost notifications and restart
  remain fail-closed because every later ticket, command, and outbound publication
  revalidates durable session state. The browser accepts that terminal ACK only when
  its durable event sequence, room, and participant bind the authenticated session.
  An ordinary close drains a frame already received for that connection generation;
  protocol failure latches across queued and asynchronously verifying frames, leaves
  the exact request pending, and reconnects for server-owned outcome recovery.
- HTTP boundary: the raw `aas1` bearer is authenticated before reading the body. The
  route caps the body at 4 KiB, decodes JSON without interpreting leave semantics, and
  passes the original value to `RoomRuntime`. Persistence is therefore the sole `{}`
  policy owner. A semantic-invalid authenticated object traverses the normal
  principal admission and room queue before returning `invalid_participant_leave` as
  HTTP 400. Responses are `private, no-store`; unresolved failures remain 503 and
  definitive permission, revocation, and conflict outcomes retain their distinct
  status classes.
- Cost and threat basis: the prior generic command path charged process and durable
  room-write budgets even for a successful action that immediately destroys its own
  authority. Only a fresh, valid, non-owner leave with no existing request identity
  now skips the process debit and does not reserve durable room-write quota. Invalid
  payloads, owner attempts, and every pre-existing leave identity retain the process
  throttle. In particular, a reusable-identity rejoin that repeats the earlier
  membership's committed leave request ID is charged and then conflicts without
  ending the new session. The mutation adds no table, index, cache, background task,
  trait, configuration layer, or cleanup scan; its durable cost is one participant
  row update, one exact session-row update, one event insert, and one result insert.
- Current boundary: admitted humans still have no `agent.control`, so no reachable
  human-owned Agent Session exists to terminate. No future-only ownership or provider
  cleanup state is added. Companion admission must expand this transaction before it
  can be called complete if that later surface grants human-owned agent authority.
- Verification: persistence tests cover atomic read-only leave, exact session end,
  owner and nonempty rollback, real reusable-identity rejoin, charged old-ID conflict,
  and preservation of the new session. Real Axum/SQLite WebSocket and HTTP tests cover
  one ACK then close, closure of an idle sibling socket, post-leave ticket denial,
  bounded invalid HTTP payload, HTTP status/no-store, and one durable leave. The copied
  production frontend passed a real one-use guest join, server-menu confirmation,
  room removal, reload, server restart on the same database, and no session recovery.

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
  to the same profile rendered in the room and lower-left panel. Current replacement
  and storage acceptance is owned by the asset-custody correction rather than the
  historical generic uploader quotas recorded above.
- Real Axum tests exercise create, preflight, admission, every typed ticket exchange,
  target-ticket replay/wrong-purpose/wrong-room, raw-bearer rejection, read-only
  full person-profile patch success, read-only profile-avatar upload/room-upload denial,
  same-user existing-avatar bind/clear, foreign-avatar rejection, public bound
  profile-avatar read, and proof that profile mic/deaf fields do not mutate room mute
  or capability; normal posting, profile SSoT, preferences, leave, exact
  revoke, kick, and room close. Invite management tests prove consume-before-body,
  room/purpose binding, transactional capability revalidation, and that ingress
  custody is not management authority.
- Current real WebSocket tests prove initial snapshot readiness, normal posting,
  read-only posting denial, one-use ticket replay denial, and immediate idle close
  after exact session replacement. Persistence tests replace the exact session
  before the SQLite mutation UOW and prove no durable command result or event
  commits. Controlled virtual-time expiry keeps a live socket active past the
  independent idle deadline and closes it at durable session expiry. A bounded real
  broadcast queue proves lag triggers durable revalidation and closure always fails
  closed. A committed SQLite replacement with the derived notification deliberately
  omitted proves the final outbound check blocks the next durable event. Handler
  cancellation and restart tests prove one durable
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
