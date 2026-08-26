# Human invite schema verification — 2026-08-26

Status: partial slice evidence; canonical invite reads and the manager-authorized
create/revoke writes plus standalone credential issuance/authentication are
implemented. Row-bound read-only human preflight is also implemented, while raw
credential transport, routes, admission, post-commit notification, and the activated
browser flow are not implemented by these commits.

## Provenance and scope

- Original behavior baseline: `d5046473010d1353a81ee38337360e6d98f7bd6f`.
- Approved Rust design: `bfde3de`.
- Dual-credential schema commit: `b20c3b7`.
- Locator-binding correction: `b46eae9`.
- Canonical invite read boundary: `afd3f6d`.
- Manager-authorized invite create write: `a504835`.
- Exact timestamp correction: `06201de`.
- Invite revoke unit: `bbeeda7`, corrected by `a472566`.
- Current credential authority: `ce2d2a3`, corrected by `9544081`.
- Read-only preflight snapshot: `c2abc58`, corrected by `2520cf0` and `7982161`.
- Existing-session frontend scope regression: `43d4609`.
- The schema is fresh-only at version 35. No migration, compatibility reader,
  fallback column, or partially upgraded authority is accepted.

The initial schema increment changed only the durable `room_invites` authority and the fixtures
that must insert valid parent rows. It does not claim invite creation, inspection,
admission, HTTP, WebSocket, or frontend completion.

## Prior cost and observed threat

Before `b20c3b7`, Rust stored one 32-byte invite fingerprint beside a 36-character
UUID-shaped public ID. That shape could not represent the reachable original
contract: create exposes a signed `aai1` credential and an independent `aaj1_`
join code, while the public ID is the first 16 lowercase hex characters of the
signed-token SHA-256 digest.

The first correction made both fingerprints and the 16-hex format durable. Manual
Daybreaker review then found a concrete cross-binding threat: the database still
allowed a format-valid public ID to be paired with an unrelated signed-token
fingerprint. A confused writer could consequently make full-fingerprint admission
and public-ID management, revoke, or audit resolve different authority. This was
not a speculative optimization; it was a missing durable product invariant.

## Change intent and preserved contract

Version 35 stores `signed_token_fingerprint` and `join_code_fingerprint` as
separate `NOT NULL UNIQUE` 32-byte BLOBs. It rejects the old alias and enforces:

```text
invite_id = lowercase(first 16 hex characters of signed_token_fingerprint)
```

The database therefore owns the canonical locator/fingerprint relationship instead
of trusting every future writer to reproduce it. This preserves the two reachable
credential namespaces, their distinct identity on reusable admission, the original
public ID, exact byte comparison, and raw-token non-persistence. It adds no token
parser, issuer, cache, secret, retry owner, or future-provider abstraction.

## Resource and latency record

- Disk payload before this increment: one 32-byte fingerprint and a 36-character
  public ID per invite, plus one fingerprint uniqueness index.
- Disk payload after this increment: two 32-byte fingerprints and a 16-character
  public ID, plus one uniqueness index per credential namespace. Ignoring SQLite
  record/index encoding, the fixed row payload grows by 12 bytes: `+32 - 20`.
  Exact page and index growth is not claimed before a real invite workload exists.
- CPU: the locator relation evaluates SQLite `hex`, `substr`, and `lower` over one
  fixed 32-byte value only on invite insert or update. It adds no read-path work.
  No throughput improvement is claimed.
- Memory: the schema adds no process state, cache, background task, or unbounded
  allocation. Runtime allocation cost remains unmeasured until the issuer exists.
- Latency: no production route exists in this increment, so route latency was not
  fabricated from schema-test timing. The intended indexed direct lookups and their
  measured query cost must be recorded with the repository implementation.
- Security trade-off: one extra BLOB and uniqueness index are accepted to preserve
  two independently reachable credentials. The locator CHECK adds bounded write
  work to close the observed cross-binding threat without another authority layer.

## Verification result

On `b46eae9`:

- `cargo test -p agentsassemble-persistence schema::tests -- --nocapture` passed
  all 5 schema contract tests in 0.04 seconds;
- the targeted mismatch fixture had valid foreign keys, scope, time, use count, and
  two distinct BLOB32 values, and failed only because `aaaaaaaaaaaaaaaa` did not
  match the `bbbbbbbbbbbbbbbb` locator derived from its signed fingerprint;
- accepted and rejected use-limit fixtures derive their IDs from their signed
  fingerprints, so the rejected rows still fail at the intended effective ceiling;
- `make check` passed architecture, source-growth, policy, formatting, and all-target
  workspace check gates; `schema.rs` remained at 748 lines;
- `cargo test -p agentsassemble-persistence schema_version::tests -- --nocapture`
  passed its exact-version test in 0.01 seconds, proving version 34 is rejected
  rather than silently applying or accepting the stronger version 35 schema;
- the critical web reviewer and the same Daybreaker manual security reviewer both
  returned `APPROVE`, with no remaining Critical, High, or Medium finding;
- no Deep Scan, automated security scanner, provider process, browser, or Computer
  Use resource was run for this schema-only increment.

These results prove only the physical schema invariants and clean cutover. The real
dual create/join paths, raw-credential absence, exact retry ordering, concurrency,
revocation, restart, and copied-frontend behavior remain required later evidence.

## Canonical read boundary

Commit `afd3f6d` adds the smallest read-only persistence surface needed by the next
issuer and admission increments. Both complete 32-byte credential fingerprints
resolve one canonical `HumanInvite`; listing returns stable `(created_at, invite_id)`
order and retains expired and revoked rows so a later route can apply the original
view policy. Reads never clean up, revoke, consume, cache, or otherwise write invite
state.

Stored authority is decoded fail-closed. Fixed fingerprints, timestamps, scope,
boolean state, public-ID derivation, nonempty identity fields, configured/effective
use counts, and time ordering must all remain valid. A malformed row becomes the
internal `InvalidHumanInvite` persistence failure. The existing WebSocket error
boundary redacts that variant to `persistence_failed`; it is not misreported as an
ordinary invalid invite and no stored credential or row value is returned to the
client.

### Read cost and design restraint

- A one-off SQLite planner check over the same primary/unique key shape reported an
  indexed search for each exact fingerprint query: the signed and join lookups used
  their separate SQLite automatic uniqueness indexes. Each successful lookup
  decodes one row and allocates only the returned owned strings and two fixed
  32-byte arrays. No process cache, background cleanup, or duplicate authority was
  added.
- The stable list plan reported a table scan and temporary B-tree sort because no
  `(created_at, invite_id)` index exists. That is a real potential CPU/memory cost,
  but no invite route or representative invite workload exists yet, and the current
  original behavior returns the complete list. Adding another persistent index or
  speculative pagination would therefore trade disk and write amplification for an
  unmeasured benefit or change reachable behavior. This increment records the cost
  and leaves it unchanged until route-level evidence justifies a contract-preserving
  change.
- The read code performs no disk writes and adds no schema bytes. `hex` was already
  present in the resolved dependency graph; declaring it directly avoids a private
  formatter without adding another resolved package.
- Production latency is not claimed from unit-test wall time. There is no active
  invite HTTP path in this increment to benchmark honestly.

### Read verification

- `cargo test -p agentsassemble-persistence -- --nocapture` passed all 134 tests in
  1.10 seconds, including one contract test proving that both complete fingerprints
  return the same typed row, listing preserves that row, the effective reusable
  ceiling remains 128, and an unknown fingerprint returns no row;
- `cargo clippy -p agentsassemble-persistence --all-targets -- -D warnings` passed;
- `make check` passed architecture, source-growth, policy, formatting, and all-target
  workspace compilation after the new internal error was explicitly mapped;
- the commit contains 278 added lines across seven files, including the 269-line
  read module, and every touched source file remains below the mandatory limit;
- the critical web reviewer compared both original PostgreSQL and local repository
  ordering/filter boundaries, and it and the same Daybreaker manual security
  reviewer returned `APPROVE` with no Critical, High, or Medium finding;
- no provider, browser, Computer Use resource, Deep Scan, or automated security
  scanner was used for this read-only increment.

This evidence does not prove creation, revocation, credential parsing/signing,
preflight policy, admission, concurrency, restart behavior, HTTP authorization, or
frontend parity. Those paths remain explicitly incomplete.

## Manager-authorized create write

Commit `a504835` adds one persistence write; it does not mint or return the raw
`aai1` or `aaj1_` credentials. Its input contains only both fixed fingerprints,
normalized public invite policy, and timestamps. Room and creator are deliberately
absent. The transaction re-resolves the supplied manager's current room membership
and profile binding, proves the exact local operator plus complete bootstrap
integrity, derives room and creator from that current identity, inserts the fresh
row, decodes the returned canonical authority, and only then commits.

### Create threat and preserved contract

A `RoomUserIdentity` is an earlier observation, not a durable capability. Trusting
its strings directly would let a stale identity create an invite after membership
loss or let a future caller select another room or creator. Revalidation inside the
same write transaction closes that concrete time-of-check/time-of-use and confused-
deputy boundary. The test removes the manager membership after obtaining the typed
identity and proves the next create fails without another row.

The write preserves exact nonnegative configured `max_uses`, including zero and
values above the effective ceiling; the stored effective limit remains derived and
never overwrites the public configured value. Typed scope, expiry, base participant,
display name, both independent fingerprints, fresh `use_count = 0`, and
`revoked = false` remain one row. Upstream issuance must still perform the original
normalization and generate the real credentials before this input exists. Both
timestamps must also be exactly representable in microseconds, matching the original
Python time precision and the SQLite integer contract.

The original repository used an upsert keyed by the truncated public invite ID.
Rust deliberately performs a fresh insert. A different full signed fingerprint that
shares the first 64 digest bits must fail instead of overwriting an existing invite
and silently rebinding its join credential. This is a security-preserving collision
rejection, not compatibility or retry behavior; no hidden second attempt is made.

The first web review found one concrete Medium issue in `a504835`: Chrono can carry
nanoseconds that `timestamp_micros()` silently discards. Because the signed-token
fingerprint already exists at this boundary, truncating here could make signed expiry
claims disagree with the newly committed row. Commit `06201de` rejects either
timestamp unless its subsecond nanoseconds are an exact multiple of 1,000. It does
not round or normalize after signing.

### Create cost and verification

- Input validation scans at most 64 participant-ID characters and 128 display-name
  characters, derives 16 lowercase hex characters from eight fingerprint bytes,
  and allocates no cache or background state.
- The existing one-connection SQLite writer performs current membership/profile and
  bootstrap-integrity reads followed by one `INSERT ... RETURNING`. Returning the
  inserted columns avoids a second select while still reusing the canonical decoder.
  The transaction adds no event, session, cleanup, or unrelated write.
- Durable cost is exactly one `room_invites` row plus the schema's existing primary,
  two credential-uniqueness, composite-authority, and room-state index entries. No
  new table or index was added by this commit.
- `cargo test -p agentsassemble-persistence -- --nocapture` passed all 135 tests in
  1.06 seconds; the focused invite tests passed 2/2 in 0.02 seconds. The create test
  adds one nanosecond to an exact expiry, proves rejection, then proves the later
  valid create is the only persisted row;
- warning-denied persistence all-target Clippy and `make check` passed;
- the commit is three files with 163 additions and five deletions; the invite module
  is 428 lines and no gate exception was added;
- Daybreaker approved the original create commit and the correction. The critical
  web reviewer returned one Medium for lossy timestamp conversion, then approved
  `06201de` with no remaining Critical, High, or Medium finding;
- no production HTTP latency is claimed because the issuer and route remain absent.

This evidence proves only the manager-authorized durable insert and rollback on
stale membership. Credential entropy, signing, response custody, exact route grants,
revoke, and browser behavior remain explicitly incomplete.

## Invite revoke unit

Commit `bbeeda7` first made invite revocation one room-scoped manager transaction but
incorrectly ended sessions derived from the invite. Manual review found that this
conflated two separate original authorities. Corrective commit `a472566` retains the
manager transaction and exact invite update while removing all session mutation and
notification targets from this unit.

### Revoke threat and preserved contract

The original invite revoke prevents future admission but does not revoke sessions
that were already established. Device/browser credential revoke, exact session
revoke, participant leave or kick, and room close are separate session lifecycle
authorities. Treating invite revoke as credential revoke would terminate every
current reusable-link participant and make a lost-response one-use exact retry
unavailable after the invite was revoked. That shrank reachable behavior and
contradicted the SDD's retry ordering, so it was removed rather than justified as
security hardening.

Existing invite IDs remain idempotent: revoking an already-revoked row succeeds and
missing or other-room IDs return false. Room and manager are derived from current
durable identity, not invite metadata supplied by the caller. The operation returns
only a boolean and exposes no credential, session fingerprint, or notification
provenance.

### Revoke cost and verification

- The corrected unit performs one primary-key invite update inside the existing
  manager transaction. It performs no session scan/update, builds no fingerprint
  vector, publishes no event, and adds no cache or index.
- Prior cost removed: the first implementation performed one indexed session update
  and allocated one 32-byte return item per active session for the invite. No
  performance improvement is claimed beyond eliminating work that had no product
  authority and was semantically wrong.
- The focused invite suite passed 3/3 in 0.02 seconds. Its revoke contract uses a
  separate person profile, matching `guest-ab` participant, one-use invite, and one
  active browser session under all current foreign keys; it proves the invite flag,
  active session preservation after both first revoke and idempotent replay, and a
  false missing result.
- The complete persistence suite passed 136/136 in 1.06 seconds; warning-denied
  all-target Clippy and `make check` also passed.
- The critical web reviewer and Daybreaker both re-reviewed corrective commit
  `a472566` manually and approved it with C=0/H=0/M=0. Neither used Deep Scan or an
  automated security scanner.
- The correction changes two files with 22 additions and 49 deletions. The invite
  module is smaller than before and remains below the mandatory source limit.

Production revoke latency is not claimed before the authorized HTTP route exists.
Session closure is explicitly not an invite-revoke effect; each separate session
revocation owner remains incomplete until its own implementation and live-flow test.

## Current invite credential authority

Commit `ce2d2a3`, corrected by `9544081`, adds the cryptographic boundary required
before a real preflight or create route can exist. It owns the current signed `aai1`
token and independent `aaj1_` join code, but performs no database lookup and therefore
does not claim claims-to-row matching, invite usability, admission, or HTTP
reachability.

### Prior cost and threat

The original code used a string signing secret and separately implemented signing,
join-code generation, parsing, Base64 handling, URL rules, and fingerprints. Copying
those mechanics into each future route would create multiple credential parsers and
make the same raw token cross more owners. Parsing unbounded or noncanonical input
before authentication would also allocate and decode attacker-controlled JSON.

The Rust runtime already has one permission-checked persistent host envelope with a
fresh 32-byte session HMAC key. Creating another key or secret store would add disk
state, backup semantics, and rotation failure modes without a separate cryptographic
authority. The approved SDD instead separates message domains with the fixed `aai1.`
signing input and the distinct future session-bearer context.

### Change intent and preserved contract

- `HumanInviteCredentialAuthority` is constructed once from the database-bound
  `PersistentHostIdentity` and shares the fixed key through one `Arc<[u8; 32]>` when
  `AppState` is cloned. It exposes no key accessor and implements neither `Debug` nor
  serialization. Issued raw credentials likewise implement neither trait.
- The signed credential preserves the original `aai1.<payload>.<signature>` shape,
  sorted compact UTF-8 JSON claim names and nested contracts, the 18-byte signed
  nonce, scope-to-permission mapping, and HMAC-SHA256 over exact ASCII
  `aai1.<payload>`. The join code remains a distinct `aaj1_` value backed by exactly
  24 independent operating-system-random bytes.
- Both fingerprints cover the complete ASCII credential with SHA-256 and remain
  fixed 32-byte values ready for the existing BLOB columns. No raw credential,
  claims copy, signing key, encoded digest, or nonce is made durable by this unit.
- Creation rejects noncanonical identity text, sub-microsecond or inverted time,
  unsafe room transport, and non-current URL shapes. Authentication bounds signed
  input at 4 KiB, validates exact ASCII segments and canonical unpadded Base64url,
  checks a 32-byte HMAC in constant time before decoding JSON, and then enforces the
  exact claim, nonce, timestamp shape/order, URL, and permission contract. It exposes
  signed expiry without applying the moving wall clock. The admission owner must
  bind the claims to the durable row, resolve an exact one-use retry, and only then
  apply current expiry to new and reusable admissions. A join code must have the
  exact 37-byte public shape and decode canonically to 24 bytes.
- The mature `url` crate owns generic URL parsing. Product policy permits HTTPS for
  loopback, private/link-local IPs, `.local`, and single-label LAN hosts, while HTTP
  remains loopback-only. Userinfo, query, and fragment are rejected. There is no
  transport fallback or second parser.

### Cost, limits, and verification

- Successful creation performs two bounded OS-random fills, one bounded JSON encode,
  one HMAC-SHA256, and two SHA-256 fingerprints. Authentication performs one bounded
  HMAC before one JSON decode. The longest test vector is 1,049 bytes, below the
  4-KiB parser ceiling; no heap or latency improvement is claimed without route-level
  measurement.
- App-state clones share the 32-byte key allocation instead of duplicating it. No
  cache, replay set, generic credential-provider trait, background task, or mutable
  parser state was added.
- A full fixed token vector locks sorted claim names, nested field order, signing
  input, signature, timestamp form, nonce size, and join-code size. Separate cases
  reject tamper, padding/noncanonical input, oversized input, sub-microsecond
  timestamps, public room hosts, and non-loopback HTTP. An authentic token still
  yields its claims at and after expiry, while the explicit time predicate is false
  one microsecond before expiry and true at the exact boundary.
- The server library suite passed 46/46; warning-denied server lib/test Clippy and
  `make check` passed after both commits. The issuer commit is five files with 711
  additions and two deletions. The correction is one file with 36 additions and 41
  deletions, leaving the single invariant-owning module at 692 lines below the
  mandatory 800-line source limit.

The first web review found one Medium contract error: `verify(credential, now)`
combined immutable authentication with current invite usability. That API could not
return the authenticated claims and fingerprint of an expired one-use credential,
so the later admission owner would either reject before exact lost-response retry or
bypass claims-to-row binding by looking up raw state first. Corrective commit
`9544081` replaces it with `authenticate(credential)` and exposes only
`is_expired_at(now)` on authenticated signed claims. It adds no mode flag, alternate
parser, cache, durable state, or fallback. The correction removes one wall-clock
comparison from authentication; it does not claim a meaningful latency improvement.
The critical web reviewer and Daybreaker both manually approved the correction with
C=0/H=0/M=0. Neither ran Deep Scan, an automated security scanner, or a provider.

## Row-bound read-only preflight snapshot

Commit `c2abc58`, corrected by `2520cf0` and `7982161`, compares authenticated
signed evidence or a complete join-code fingerprint with the exact current invite
row and resolves browser startup state from one SQLite read transaction. It does not
accept raw credentials, create identity, consume an invite, materialize expiry, end a
session, publish an event, or claim an HTTP route.

### Prior cost, threat, and change intent

The original product split invite, room, session, device identity, profile, and
membership reads across separate JSON-backed owners. Reproducing those unsnapshotted
reads would permit one preflight response to combine authority observed at different
times. Rust instead reads the current invite and room together, then optional session
or device/profile/membership state inside one transaction on the existing single
SQLite connection. This adds no second state owner or client-side orchestration.

The first implementation correctly bound signed claims to the durable row and kept
preflight read-only, but its session query discarded expired, ended, and inactive
same-room rows in SQL. Those failures became indistinguishable from no session and
could fall through to device/profile authority. Both manual reviewers ultimately
classified that credential-state collapse as Medium. `2520cf0` reads the exact row
before classifying it and returns one generic `SessionUnavailable` for a resolved
same-room row that is expired, ended, or no longer a joined human. Durable corruption
remains a persistence error rather than being relabeled as ordinary unavailability.

The same correction also reads `sessions.invite_scope` strictly. A read-only session
presented while inspecting a read-write invite remains read-only in the
`ExistingSession` response. Test-only commit `43d4609` proves the copied frontend
adopts that server-confirmed scope instead of retaining or inventing a broader local
value.

`2520cf0` initially over-applied fail-closed behavior to a missing fingerprint and a
valid session for another room. Daybreaker found that this breaks a reachable flow:
the copied frontend stores one global room session and must send it before a new
invite's room is known. The original route also converted an unknown/expired bearer
to no session before preflight and treated a valid other-room session as inapplicable,
then evaluated the independent current device authority. Corrective commit `7982161`
adds only that `NotApplicable` distinction. It does not permit an expired, ended, or
inactive session row for the current room to downgrade to device authority.

### Preserved contract and resource record

- Current invite authority is resolved first and applies revoke, expiry, effective
  use limit, and active-room gates. Signed evidence must match its complete
  fingerprint, room, base participant, display name, scope, creation time, and expiry;
  a mismatch is `InvalidHumanInvite`, not a semantic rejection.
- A live same-room session precedes device identity and requires its unique
  fingerprint, browser client kind, stored active state, wall-clock expiry, exact
  profile/participant binding, joined-human membership, and stored scope. Unknown and
  other-room sessions authorize nothing; they merely do not replace the separately
  durable current-room device/profile decision.
- Human display name and avatar come only from `user_profiles`. Participant JSON owns
  room membership and status, not the person profile. Operator status requires the
  exact local user and participant pair.
- The longest path remains three indexed queries: invite credential unique index to
  room primary key, session fingerprint unique index to profile/participant keys, and
  only when the session is absent or inapplicable, device fingerprint primary key to
  profile/participant keys. A matching or unavailable same-room session stops after
  two queries. No schema, index, cache, trait, cleanup task, or background state was
  added.
- The corrected session query reads one bounded row and performs fixed string/time
  checks in Rust so absence and current-room unavailability remain distinguishable.
  Expired and ended rows return before profile JSON decoding. No production latency,
  CPU, memory, or throughput improvement is claimed from unit-test time or the
  one-off planner evidence.
- The frontend production build reported a real 761.64 kB main JavaScript chunk
  warning. This preflight correction changes no production frontend bytes, and there
  is no evidence that speculative code splitting belongs in this persistence slice;
  the observation is retained for later frontend performance work.

### Verification result

- Focused preflight tests passed 5/5. They cover profile-required with no writes,
  profile SSoT and existing membership, signed row mismatch and current invite gates,
  same-room session priority, immutable read-only session scope, stored-active expiry
  without materialization, ended state, inactive membership, unknown fingerprint,
  and a valid other-room session. A valid device credential is present in the
  contested cases to prove the intended rejection or independent-authority path.
- The complete persistence suite passed 141/141 in 1.15 seconds. Warning-denied
  persistence all-target Clippy, `make check`, and `git diff --check` passed without a
  gate exception. The implementation is 341 lines and the test module 544 lines.
- Frontend `useRoomAdmission` tests passed 14/14 and the production build completed,
  including the existing-session scope regression.
- Daybreaker approved final commit `7982161` with C=0/H=0/M=0. The critical web
  reviewer approved `2520cf0` before the cross-room reachable-flow correction; its
  final `7982161` re-review is recorded when complete. No Deep Scan, automated
  security scanner, provider, or Computer Use resource ran for this increment.

The next increment must authenticate raw `aai1`/`aaj1_` input plus canonical browser
and session credentials at the server boundary and submit only this typed evidence
to the snapshot. Until that route exists, preflight is not reported as a reachable
browser feature.
