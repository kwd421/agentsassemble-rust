# Asset Custody and Lifecycle Correction

Status: storage correction implemented through public commit `ac542de`; room-
appearance copied-frontend upload/read and restart lifecycle locally verified

## Definition

Profile avatars, pre-join avatars, room appearance images, and message attachments
have different authority owners and lifetimes. Rust stores and mutates each through
that product owner while sharing only bounded raster validation and exact resource
accounting.

## Corrected baseline defect

- The superseded `profile_attachments` combined user-owned pending/current avatars and
  invite/browser-owned pre-join avatars through one state column and five nullable
  provenance columns.
- Profile upload had inherited the original generic uploader policy of 64 items and
  128 MiB even though one profile can use only one current avatar and one pending
  replacement.
- Profile and pre-join modules duplicated expiry deletion, the 4,096-item/8-GiB
  process ceilings, replacement arithmetic, and SQL over the combined state space.
- A transferred pre-join row retained invite and room accounting provenance after
  it becomes a person profile. That provenance no longer owns the asset.
- `room_appearance_assets.created_by_user_id` was described as quota state even
  after room ownership transfer, although the applied banner or icon belongs to
  the room. The settings event already owns mutation audit.
- Rust still has no message-attachment persistence owner. A schema, trait, or generic
  asset framework for that absent slice would be speculative.

These are implementation defects or stale design assumptions, not behavior that
must be preserved merely because the original constants or current schema contain
them.

## Target contract

### Separate owners

- A profile-avatar table owns only a user's avatar. Its only states are `pending`
  and `current`; one unique owner/state constraint permits at most one of each per user.
  Uploading a new pending avatar atomically replaces only that user's previous
  pending avatar. Applying it promotes that exact pending avatar and deletes only
  the previous current avatar after the profile reference changes in the same
  transaction.
- A pre-join-avatar table owns only one unexpired image for an exact
  invite-credential/browser-credential custody fingerprint. Pending is implicit,
  so it has no state column. A new upload for the same custody atomically replaces
  only its predecessor. Admission atomically moves the exact bytes and opaque ID
  into the admitted user's profile lifecycle and removes the pre-join row; invite,
  browser, and room provenance do not survive as profile ownership.
- A room-appearance table owns pending upload custody and bound room custody.
  Applying a banner or icon clears uploader custody. Bound usage belongs to the
  room, not the uploader. Banner and icon are evaluated together, and an old bound
  asset is deleted only when neither next reference uses it.
- Message attachments remain outside this correction until their reachable server
  writer is implemented. They will have message/room retention rather than being
  inserted into a profile or appearance lifecycle in advance.

### One safety owner, no generic asset framework

One small persistence module owns only the limits and arithmetic currently shared
by implemented raster stores:

- 10 MiB canonical bytes per image;
- 4,096 by 4,096 maximum dimensions, 16 Mi-pixels, 72 MiB decoder allocation, and
  the existing two-job decode admission bound;
- the 4,096-live-asset and 8-GiB absolute SQLite storage ceilings across current
  profile, pre-join, and room-appearance rows; and
- checked replacement usage `current - exact predecessor + new` for both count and
  bytes.

The per-user 64-item/128-MiB policy is removed. Per-uploader and per-room operating
quotas are not hard-coded here; a future request may make them configurable, but
this slice adds no configuration layer. The fixed limits above remain because they
bound one hostile decode and total local database growth, not because the original
used the same numbers. Lifecycle cardinality supplies the profile limit. Pre-join
expiry and exact-custody replacement bound stale work without deleting other
subjects' live assets.

The shared module is not a trait, repository abstraction, asset registry, policy
engine, or future message-attachment API. SQL authority, state transitions, and
deletion stay in their product lifecycle modules.

### Deletion and failure

- Cleanup deletes only expired pending rows owned by its own lifecycle.
- Replacement deletes only the exact predecessor whose reference is superseded in
  the same transaction.
- A limit failure never evicts a referenced, current, bound, foreign, or merely old
  asset. Transaction failure preserves both the old asset and old reference.
- Usage corruption, arithmetic underflow/overflow, cross-owner reference, stale
  custody, and malformed stored bytes fail closed.
- The public opaque attachment URL shape, canonical static PNG bytes, no-store
  reads, pre-join preview, admission result, profile projection, retry behavior, and
  room-settings authority remain unchanged.

Residual availability threat: a holder of one valid reusable invite can mint many
browser credentials and occupy the absolute live-asset ceiling with distinct pre-join
custodies. The 128-connection, two-decode, and request-deadline bounds limit rate, not
eventual occupancy. The current product decision does not hard-code an invite/room
operating quota or silently evict another custody, so exhaustion fails closed until
rows expire. A later configurable operating policy may address fairness; this slice
does not disguise that policy choice as an absolute safety bound.

## Non-goals

- no migration or compatibility path for the superseded schema;
- no background sweeper, cache, filesystem copy, generic garbage collector, quota
  configuration service, message-attachment table, or client-owned cleanup; and
- no room-appearance route activation before its complete authority and target
  transaction exist.

## Acceptance and verification

- Repository-wide searches leave each hard constant, replacement calculation,
  expiry SQL, and state transition with one documented owner.
- Schema tests prove one current plus one pending profile avatar, one row per exact
  pre-join custody, room ownership after appearance promotion, and rejection of
  impossible state combinations.
- Persistence tests prove pending replacement at the exact count/byte ceiling,
  failed replacement rollback, current-avatar deletion only after apply, exact
  pre-join replacement and transfer, expiry-only cleanup, and no per-user 64/128-MiB
  rejection.
- Existing HTTP, admission, profile, and copied-frontend avatar flows pass without
  fallback. Room appearance and message attachments remain visibly unavailable
  until their own complete slices are connected.
- Resource documentation records the prior query/state cost, the change intent,
  preserved contracts, measured or directly counted CPU/memory/disk/latency effects,
  and residual threats. Claims without evidence are omitted.
- Manual web and Daybreaker reviews explicitly cover security, duplicate policy,
  responsibility separation, unnecessary state/abstraction, deletion safety, and
  overimplementation. Only findings and the final approval outcome are recorded.
