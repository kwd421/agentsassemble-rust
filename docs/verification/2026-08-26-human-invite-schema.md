# Human invite schema verification — 2026-08-26

Status: partial slice evidence; canonical invite reads are implemented, while invite
writes, issuer, routes, admission, and browser flow are not implemented by these
commits.

## Provenance and scope

- Original behavior baseline: `d5046473010d1353a81ee38337360e6d98f7bd6f`.
- Approved Rust design: `bfde3de`.
- Dual-credential schema commit: `b20c3b7`.
- Locator-binding correction: `b46eae9`.
- Canonical invite read boundary: `afd3f6d`.
- The schema is fresh-only at version 35. No migration, compatibility reader,
  fallback column, or partially upgraded authority is accepted.

This increment changes only the durable `room_invites` authority and the fixtures
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
