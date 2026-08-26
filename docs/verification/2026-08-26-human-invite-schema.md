# Human invite schema verification — 2026-08-26

Status: partial slice evidence; canonical invite management, credential
authentication, read-only preflight, canonical admission inputs, the atomic SQLite
plus bounded RoomRuntime owners, local HTTP preflight/join, and the fail-closed
browser credential owner and pre-join avatar upload/preview are implemented.
Session-derived grants, the authenticated room socket, and trusted public ingress
are not implemented by these commits.

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
- Reusable admission corrections: `cba3cb8`, `9c5883f`, and `27cf90a`;
  `ddac71c` only separates their schema tests from the production owner.
- Admission input and durable owner: `0606257`, `61745fb`, `3bbc8c6`, `dbcf928`,
  `657701a`, `6f91ab7`, `ee747ab`, `5f44cec`, `d76b2e6`, and security correction
  `c99a031`; queue routing/runtime ownership is `29d3d66`, `cf29ecd`, and `06587b0`,
  with exact-recovery/avatar correction `28d5d56`. Local HTTP preflight/join is
  `b32c2b7`; the browser credential owner and call-site cutover is `caf9e37`.
- Pre-join avatar persistence is `facaaab`; its HTTP upload, preview, and admission
  flow is `cc57217`, with evidence in `625e556` and `9c4a208`.
- The schema is fresh-only at version 38. No migration, compatibility reader,
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
- Daybreaker and the critical web reviewer approved final commit `7982161` with
  C=0/H=0/M=0. The web review first approved the overcorrection in `2520cf0`, then
  re-read the original route, session verifier, copied browser flow, and final diff
  before correcting that conclusion. No Deep Scan, automated security scanner,
  provider, or Computer Use resource ran for this increment.

The next increment must authenticate raw `aai1`/`aaj1_` input plus canonical browser
and session credentials at the server boundary and submit only this typed evidence
to the snapshot. Until that route exists, preflight is not reported as a reachable
browser feature.

## Raw credential preflight boundary

Commit `0b22bf8` adds the server boundary that accepts the raw invite, browser, and
optional session credentials and submits only authenticated typed evidence and
fixed-size fingerprints to the existing row-bound snapshot. It deliberately does
not register an HTTP route or claim a reachable browser feature.

### Prior cost and threat

The credential authority and persistence snapshot previously existed as separate
units. Passing raw request strings directly into persistence would make durable code
another parser and possible logging owner, while hashing arbitrary strings without
first enforcing the current credential domains would accept old, weak, padded, or
otherwise ambiguous client identifiers. Repeating invite parsing in the future route
would also create a second `aai1`/`aaj1_` authority.

The original reachable HTTP flow trims the invite value, verifies a bearer through
the session owner, and then calls preflight with either a verified session or no
session. The approved Rust contract keeps the same ordering of authorities while
strengthening newly issued browser/session formats: the browser credential is
mandatory and the optional bearer is considered only when present. A malformed
presented value is not converted to absence or a compatibility path.

### Change intent and preserved contract

- The existing `HumanInviteCredentialAuthority` remains the only raw invite parser.
  Its authenticated signed claims or join-code fingerprint are converted directly
  into `HumanInviteCredentialEvidence`; the snapshot still owns exact current-row
  binding, current invite policy, and session/device/profile decisions.
- Browser credentials accept exactly `aad1_` plus canonical unpadded Base64url of
  32 bytes. Session bearers accept exactly the disjoint `aas1.` domain plus the same
  canonical 32-byte body. Length, ASCII alphabet, decoded length, and re-encoding
  are all checked before hashing. Whitespace, padding, old identifiers, wrong
  prefixes, and noncanonical tail bits are rejected rather than normalized.
- SHA-256 covers each complete ASCII credential including its prefix. Only the
  resulting `[u8; 32]` crosses into persistence. The error display contains generic
  categories rather than the credential, and the function neither logs nor stores
  its raw string arguments.
- `None` is the only absent-session representation. Any `Some` value must be a
  canonical `aas1.` bearer. The existing snapshot still distinguishes a current-room
  unavailable bearer from a missing or other-room bearer after exact fingerprint
  lookup; this boundary adds no semantic fallback.
- The caller supplies one fixed `now`, which is forwarded unchanged to the single
  read snapshot. Credential syntax/authentication completes before SQLite is read.
  No route, response mapping, durable state, trait, cache, cleanup task, or alternate
  credential format was added.

### Resource cost and verification

- The `aai1` path retains the authority's bounded HMAC verification and JSON decode;
  the `aaj1_` path retains its bounded canonical decode and fingerprint. Each client
  credential adds one 32-byte Base64 decode, one 43-byte canonical re-encoding, and
  one SHA-256 over the complete 48-byte credential. At most two such client values
  are processed. Those temporary allocations are fixed and attacker input is
  rejected by exact length before decode.
- The durable work remains the preflight snapshot's existing maximum of three
  indexed reads. This boundary performs no disk write and adds no schema or index.
  No production CPU, memory, disk, latency, or throughput improvement is claimed;
  route-level measurement remains unavailable because the HTTP route is not active.
- One parser contract test covers both domains, complete-prefix hashing, and malformed
  length/padding/whitespace/wrong-prefix rejection. One integration test issues and
  persists both current invite credentials, proves each reaches the same
  `ProfileRequired` snapshot result, and proves old browser/session strings stop at
  the typed boundary.
- The server library passed 48/48 tests. Warning-denied server all-target Clippy,
  mandatory architecture/source-growth gates, workspace all-target build, formatter,
  and diff checks passed. The implementation is one 274-line owner plus a two-line
  module/export connection; the commit has 276 additions.
- Daybreaker and the critical-web reviewer manually approved the pushed commit with
  C=0/H=0/M=0. The web review separately re-read the credential authority, durable
  snapshot, copied HTTP helper, and inactive route registry. It retained the copied
  frontend's old device-token generator as an explicit activation dependency rather
  than treating the inactive server boundary as browser parity. No Deep Scan,
  automated security scanner, provider, or Computer Use resource ran.

## Deterministic human session bearer

Commit `1b1c7e6` adds the restart-stable `aas1.` issuer required by the future atomic
admission owner. It also moves presented-session shape validation out of preflight so
issuance and lookup fingerprinting have one format owner. It does not create a
session row, inspect terminal admission state, register a route, or claim a reachable
session flow.

### Prior cost, threat, and change intent

A committed admission must return the same raw bearer after a lost response without
storing that bearer. A random issuer would make recovery impossible; truncating the
MAC to the original generic random issuer's 24-byte body would change the reachable
human-admission shape. Independently defining `aas1.` in issuance and preflight would
also permit their prefix, length, canonical encoding, or fingerprint rules to drift.

The issuer uses the existing persisted 32-byte session HMAC key and the one fixed
transcript `agentsassemble-human-session-bearer-v1\0 || admission_key[32]`. The NUL
terminator and fixed-size admission key make the two fields unambiguous, while the
session context is disjoint from the existing `aai1.<payload>` invite signing input.
No key derivation layer or second persistent secret is needed.

### Preserved contract and design size

- HMAC-SHA256 consumes the complete fixed transcript. Its full 32-byte output is
  encoded as 43 canonical unpadded Base64url characters after the exact `aas1.`
  prefix, producing the approved 48-character bearer without truncation.
- SHA-256 covers that complete 48-byte ASCII bearer and exposes only the resulting
  `[u8; 32]` for durable lookup. The authority and issued value implement neither
  `Debug` nor serialization; tests do not include the actual bearer in failure text.
- `AppState` constructs the invite authority and bearer authority from the same
  database-bound host identity. Each authority owns one separate `Arc<[u8; 32]>`, so
  initialization makes one additional 32-byte secret copy and allocation; later
  `AppState` clones share both allocations. A shared-key wrapper would save only that
  fixed copy while coupling two disjoint credential domains, so it is not added
  without a measured need.
- The same module now checks every presented bearer for exact ASCII length, prefix,
  URL-safe alphabet, 32-byte decode, and decode/re-encode canonicality before hashing.
  Preflight still forwards only the fingerprint and retains its exact
  NotApplicable/Unavailable/Live state meanings.
- The unit adds no RNG, database access, durable state, cache, trait, fallback,
  alternate token format, or background owner. Future admission code must call it
  only for a newly committing or exact-live row; terminal rows remain unavailable.

### Resource cost and verification

- Each issue performs one HMAC-SHA256 over the fixed context plus 32-byte admission
  key, one Base64url encode of 32 bytes, and one SHA-256 over 48 bytes. The returned
  48-byte `String` is allocated once at exact capacity and the maintained Base64
  encoder appends directly into it.
- During implementation review, `format!` was observed to construct a separate
  43-byte encoded-body `String` before the returned bearer. Direct `encode_string`
  removes that one known temporary heap allocation while preserving the fixed vector.
  The pinned `base64 0.22.1` implementation writes through a 1,024-byte stack buffer
  and `StringSink::push_str`; after the five-byte prefix, the preallocated string has
  exactly the required 43 bytes of remaining capacity. This is a structural
  allocation count, not a production latency or throughput claim. Parsing a
  presented bearer retains its two bounded decode/re-encode temporaries for
  canonicality; no speculative parser optimization was added.
- A fixed vector locks the transcript, full MAC, bearer shape, and complete-bearer
  fingerprint. Tests prove same key/admission determinism, different admission
  separation, canonical presented parsing, exact recovery after reopening the same
  SQLite host, and a different result from a freshly initialized host.
- Focused issuer tests passed 3/3 and the complete server library passed 51/51.
  Warning-denied server all-target Clippy, formatter, mandatory architecture and
  source-growth gates, workspace all-target build, and diff checks passed. The new
  format owner is 199 lines; the commit has 218 additions and 11 deletions across
  four files.
- Daybreaker and the critical-web reviewer manually approved the pushed commit with
  C=0/H=0/M=0. The web review independently confirmed the disjoint HMAC domains,
  complete-MAC and complete-bearer hashing, restart recovery, fresh-host separation,
  parser preservation, and the pinned encoder's single output allocation. It also
  retained terminal-row exclusion as a mandatory integration condition: the future
  admission transaction may issue only for a newly committing or exact-live row.
  No Deep Scan, automated security scanner, provider, or Computer Use resource ran.

## Reusable credential admission corrections

Commits `cba3cb8`, `9c5883f`, and `27cf90a` correct two schema-level admission
defects before the atomic admission unit exists, then remove the correction's
now-unowned lookup index. Commit `ddac71c` moves only the focused tests into the
existing schema test module so the production owner remains below its mandatory
size gate. These commits do not implement admission, issue a bearer, consume an
invite, or make the browser flow reachable.

### Prior cost, observed threat, and change intent

Version 35 made `(invite_id, reusable_identity_fingerprint)` unique for every
reusable session. That accidentally treated one stable browser using one invite as
one admission identity. The reachable original flow instead derives identity from
the complete presented invite credential, so using the signed `aai1.` token and then
the independent `aaj1_` join code creates two distinct admission keys. Both consume
the reusable invite as distinct principals while resolving the same user and room
participant; the later live session replaces the earlier one.

The unique index rejected the second durable row even after the earlier row ended.
This was not initially a performance optimization opportunity: it was a concrete
mismatch between stored uniqueness authority and the approved retry/replacement
contract. Version 36 first removed only `UNIQUE`. A second ownership audit then found
that no current or approved future product query uses the resulting
`(invite_id, reusable_identity_fingerprint)` index, so version 37 removes the index
instead of preserving speculative disk and write cost. Exact retry ownership remains
the 32-byte `admission_key` primary key, and one live session per room participant
remains the separate partial unique index on
`(room_id, participant_id) WHERE state = 'active'`. Invite/room/scope/key-kind,
profile/participant, reusable-credential/user, and room-participant composite foreign
keys remain unchanged.

The remaining composite foreign key proved that the reusable credential and user
exist, but it did not prove that the request browser fingerprint stored in the same
session row is that credential. A writer defect could therefore persist browser A
as request custody while binding the durable user identity of browser B. Version 38
adds the smallest durable invariant: every reusable row must have
`reusable_identity_fingerprint = browser_credential_fingerprint`. One-use rows still
carry no reusable identity, and the existing composite credential/user foreign key
continues to bind the equal fingerprint to its user.

### Resource and security record

- Disk: no table, column, or replacement index was added. Version 37 removes one
  B-tree entry per reusable session. A browser that deliberately exercises both
  current credential forms may now retain the intended second terminal session row;
  the removed index therefore also avoids one entry for that row. Exact SQLite page
  savings are not claimed before the admission route and representative workload
  exist.
- CPU and latency: each reusable-session insert avoids one B-tree maintenance write.
  No production query loses an index: exact retry uses the admission primary key,
  session lookup uses the session-fingerprint unique key, reusable identity uses the
  device-credential primary key, active replacement uses the participant partial
  unique key, cleanup uses the invite-state index, and capacity uses the live-state
  indexes. No end-to-end speedup is claimed without a production admission request.
- Version 38 adds one fixed 32-byte equality comparison on each reusable-session
  write. It adds no lookup, allocation, index entry, stored value, or runtime branch;
  no measurable latency effect is claimed.
- Memory: the schema correction adds no runtime object, cache, task, allocation, or
  alternate authority.
- Security: removing this non-authoritative index does not remove any constraint or
  authorize a different browser or user. The complete admission key, composite
  foreign keys, and the atomic UOW's full-fingerprint collision checks remain
  mandatory. The schema test proves only coexistence; actual `aai1.`/`aaj1_` key
  derivation and live replacement are verified separately below.
- Security: the version 38 equality check prevents cross-browser credential/user
  binding even if the future repository supplies internally inconsistent fields.
  This is defense against a concrete durable-authority corruption path, not a second
  identity model.
- Clean cutover: the schema version increases from 35 through 36 to 37, so either
  older shape is rejected. Version 38 likewise rejects version 37 instead of
  migrating, reinterpreting, or accepting it through compatibility behavior.

### Verification result

- The focused coexistence test inserts two ended reusable rows with the same invite,
  browser credential, reusable identity, user, and participant but distinct admission
  and session fingerprints; both rows persist. Existing composite-authority tests
  still reject cross-room, wrong-scope, wrong-kind, wrong-user, wrong-participant, and
  wrong-credential bindings.
- The focused binding test inserts a valid reusable row and proves SQLite rejects an
  attempt to change only its request-browser fingerprint. The coexistence test also
  proves two distinct reusable admissions can persist and that the existing partial
  unique key still rejects activating both for the same room participant.
- The complete persistence suite passed 143/143. Warning-denied workspace all-target
  Clippy, `make check`, formatter, source-growth, architecture, policy, and diff gates
  passed. `cba3cb8` changes 47 lines across two files; `9c5883f` changes three lines;
  `ddac71c` separates 49 test lines; and `27cf90a` changes 54 lines across three
  files. The production `schema.rs` is 766 lines without weakening or excepting the
  mandatory gate.
- Daybreaker manually approved all pushed semantic corrections with C=0/H=0/M=0.
  For version 38 it found only one fixed 32-byte comparison per reusable write and no
  index, state, runtime read, or measurable latency concern. The critical
  web reviewer independently found the same uniqueness defect during its broader
  admission-plan review; its commit-specific final review is still pending and is not
  claimed here.
- No Deep Scan, automated security scanner, provider, product-browser flow, or
  Computer Use resource ran for this schema-only correction.

## Atomic human admission owner

Commits `657701a` through `28d5d56` implement the durable boundary without activating
an HTTP route. `SqliteStore::admit_human` owns the exact retry/current-gate ordering,
identity resolution, capacity decision, invite consumption, profile and participant
state, canonical room events, session replacement/insertion, public result snapshot,
and deterministic raw bearer return in one SQLite transaction. The serialized result
contains no raw bearer or credential. Events and replaced fingerprints are returned
only after commit for the RoomRuntime post-commit owner.

### Prior cost or threat and change intent

- The original admission coordinator is a 740-line multi-store saga with intermediate
  workflow writes and compensations. The concrete migration risk was partial durable
  authority after a crash or late write failure. The Rust change uses the existing
  single-connection SQLite writer and one transaction; it does not introduce a saga,
  repository hierarchy, background worker, or generic unit-of-work abstraction.
- A detached issuer could mint a bearer before a transaction had selected a new or
  exact-live durable row. Commit `ee747ab` removes that public issuer; the persistence
  owner performs one HMAC-SHA256, one unpadded base64url encoding, and one SHA-256 of
  the final 48-byte ASCII bearer only on those two successful branches.
- Optional admission avatars may contain up to 10 MiB. The custody lookup selects
  state, room, two fixed fingerprints, expiry, type, size, SQLite `length(content)`,
  and timestamp; it does not return or decode the attachment BLOB in Rust. This
  avoids a concrete Rust heap copy while reusing the public attachment read's stored
  integrity checks. SQLite still evaluates the BLOB length, so no disk/CPU or latency
  number is inferred without a route benchmark.
- Manual Daybreaker review found a concrete corruption path in the first durable
  implementation: invalid participant/profile authority could be interpreted as
  liveness loss, commit `state='ended'`, or let a reusable profile patch silently
  repair revision zero. Commit `c99a031` validates room/participant JSON binding,
  human type, and profile revision before liveness or patching. Corruption now returns
  `invalid_state` and rolls the whole transaction back. Commit `28d5d56` narrows
  exact-row writes further: only actual wall-clock expiry materializes an active row
  as ended; active+inactive-room or active+non-Joined membership is an impossible
  atomic lifecycle state and fails without a repair write.
- Server clocks can carry nanoseconds while SQLite timestamps and the original
  Python contract are microsecond precision. Before `28d5d56`, result JSON could
  retain a nanosecond that the session column truncated, breaking the first exact
  retry. Admission now canonicalizes server-owned `now` once at entry and shares that
  exact value through every gate, profile/event/result, and session write.

### Preserved contract and actual verification

- One-use exact retry precedes later invite gates; reusable exact retry follows the
  current invite gates. Same payload returns the original result and recovered live
  bearer without another use/event, while changed payload conflicts and expired
  completed admission remains terminal.
- Reusable admission preserves room-owned role and mute, updates only person-profile
  display/avatar authority, reuses the concrete cross-room projection function, and
  replaces only the same participant's live session. New profiles use the admitted
  human constructor; existing profiles are validated before patching.
- A trigger rejecting the final session insert proves invite use, profile, device,
  participant, event, and session writes all roll back. Controlled participant-type
  and profile-revision corruption proves exact retry leaves the session active and
  reusable re-admission consumes no invite use instead of repairing authority.
- The complete persistence suite passed 156/156 after `28d5d56`. Warning-denied
  all-target/all-feature Clippy, formatter, workspace check, architecture,
  source-growth, policy, and diff gates passed. The production transaction and
  identity modules are 664 and 276 lines respectively; the shared attachment owner
  is 731 lines. No 800-line gate exception or threshold change was added. The latest
  correction was 217 insertions and 25 deletions across four files, below the
  1,000-line commit review threshold.
- Daybreaker returned `C=0/H=0/M=1` on the pre-fix implementation, then manually
  approved `c99a031` and the complete admission series with `C=0/H=0/M=0`. It
  confirmed structural validation precedes liveness, reusable profiles use the
  shared validator, and errors escape before commit. A later critical web review
  returned `C=0/H=0/M=3`: one finding was already closed by `c99a031`; microsecond
  clock equality, exact terminalization scope, and avatar integrity are closed by
  `28d5d56`. Both reviewers are manually rereviewing the latest HEAD, so final
  cross-review approval is not yet claimed. No Deep Scan, automated scanner,
  provider, browser flow, or Computer Use resource ran for that durable-owner
  increment.

## Bounded RoomRuntime admission ownership

Commits `29d3d66`, `cf29ecd`, and `06587b0` connect the durable UOW to the existing
room mutation owner without exposing an HTTP route. The original `room_runtime.rs`
was already 790 lines. The structure commit moved existing provider-result handling
and publication retry setup to their existing owning modules; it added no state or
behavior and lowered the file before admission integration rather than weakening the
800-line gate.

- Queue cost and intent: human admission is a second variant in the existing
  128-slot `RoomMutation` queue, not a second channel. Commands and admissions share
  the same total pending bound and per-room serialization. Full/closed queues return
  the existing `room_busy`/`room_unavailable` failures without waiting.
- Routing cost and authority: authenticated signed evidence already carries its room
  claim and needs no routing query. A join code performs one indexed, one-column
  `room_id` lookup because the reachable frontend omits `meeting_id`. This lookup is
  not authority and deliberately ignores revoke/expiry/use gates so it cannot break
  one-use exact retry ordering; the queued transaction re-resolves every durable
  binding and gate.
- Cancellation and post-commit contract: queue acceptance transfers request custody
  to the room task. A dropped reply receiver does not cancel the dequeued transaction.
  After commit, the target room drains its durable publication cursor, exact replaced
  session fingerprints are broadcast, and only then is the raw bearer sent through
  the non-debuggable oneshot result. Other-room profile projection events remain in
  their own durable histories and are drained only by those rooms' existing retry or
  startup owners; they are never sent through the target room broadcaster.
- Actual verification: three focused runtime tests prove dropped-reply completion,
  joined/update publication plus replacement notification, and an ordered cross-room
  case where another room's profile event precedes the target join without crossing
  streams. Before HTTP activation, the complete server run passed 52 unit and 31
  integration tests. Clippy,
  `make check`, architecture/source-growth/policy gates passed. The runtime commit is
  437 insertions/25 deletions; `room_runtime.rs` is 798 lines and the focused
  admission module is 314 lines, with no threshold exception. These tests establish
  ordering and bounded state, not HTTP latency or throughput improvement.

These are operation counts, bounded data sizes, and observed contract results, not
an end-to-end performance claim. CPU time, heap peak, SQLite page growth, and request
latency will be measured only after the complete browser flow exists; speculative
caches, batching, cleanup workers, and future-provider abstractions remain excluded.

## Local HTTP preflight and admission boundary

Commit `b32c2b7` mounts the current `/api/room-invite/admission` and
`/api/room-invite/join` contracts without creating another admission owner.

- Prior cost and threat: the copied frontend reached 404 despite the durable UOW and
  bounded room actor being complete. Raw invite, `aad1_`, and optional `aas1.` values
  are replay authority and cannot enter serialized internal messages, diagnostics, or
  persistence. The old frontend helper still generates weak fallback device values,
  so mounting the route must not be confused with activating the browser flow.
- Intent and smallest design: one 434-line adapter owns only bounded decode,
  credential authentication, public projection, and status mapping. It reuses the
  shared 16 KiB Axum decoder, existing 128-connection/deadline owner, canonical raw
  credential authenticators, read-only preflight snapshot, and RoomRuntime admission
  queue. There is no HTTP workflow, new queue, cache, limiter, background task,
  compatibility credential, or alternate mutation path.
- Preserved contract: both current invite credentials work, invite-auth failures do
  not disclose signature/format detail, `room`/`read_only` and all current preflight
  decisions keep their copied frontend shape, admission returns the original bounded
  result plus exact live bearer, and every rejection maps from the deciding durable
  authority. Handler cancellation after queue acceptance cannot cancel the room
  owner; exact retry remains the response-loss recovery mechanism.
- Resource and security evidence: bodies are buffered once with a 16 KiB ceiling and
  responses carry `private, no-store`. Fixed browser/session headers are shape-checked
  before owned copies and before body decode. A successful response consumes the
  already published commit to move, rather than clone, the result and bearer. These
  are observed operation and allocation bounds, not benchmark evidence; no latency,
  CPU, memory, disk, or throughput improvement is claimed.
- Verification: a real loopback server test checks missing browser authority,
  uniform invalid-invite preflight, exact CORS headers, no-store, read-only profile
  preflight, actual queue/UOW admission, 48-character `aas1.` issuance, byte-identical
  exact retry after one-use exhaustion, and changed-payload 409 with use count still
  one. The complete server run passed 52 unit plus 32 integration tests, with zero
  failures. Warning-denied all-target/all-feature Clippy, formatter, workspace check,
  architecture/source-growth/policy gates, and `make check` passed. The commit is 792
  insertions and 2 deletions, below the mandatory split threshold.
- Explicit incompleteness: the copied browser now produces the canonical `aad1_`
  credential and can call the local preflight/join routes, but the Rust pre-join
  avatar upload is implemented by later commit `cc57217`. Live-session ticket
  exchange, session-authenticated WebSocket, leave/revoke, and trusted public ingress
  remain unavailable. No packaged frontend parity or Computer Use result is claimed.

## Fail-closed browser credential owner

Commit `caf9e37` cuts every current browser identity caller over from the copied
fallback-capable helper to one canonical durable credential owner.

- Prior observed threat: the copied helper accepted arbitrary stored strings with a
  length of eight and generated `Date.now`/`Math.random` values when native UUID or
  storage failed. The latter could be weak and page-scoped. Because the durable
  admission and pending-upload subjects include this credential, regeneration can
  change response-loss custody and browser-bound quota identity rather than reporting
  that durable identity is unavailable.
- Intent and smallest design: one new origin-storage key owns exactly `aad1_` plus
  canonical unpadded Base64url for 32 `crypto.getRandomValues` bytes. Creation performs
  one fixed-size random fill, one write, and one readback equality check. Reuse reads
  and canonically decodes/re-encodes only 32 bytes. The old key remains untouched and
  is never a migration or compatibility input. No cache, context, background work,
  package, alternate generator, or ephemeral success path was added.
- Preserved contract: the same value is supplied through the existing `device_token`
  body/header fields to preflight, admission, pre-join upload, recovery, pairing,
  host claim, profile, and preference paths. Unsupported WebCrypto, storage access or
  durability failure, and malformed persisted values are visible failures. Preflight,
  join, and pre-join upload tests prove those failures occur before network adapter
  invocation; stored malformed values are not repaired or deleted.
- Actual cost and verification: first use adds one exact storage readback after the
  write; subsequent use is one storage read plus fixed 32-byte validation. No CPU,
  heap, disk, or latency improvement is claimed. All 77 frontend test files and 385
  tests pass. The production TypeScript/Vite build and exact copied-CSS cascade/hash
  verification pass. `make check` passes architecture, source-growth, policy,
  formatting, and workspace compilation gates. The commit is 361 insertions and 47
  deletions across 11 files, below 1,000 lines.

## Bounded pre-join avatar persistence owner

Commit `facaaab` implements the durable pre-join avatar write and quota boundary
without making the still-missing HTTP upload route appear complete.

- Prior observed cost and threat: the original filesystem path scans and parses the
  attachment directory, deletes an exact-custody predecessor, rescans, and writes the
  file and JSON metadata as separate filesystem objects. Checking invite authority
  only before decoding would also permit a revoke/use-limit race to commit afterward;
  checking only afterward would spend bounded decoder capacity for an already invalid
  request. Review of `cc57217` found a narrower gap: canonical join-code parsing is
  not durable authentication, so arbitrary `aaj1_` values could allocate a ten-MiB
  Base64 output before the indexed invite lookup. Across the 128 admitted connections,
  that was a 1.25-GiB additional decoded-output bound plus synchronous decode CPU.
- Intent and smallest design: `81c04e7` splits preauthorization from storage without
  adding another authority owner. `SqliteStore::authorize_human_prejoin_avatar`
  checks current invite/room/use-limit state and returns one opaque capability with
  private fields. `store_human_prejoin_avatar` accepts only that type, reuses the
  existing bounded PNG canonicalizer, and repeats the durable check in the final
  SQLite transaction. The transaction reuses the one
  existing attachment table, deletes expired pending rows, computes invite,
  pending-room, room, and runtime quotas in one aggregate, excludes/deletes only the
  exact custody predecessor, and inserts one one-hour `admission_pending` row. No new
  store, table, decoder, queue, task, trait, compatibility path, or fallback exists.
- Preserved product and security contract: exact invite-plus-browser custody, shared
  signed-invite quota provenance, all original item/count/byte limits, atomic exact
  replacement, one-hour expiry, and admission-owned transfer remain unchanged.
  Bound admission assets keep invite/room/runtime provenance but do not consume the
  admitted user's ordinary uploader quota. Raw invite/browser credentials never
  enter this persistence API, rows, errors, or fixtures.
- Actual bounded cost: rejected current authority performs one indexed invite/room
  read and no Base64 output allocation, image decode, or BLOB write. The body-carried
  credential contract still requires the adapter to buffer a JSON envelope capped at
  14,046,552 bytes under the ten-second deadline and 128-connection process bound; the
  patch does not claim to remove that accepted-body cost. A valid write still performs
  exactly two current-authority reads
  around one existing decoder admission (two concurrent permits; 10 MiB input,
  4,096-pixel dimension, 16 Mi-pixel, and 72 MiB allocation bounds), one cleanup
  delete, one conditional aggregate over the live set bounded by 4,096 records, one
  exact delete, and one canonical PNG insert. The duplicate authority read is the
  explicit TOCTOU cost. No CPU, memory, disk, or latency improvement is claimed
  without representative measurement.
- Verification result: the three focused persistence tests cover unknown durable
  invite rejection, exact replacement/isolation, canonical metadata and one-hour TTL,
  stale preauthorization rejection after revoke without mutation,
  shared eight-item invite quota, replacement while full, ordinary-uploader quota
  isolation, and rejection at 4,096 live pre-join rows. All 159 persistence tests
  pass. `RUSTFLAGS='-D warnings' cargo clippy --workspace --all-targets --all-features`
  and `make check` pass. The original persistence commit is 537 insertions/8
  deletions; correction `81c04e7` is 198 insertions/108 deletions across four files.
  The current 575-line module and 737-line canonicalizer owner both pass the unchanged
  800-line source gate.

## HTTP review corrections

Critical web review of `b32c2b7`/`34d7149` returned `REVISE` with zero Critical,
zero High, and two Medium findings. Commit `888084e` closes both before another
admission surface is added.

- Prior observed mismatch: `RequestBodyDeadlineLayer` already bounded every body to
  ten seconds, but the shared Axum collector mapped every collection error to 413.
  A client that declared a body within the route limit and then stalled therefore
  received `payload_too_large`, while original commit `d504647` returned HTTP 408 for
  `RequestBodyDeadlineExceeded`. Separately, participant collision returned
  `identity_conflict` instead of the reachable original
  `participant_identity_conflict` code.
- Intent and preserved contract: the shared decoder inspects the maintained body
  error chain for `tower_http::timeout::TimeoutError`, returns typed
  `RequestTimeout`, and maps that to 408 `request_timeout` at every existing HTTP
  adapter. Declared or collected size overflow remains 413. The host-ticket empty-body
  route reuses this same owner, and join maps only the public collision string back to
  `participant_identity_conflict`; persistence authority and its typed rejection are
  unchanged. The existing ten-second timer, connection/route bounds, queue ownership,
  cancellation behavior, and retry semantics are untouched.
- Cost and verification: successful requests execute no new branch beyond the same
  `Result` mapping. Only a failed body collection walks its short error-source chain;
  no allocation, timer, task, state, retry, or dependency was added. A deterministic
  `DeadlineBody` test completes in milliseconds and distinguishes 408 timeout from
  413 length overflow. An adapter test pins 403
  `participant_identity_conflict`. The bearer retry assertion no longer formats its
  ephemeral token on failure. Warning-denied workspace Clippy, 54 server unit tests,
  every server integration test, and `make check` pass. The implementation/test
  commit is 109 insertions and 24 deletions across eight files.

## Reachable pre-join avatar HTTP flow

Commit `cc57217` connects the copied guest profile panel's existing request shape to
the reviewed persistence owner without adding another product surface.

- Prior observed gap and threat: `/api/attachments` required a profile ticket even
  when the current guest UI sent an invite and durable browser credential before
  admission, so the real flow returned 401. Header-based authenticated writes must
  still consume their one-use ticket before body decode; an invalid supplied header
  cannot become an invitation to try the public branch. Returning an unreadable
  pending URL would also make the guest's immediate `<img>` preview fail.
- Intent and preserved contract: the one existing handler selects its authority from
  Authorization header presence. Supplied tickets are consumed first and never fall
  through. No-header requests decode only the existing bounded envelope, accept only
  `profile_avatar`, parse invite and canonical `aad1_` credentials, obtain current
  durable invite/room authorization before Base64/image work, and pass only the opaque
  authorization to
  `store_human_prejoin_avatar`. Existing local/session profile uploads are unchanged.
  The existing public attachment lookup admits only bound images or unexpired
  `admission_pending` images by opaque UUID; ordinary pending rows remain invisible.
  Admission retains final exact attachment/custody/invite/room/TTL/integrity checks
  before the profile and room projection can reference it.
- Actual cost and security boundary: body buffering stays capped at 14,046,552 bytes
  with the existing ten-second deadline. Invalid durable authority now stops after an
  indexed read without allocating the decoded output; valid public input incurs that
  read, one bounded Base64 decode, the already measured shared decoder
  bounds, and the reviewed SQLite write. Preview is one primary-key/state/expiry
  lookup plus the canonical BLOB read. The raw payload type is not `Debug` or
  `Serialize`; raw invite/browser credentials remain HTTP-local. The live opaque URL
  is a bounded read capability for one avatar only and conveys no mutation or room
  authority. No cache, new route, grant, ticket, table, task, queue, retry, or retained
  state was introduced. No CPU, memory, disk, or latency improvement is claimed.
- Verification result: the real loopback test proves invalid Authorization wins over
  malformed JSON; an arbitrary canonical unknown `aaj1_` carrying valid Base64 for a
  ten-MiB output is rejected as `invite_invalid` at the durable pre-decode gate;
  per-browser custody isolation, exact replacement/old-URL 404, live
  canonical preview, atomic admission binding to the selected avatar, exact retry,
  and post-admission reads. The existing profile test continues to prove ordinary
  pending rows return 404. All 159 persistence tests, 54 server unit tests, all server
  integration tests, warning-denied workspace Clippy, and `make check` pass. Commit
  `cc57217` is 182 insertions/12 deletions; correction `81c04e7` is 198
  insertions/108 deletions across four files. The production modules are 390, 575,
  and 737 lines and pass the unchanged source gate.
  Daybreaker Blue High and the critical web reviewer then manually re-read the
  correction and both returned `APPROVE` with Critical 0, High 0, and Medium 0. The
  web review explicitly checked the pre-decode durable ordering, opaque capability
  custody, final transactional recheck, residual 14,046,552-byte envelope cost,
  ten-MiB regression input, source gates, and credential-safe errors. Neither review
  used Deep Scan, another automated scanner, or a real provider.

## Durable human-session authorization owner

Commit `28babe8` implements the raw-free persistence contract needed by the still-
unmounted public session exchanges; it does not mark WebSocket, profile, preference,
or attachment session flows complete.

- Prior observed duplication and threat: the original verifies a fingerprinted
  session and then lets routes resolve user/room/membership separately. Rust preflight
  likewise had a private session query. Copying that logic into exchange and target
  routes could let liveness, corruption, profile binding, or scope decisions diverge.
- Intent and preserved contract: one indexed joined snapshot now owns active state,
  expiry, browser client kind, scope, Active room, Joined exact human participant,
  and exact revisioned profile binding. It returns a non-serializable private-field
  `HumanSessionAuthorization` with only fingerprint provenance, derived principal,
  and expiry; no raw bearer crosses persistence. Preflight reuses the resolver while
  preserving unknown/foreign-room `NotApplicable`, same-room unavailable status,
  profile display-name SSoT, and read-only non-posting capabilities. Invite revocation
  still does not revoke an already committed live session.
- Actual bounded cost: one authorization begins a read transaction, performs one
  session-fingerprint lookup with profile/participant/room primary-key joins, decodes
  one room, participant, and profile JSON value, and commits the read. This adds a
  room join/decode to the previous preflight path in exchange for deleting its second
  session resolver. No performance improvement is claimed without representative
  measurement. No table, index, cache, route, grant, task, timer, or fallback exists.
- Verification result: five existing preflight tests continue to pass. A new
  real-admission test fixes exact fingerprint/principal/profile projection, read-only
  scope/capabilities, participant-left rejection, and corrupt profile-revision
  failure. All 160 persistence tests, warning-denied persistence Clippy, and
  `make check` pass. Commit size is 387 insertions/35 deletions across four files;
  the 189-line owner and 168-line focused test module pass the unchanged source gate.

## Bounded public human-session grant owner

Structure-only commit `294b239` separates the pre-existing ticket tests from their
single implementation owner without changing behavior. Commit `7af1345` then adds the
in-memory contract required by the still-unmounted typed public exchanges.

- Threat and intent: a global-only 4,096-entry limit allowed public sessions to starve
  private control. The existing store now admits at most 1,792 public grants, eight per
  exact session fingerprint, and therefore preserves 2,304 production entries for
  local/private authority. It keeps one mutex and one map; the existing expiry sweep
  also counts public and same-session entries, so no second state owner exists.
- Preserved contract: the grant owns the opaque persistence-issued authorization and
  exactly one of WebSocket connect, own profile, preference read, or preference write.
  Wrong-purpose presentation consumes the grant. Grant issuance caps its monotonic
  deadline at session expiry, and consumption also rejects an authorization whose
  absolute expiry has passed. Read-only sessions cannot mint preference-write
  authority, and private ticket behavior is unchanged. The public exchange routes
  and target durable revalidation are not mounted yet and are not counted as
  reachable parity.
- Measured cost: the one-time same-toolchain size probe measured 168 bytes for
  `HumanSessionAuthorization`, 176 for its public grant, and stored-entry growth from
  160 to 216 bytes. At the hard 4,096-entry process bound that is at most 229,376
  bytes of inline slot growth; principal/ticket/proof string heap capacity, `HashMap`
  allocation, and allocator overhead are excluded, and no total-heap bound is
  claimed. The store adds no disk state. Five warmed debug runs of the exact boundary test measured
  the first 16 uncontended public issue calls at 9.1–10.1 microseconds average and
  18.6–23.5 microseconds maximum, excluding durable authorization but including mutex
  acquisition, sweep/count, UUID generation, and insertion. This is local diagnostic
  evidence, not a production latency claim. The private algorithm remains its previous
  single expiry pass and insert; no speculative rate limiter or accounting index was
  introduced.
- Verification result: all test authorizations come from real SQLite admissions.
  Nine focused tests cover exact purpose, replay, read-only write denial, the eighth
  and ninth outstanding grants, consumption reclamation, a scaled 16-public boundary
  beside the exact 2,304 private reserve, full-store rejection, and the production
  1,792 calculation. All 57 server unit tests and every server integration test pass;
  warning-denied server Clippy and `make check` pass. The implementation diff is 504
  insertions and 18 deletions across three files, below 1,000 lines, and the 757-line
  implementation passes the unchanged source gate.

## Human-session profile target revalidation

Commit `8efaa25` implements the persistence target behind the still-unmounted public
profile grant. It does not change the current profile HTTP adapter or frontend.

- Security contract: the target resolves the grant's exact session fingerprint in the
  same transaction as the read or write and compares expiry, room, user, participant,
  browser client kind, scope, operator state, and derived capabilities. A current
  profile display-name change is allowed because profile state owns that mutable value;
  a changed immutable session value is corrupt provenance. Ended/expired sessions,
  inactive rooms, left participants, and missing or corrupt profile bindings fail
  closed. Profile writes keep avatar ownership and every room projection/event in the
  same transaction.
- Smallest-design and cost evidence: the existing resolver now returns the full
  profile it had already decoded, so the target does not issue a second profile query.
  The profile is boxed once inside the internal resolution enum to satisfy the
  warning-denied large-enum boundary; it is neither cached nor retained after the
  operation. A read remains one transaction and one indexed session query with three
  primary-key joins. A write adds the unchanged profile mutation/projection work. No
  route, table, index, cache, task, timer, retry, compatibility path, or fallback was
  added, and no unmeasured latency improvement is claimed.
- Verification result: one real read-only admission proves profile read/update,
  mutable display-name continuity, changed-expiry rejection, post-grant participant
  leave rejection for both read and write, rollback of the rejected patch, and corrupt
  profile-revision failure. All 160 persistence tests, warning-denied persistence
  Clippy, and `make check` pass. The diff is 166 insertions and 8 deletions across four
  files; the 228-line authority and 639-line profile owner pass the unchanged source
  gate. HTTP consumption and frontend use remain explicitly incomplete.

## Public human-session grant manual-review findings

- The web review found that monotonic grant expiry alone could outlive the durable
  session after a forward wall-clock change. Commit `9e3c874` keeps remove-first
  consumption and rejects absolute session expiry before returning authority; a
  controlled-time regression proves the rejected grant cannot be replayed.
- The same review found that 229,376 bytes described only inline enum growth, not
  string heap capacity or map/allocator overhead. The resource claim above is now
  limited to what the same-toolchain `size_of` probe measured.
- Daybreaker found that the production wall clock was sampled before waiting for the
  grant mutex, so a forward clock change during that wait could compare the backing
  session against stale time. Commit `12edda4` keeps remove-first consumption, then
  samples production time and runs the shared purpose/absolute-expiry resolver; the
  test seam injects time only after the same removal boundary.
- Final outcome: the web reviewer and Daybreaker both returned `APPROVE` with
  Critical 0, High 0, and Medium 0. They found no remaining duplicate policy owner,
  unnecessary state/abstraction, overimplementation, or structure-gate issue.

## Asset custody storage correction

Public commits `334c918`, `d337003`, `23571fe`, and `ac542de` replace the superseded
combined avatar state space with clean schema 40. No schema conversion, compatibility
branch, fallback, background sweeper, cache, configuration layer, generic asset trait,
or speculative message-attachment owner was added.

- Prior cost and defect: one 12-column `profile_attachments` row shape represented
  profile and pre-join ownership through three states and nullable user, room,
  custody, and invite provenance. Profile uploads also inherited 64-item/128-MiB
  uploader policy, while pre-join writes separately scanned conditional invite and
  room quota state. A promoted pre-join row retained provenance that no longer owned
  the profile asset.
- Intent and preserved contract: `profile_avatar_assets` has only `pending` and
  `current`, with one unique `(owner_user_id, state)` owner; `prejoin_avatar_assets`
  has no state and permits one row per exact custody fingerprint. A new pending upload
  deletes only its exact predecessor. Admission copies the exact BLOB metadata and
  opaque ID into the user's pending lifecycle, deletes the exact pre-join row, and
  promotes it in the same SQLite transaction. Failure rolls back every step. Opaque
  URLs, canonical PNG validation, one-hour pre-join expiry, final invite/browser/
  room revalidation, profile projection, and retry behavior remain unchanged.
- Resource and deletion evidence: the current profile row has nine columns and the
  pre-join row ten, so neither pays for the other lifecycle's nullable state. The
  4,096-live-item/8-GiB absolute database bound and checked
  `current - exact predecessor + new` calculation have one implementation owner.
  The runtime 10-MiB owner is bound directly into one schema contract test: all
  three asset tables accept the exact runtime value and reject that value plus one,
  so the unavoidable SQLite literal mirrors cannot drift silently. Expired
  cleanup SQL remains in its lifecycle module and only deletes that lifecycle's
  pending rows; no limit path evicts current, bound, foreign, referenced, or merely
  old data. The profile 64-item/128-MiB and pre-join invite/room operating quotas and
  their unused indexes are gone.
- Residual availability threat: one valid reusable-invite holder can rotate browser
  credentials and eventually occupy the absolute live-asset ceiling. Existing HTTP
  connection/decode/deadline bounds slow attempts but do not prevent that occupancy.
  The explicit current policy forbids a hard-coded invite/room operating quota and
  forbids evicting another live custody, so the server fails closed at the absolute
  ceiling and relies on one-hour expiry. No stronger fairness claim is made.
- CPU/disk/latency evidence and trade-off: the shared live-usage statement now has
  three `UNION ALL` branches and computes count plus bytes from that one stream,
  instead of separately counting and summing the old stores. SQLite
  `EXPLAIN QUERY PLAN` showed the pre-join expiry branch using its expiry index. It
  also showed the profile OR predicate scanning before `23571fe`; replacing, not
  adding to, the old partial expiry index with `(state, expires_at)` made both live
  branches and expired-pending deletion indexed. Admission ownership transfer uses
  insert/delete rather than an in-place cross-owner state mutation, so it performs
  more SQLite statements; this is the explicit cost of removing impossible shared
  state, and no latency improvement is claimed. The unconnected room-appearance
  branch remains an empty-table scan; no speculative index was added for an absent
  writer.
- Verification result: 65 consecutive profile uploads prove exact pending replacement
  without the old user quota. Nine distinct pre-join custodies prove removal of the
  generic invite quota. At 4,096 live rows, exact replacement succeeds and net growth
  fails. Admission rejects corrupt metadata without consuming invite/session state,
  then transfers and promotes the repaired exact row. Four focused schema tests prove
  one current plus one pending profile avatar, one pre-join row per custody with no
  state column, exact agreement with the runtime item bound, and uploader deletion
  removing only pending—not bound—room assets. All 164 persistence tests, all 58
  server unit tests and server integration tests,
  warning-denied persistence/server Clippy, and `make check` passed. The production
  modules are 711, 591, 477, 286, and 251 lines; no 800-line exception or gate change
  was made.

## Asset custody manual-review findings

- The web review found that the first shared-bound macro still encoded the 10-MiB
  limit independently for Rust and SQLite, so either representation could drift.
  Commit `cada7fb` removes that macro and binds the three schema boundary checks
  directly to the runtime `MAX_RASTER_BYTES` owner.
- The web review found that the first index commit did not itself record evidence for
  replacing the pending-only profile index with `(state, expires_at)`. Commit
  `7ebf90e` records the observed pre-change scan, post-change indexed plans, and
  disk/write trade-off. It also corrects the SDD's stale description of the combined
  table from current state to historical defect.
- Daybreaker identified that one valid reusable-invite holder can exhaust the global
  asset ceiling through distinct browser custodies. The accepted current policy
  forbids fixed invite/room operating quotas and foreign-custody eviction, so this is
  the documented residual availability risk rather than an unclaimed mitigation.
- Final outcome: the web reviewer and Daybreaker both returned `APPROVE` with
  Critical 0, High 0, and Medium 0. They found no remaining related duplicate policy
  owner, ownership/lifecycle boundary defect, unsupported optimization,
  overimplementation, removable compatibility state, or structure-gate issue.

## Reachable human-session profile exchange

Public commits `51905c4`, `da372d8`, `868d41c`, `7b3f3bc`, `9fb9ce8`, and
`83e1b0b` connect the existing durable human-session authorization and profile target
to the copied profile UI. Commit `8cc1064` corrects the pre-admission UI boundary
found by the real-client run.

- Prior threat and authority boundary: accepting the raw `aas1.` session at
  `/api/user-profile` or `/api/attachments` would turn a long-lived room credential
  into a generic profile bearer. A grant issued before a participant left, a room
  ended, a profile binding changed, or the session expired also could not authorize a
  later mutation. The dedicated empty-body exchange consumes the raw session only at
  `/api/session-tickets/profile`, returns a one-use profile ticket, and the target
  revalidates the exact durable session/profile/participant/room state in the same
  transaction as the read or write. The raw session is rejected at both profile
  targets. Read-only humans may edit their person profile but cannot publish a new
  avatar, preserving room posting scope without conflating it with person identity.
- Smallest design and resource cost: the implementation reuses the existing bounded
  ticket store, typed durable authorization, profile transaction, Axum route registry,
  attachment decoder, and maintained no-store response layer. It adds no cache,
  retry, session mirror, trait, table, index, timer, background task, compatibility
  branch, or fallback. Each profile read, patch, or avatar upload intentionally pays
  one extra short HTTP exchange plus one indexed SQLite authorization before the
  existing target work. Browser requests use `cache: no-store`; every exchange and
  target response uses `Cache-Control: private, no-store`. No CPU, memory, disk, or
  latency improvement is claimed without a representative workload.
- Frontend state boundary: the copied API obtains a fresh ticket for every operation
  and never stores or reuses it. Before admission, the guest panel displays only the
  profile being submitted with the invite and does not request or edit a server-owned
  profile. Once admission returns the `aas1.` session, the panel hydrates from the
  server SSoT and all later changes use the typed exchange. This keeps the pending
  invite profile, person profile, room role/join/mute/permissions, and Agent Session
  profile as separate authorities.
- Verification result before manual review: the canonical HTTP boundary rejects a
  raw session at the target, rejects a nonempty exchange body, proves one-use replay
  failure, profile read/update, current-session revalidation, read-only profile edits,
  read-only avatar denial, avatar replacement, and `private, no-store`. The copied
  production frontend was driven through preflight and admission against a disposable
  canonical SQLite/Axum fixture. It saved and freshly re-read display name `Guest
  Verified`, status `AgentsAssemble Verified`, and a selected/cropped PNG; the same
  image appeared again in the profile editor and left-bottom profile. That run exposed
  the pre-admission unauthorized profile read fixed by `8cc1064`; the focused contract
  test now proves zero profile requests before a session exists and the normal server
  read immediately after admission. All 385 frontend tests and the production build
  pass. The disposable tab, servers, identity data, fixture state, and unique bundles
  were stopped and moved to the recoverable Trash; no verification-owned listener or
  app remained.

The authenticated human room WebSocket and preference exchanges are separate pending
slices. The profile run therefore does not claim a canonical room snapshot, roster,
message publication, remote preferences, or public-ingress parity.

## Human-session profile exchange manual-review findings

- The web reviewer and Daybreaker found one Medium cache-isolation defect: exchange
  errors, profile target responses, and browser requests did not all enforce no-store,
  while identical server cache literals had no single policy owner. Commit `644b1d5`
  centralizes the server header, applies it to every affected route response, removes
  the duplicate attachment literal, applies browser no-store to each exchange and
  target request, and pins both sides with contract tests.
- Final outcome: the web reviewer and Daybreaker both returned `APPROVE` with
  Critical 0, High 0, and Medium 0. They found no remaining related duplicate policy
  owner, ownership/lifecycle boundary defect, unsupported optimization,
  overimplementation, removable compatibility state, or structure-gate issue.

## Reachable human-session WebSocket and browser entrance

Public commits `24a2179`, `aadbe99`, and `1ef79f3` separate the human-session ticket
authority, activate exact-session WebSockets, and connect the copied browser
transport. The current candidate corrects the exact production entrances and guest
server-surface bootstrap discovered by the real-browser run.

- Preserved authority: raw `aas1.` bearer input exists only at the typed WebSocket
  ticket exchange. Subscription begins before consume; exact durable session state is
  revalidated after consume, for each inbound command, before each outbound product
  frame, at expiry, and inside the mutation transaction. Read-only sessions cannot
  post. Replacement, revoke, leave, room close, expiry, or notification lag closes or
  denies the exact socket without a polling cache or client-owned authority.
- Browser correction: exact original `/join`, `/join/`, `/pair`, and `/pair/`
  entrances serve the production bundle and `/assets` serves its root-relative Vite
  assets. Successful admission carries the already-owned server ID, lineage, and
  product surface. The existing strict directory contract validates and binds that
  surface before session persistence/token exposure. Stored pre-contract sessions and
  failed-join restoration are rejected rather than treated as compatibility paths.
- Actual cost and structure: successful admission adds one bootstrap-status SQLite
  read and one bounded surface object in the response/session. Browser binding adds
  one SHA-256 digest calculation. The server reuses its existing product-surface and
  bootstrap owners; the frontend reuses its existing directory pin. No table, index,
  cache, trait, task, timer, fallback, migration, or generic authority framework was
  added. No CPU, memory, disk, or latency improvement is claimed.
- Verification result before manual review: real Axum/SQLite/WebSocket tests cover
  ticket no-store/replay, snapshot, normal post, read-only denial, replacement close,
  and no mutation after final transactional revalidation fails. Production-browser
  Computer Use additionally proves exact entrance, token removal, normal
  snapshot/roster/post, and isolated read-only snapshot/roster/visible denial; SQLite
  contains no denied write. Focused frontend tests prove invalid join and recovered
  surfaces remain unpersisted and expose no bearer. Controlled expiry,
  notification-lag/closure, final-outbound races, and the remaining real-browser
  invite matrix remain open and are not claimed.
