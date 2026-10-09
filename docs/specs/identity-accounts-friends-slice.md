# Identity, accounts, friends and human admission

## Account deletion — owner task (2026-10-09)

Status: final additional design round 4 APPROVE C0/H0/M0/L0 (2026-10-09),
Daybreak Blue xhigh/read-only thread 01a12084-7448-7532-b549-da1cf5ba21fe.
Completed answer read; requirements below are approved design, not shipped/runtime
behavior. Implementation and at most three code-review rounds now proceed.
This task
authorizes scoped commits/pushes on Rust `codex/recovery-and-sonnet` and Worker
`codex/owner-session-renewal`, local/Miniflare deletion tests, central migrations
and deploy after code approval. Never delete the owner's or any existing production
account; a newly created throwaway guest may be used if necessary. Do not modify
`.agents/`, `scripts/__pycache__/` or `wrangler.cleanup-*.toml`.

### Required behavior and entries

Shared app/web `UserSettingsPanel` → `CentralAccountSettings`, including the
roomless startup/server chooser, must offer central account deletion for Google
and central guests. Local guest/local Google unlink, hide, leave and logout retain
their current meaning. Trusted packaged/central UI owns central credentials;
host-served JS cannot perform step-up. Cancellation does not delete anything.

Show the current account and a fixed honest explanation: own server registration
is retired while local data stays; memberships end when each host next connects;
messages, attachments and participant history on other servers may remain;
deletion is irreversible. This explanation is identical for owner/member/guest;
no owned-server inventory decides which disclosures to show. At most one optional
bounded summary read is allowed; failure, saturation, omission or stale data never
blocks proof, confirmation or deletion. No paginated impact read or impact revision
is required or stored. Google requires fresh same-subject OAuth `auth_time` within
300 seconds (missing/stale fails closed). Guest requires the existing recovery code,
without rotation or issuing a new code from the current session. Guest deletion
cannot resist theft of the whole device or recovery code. Explain this in Korean
해요체; no autofill. After step-up confirm the same account with the fixed explanation,
require `탈퇴`, and submit once. Confirmation ends all account-derived relationships;
no server data is deleted. Current account/session/device/proof must match at final write.

Central termination is one O(1) transaction: recheck current person/session/device,
proof/request, disable person, set immutable `deleted_at`, clear mutable
profile PII, consume proof and bind the receipt hash. All devices' central APIs,
recovery, grant issue/redeem and owner publication become invalid at this commit.
No child-count-dependent DELETE/UPDATE is allowed here. A 0-row condition must
abort the entire batch through a constraint, not a later JavaScript exception.

Central disable and host cleanup are distinct. Known custody and legacy unknown
must remain `cleanup_pending` until the host installs its tombstone, terminates
every derived authority, and ACKs. Offline/unsupported hosts may remain pending
indefinitely; never display that all access ended because of timeout or expiry.
Owned registration/public ingress ends, while local rooms/messages/attachments/
server AI/settings remain. Members leave every room, and their companion authority
ends; other people, independent native pairing/local operator and server AI remain.
Do not terminate user-owned external provider processes.

### Admission and terminal authority

No separate finalize call. For owner/member initial admission and reconnect:

1. Host durably prepares an unusable provisional admission bound to exact issuer,
   person hint, server/epoch/channel/device/client key. No credential, Joined,
   entitlement or HTTP/WS/SSE/room access exists before successful redeem.
2. Existing central host-signed redeem is the linearization point. One D1 batch
   rechecks active person/session/device/current owner and exact key/epoch/endpoint,
   records custody (owner_cleanup_targets or member_servers). Member grants keep
   their existing consumption contract. Owner grants keep exact replay: the first
   exact grant/host/client tuple allocates a custody generation; an exact replay
   while all current authority remains valid returns that same generation, never
   consumes the owner grant or allocates another generation. Different tuples fail.
   New grants allocate newer generations; exact replay metadata lives through the
   existing grant replay horizon. Capacity, nonce or write failure rolls back.
3. Host matches verified response and generation to provisional custody, then
   activates in one SQLite transaction rechecking terminal/disconnect barriers.
   Deleted/inactive/error/unknown redeem cannot activate provisional authority.
   Unknown result may discard the provisional row, but preserves the existing exact
   owner-grant retry semantics: a new bounded reservation retries the identical
   tuple and receives the same generation/session while still valid and connected.
   Centrally successful unknown remains custody until exact host cleanup/no-effect
   ACK. After deletion, replay is terminal/inactive and never returns authority.
4. Delete-before-redeem creates no admission/custody. Redeem-before-delete records
   custody, so a delayed activation remains pending until tombstone and exact
   generation ACK. A delayed old ACK cannot clear a newer custody generation.

The host-signed request declares `account_deletion_protocol: "v1"` for supported
redeem/floor/sync. Custody response additions are returned only to that signed
protocol; preserve old host response shapes, including deny_unknown_fields callers.
Central still records known custody for old redeem, but never reports host cleanup
complete without exact supported ACK. Unknown/unsupported remains pending; no
fallback turns it into authority or re-registration eligibility.

Owner custody uses an additive integer `custody_generation` (1..2^53-1), allocated
monotonically per person/server/registration_epoch on the first exact owner redeem.
Existing wire/session `generation` remains endpoint generation, byte-for-byte in
meaning; it is never reused as custody generation. Owner redeem response and exact
replay return both fields. Provisional state initially has no custody generation;
verified redeem fills it before activation and stores it with the owner session.
Terminal sync/tombstone target, cleanup ACK and release outbox carry the exact
issuer/person/server/epoch plus `custody_generation`. Release/ACK CAS compares that
field, never endpoint generation. Member custody uses its existing `projection_id`
as the exact generation identifier through provisional activation, sync, tombstone,
ACK and release; do not add a second member counter. Pre-redeem provisional custody
has no authoritative member projection; fill it only from verified redeem.

Provisional reservation precedes redeem and shares the existing 1,024 challenge
ceiling: <=1,024 rows/4MiB per host, <=4KiB fixed bounded fields per row, grant
fingerprint rather than bearer. Store expires_at and boot custody. Startup first
blocks adoption of previous-boot rows, prunes in <=100-row pages/yields, then opens
transport. Reservation prunes expired/previous-boot rows before row/byte CAS; local
capacity failure never redeems centrally. Transport close removes only that exact
provisional. Reuse existing challenge/grant/opening deadlines (member 300s); narrow
activation to the verified grant expiry. No new timer/periodic task. Terminal
tombstones survive provisional pruning and late boot/disconnect responses.

Every person predating custody-writing cutover has durable `legacy_unknown=1`.
Old writers default new persons to unknown; only the deployed custody-writing floor
can explicitly create a person with 0. Never infer which pre-cutover person had
already purged host custody. This release has no historical completeness proof or
unknown-clearing operation: all pre-cutover persons stay cleanup_pending even after
every retained host ACK. Result and any optional summary separately expose this unknown, with
`예전 서버의 연결이 모두 끝났는지는 확인할 수 없어요.`; do not label it an offline
known server. Time, absent bookmarks and inventory reconciliation cannot clear it.

The minimal durable floor owners are `account_deletion_incarnations`, with one
exact `(owner_person_id, server_id, registration_epoch, host_key_fingerprint)` tuple,
public host key and saved ingress origin, plus `account_deletion_floor` singleton
holding source high rowid, scan cursor, closure revision and pending/closed state.
No person/server/session cascade can remove this inventory. Preservation triggers
on server insert, owner/epoch/key change and physical cleanup copy NEW and/or OLD
exact tuples in the original transaction. Existing bounded cleanup owns backfill,
closure and eventual removal. Remove an inventory tuple only after exact host
floor/no-effect/termination ACK, all custody/replay/provenance references ended,
and >=30 days; a missing host/timeout never authorizes removal. This inventory is
cleanup authentication/routing provenance, never live admission authority.

Legacy owner floor mapping is in each existing signed <=16-item floor-page
exchange, not another request. Verify parent fingerprint/person against the exact
saved/current server/epoch/key/owner inventory tuple. Central allocates once and
replays the group's `floor_custody_generation` in owner_cleanup_targets, advancing
its greatest current generation only at first allocation. Each parent fingerprint
maps to that same group floor generation (multiple legacy parents share the slot).
Host durably attaches mapping to each parent and its aggregate before reopening
transport. Failure/ambiguity keeps transport blocked. Later exact floor replays
return the same mapping and cannot lower a newer current generation. Member floor
retains existing projection_id; floor must never grant admission or active state.

Before bounded backfill, install old-writer-safe transactional preservation guards:
registration insertion, epoch/key change, ownership transfer and physical cleanup
copy each affected exact incarnation into durable floor inventory (old and new
tuples as relevant), independent of registration cascades. Capture a durable scan
cursor/high key at that barrier, page <=100 source rows, and close in a transaction
that proves no remaining snapshot rows and records the current closure revision.
Writes behind the cursor are already preserved by those guards. No unbounded
migration snapshot or unchanged revision across the whole scan is required.
Deletion is exposed only after this bounded snapshot closure. An upgraded host
first blocks old-parent transport/adoption, reports retained live owner/member
parents in <=16 items/8KiB per signed request, and completes exact incarnation floor
CAS after its final page. Floor ACK resolves known targets only, never legacy
unknown. Routine startup queries server-scoped pending work, not all active
bindings. Local scan/prune transactions remain <=100 rows. No polling.

Additive host lifecycle state owns `central_owner_custody` per
issuer/person/server/epoch: greatest verified custody_generation and live dependent
count. Owner activation/replay, disconnect/revocation and child completion update
this aggregate inside their existing SQLite authority transaction. Exact replay
of an existing parent does not increment count; older replies never lower greatest
generation. Count cannot reach zero while any account-derived authority or relevant
provisional/adoption still exists. Only zero-after-last-dependent writes release
for the greatest generation, never the departing session's older generation.
Known redeemed/no-effect activation also records exact release when no dependent
exists. Restart disconnect-all and release creation are the same transaction,
before transport reopens. Pending unknown redeem stays fenced until sync resolves
it; it is not inferred successful or released by a timeout.

Normal last-custody disconnect records an exact-generation local release outbox in
the same transaction. Piggyback release on the next existing redeem/sync/startup;
standalone sync is used only for known pending deletion or permanent-retirement
drain of an already durable exact release/ACK outbox. CAS exact generation,
retain last acknowledged generation/time and a compact deletion-ACK tombstone for
30 days and the existing replay horizon, whichever is later. Lost ACK responses
get stable ACK. Bounded cleanup owns physical target purge; inactive custody slots
can be reused with a new generation without erasing prior ACK provenance.

Local additive lifecycle/tombstone storage must not modify frozen binding DDL.
Tombstone precedes cleanup paged by actual dependent rows and loaded effects
(<=100 rows/128KiB per SQLite transaction), not room count. Existing lifecycle state
owns stable per-table cursor/stage; session, each invite type, companion/approval,
owner-derived authority and participant event stages drain separately and yield
between pages in the same existing owner, with no new task/cadence. Reuse removal
semantics but never call its current unbounded bulk/fetch_all form for deletion.
Cursor, row revocation and idempotent participant event commit together; crash
resumes the stage. ACK only after every stage and relevant authority/adoption is
exhausted. Global tombstone blocks all
human/owner/attendee/child HTTP/WS/SSE and publication transaction paths. Late
binding/provisional activation cannot bypass it. Reuse participant removal and
revocation owners, including idle sockets, directory streams and secure channels.
Keep the existing durable late-disconnect barrier. ACK only after durable cleanup.
Startup/recovery checks old bindings/owner parents before new transport; authoritative
terminal/absent installs tombstone, while timeout/5xx/malformed response stays unknown.

Bookmark creation and alias upsert must use the single effective-terminal server
predicate, including current owner active/deleted state. Old-writer INSERT/UPDATE
of person_servers is guarded at D1 as well; no new child can join a terminal server
while cleanup drains it. Bookmark deletion remains allowed. The same predicate
covers discovery/preview/host auth, publication/name/icon, register/claim, grant
issue/redeem and results; no endpoint-local alternate authority.

Central late member-result CAS requires live exact registration/key/epoch **and**
member `persons.status='active' AND deleted_at IS NULL`. Deleted members receive a
stable terminal ACK without changing state, visibility, revision or capacity refs.

### Step-up, receipt, registration and retention

Proof/final termination has a non-borrowable lane: ordinary IP/actor/GENERAL traffic
cannot exhaust a valid fresh proof's pre-reserved O(1) disable. Failure attempts use
separate IP/session limits. Valid final device signature + proof bypasses ordinary
nonce/admission budgets; atomic request-bound proof consumption owns replay.
No ordinary mutation inherits this exception. Final write rechecks authority.

Keep `DELETE /v1/account`; old confirmation-only calls fail reauth-required.
Add the signed proof entry, opaque receipt status, and exact host-signed
account-deletions sync/ACK. One random 256-bit receipt capability, hash stored,
request/account binding, deletion-status-only, rate limited, no-store, expires in
24h. No retained device key, signed receipt-read ceremony or automatic polling.
Lost response stays unknown until explicit receipt lookup. No automatic account
creation to confirm deletion. Identity verification first returns active/deleted/
absent without issuing a person/session. Explicit new registration alone creates
a fresh person/device; Google deleted-identity transfer uses atomic terminal CAS.
Guest after verifier purge cannot confirm old deletion and must see unknown.
New registration never inherits old custody, lists, bindings or hosting incarnation.

Central mutable PII clears at disable; sessions/keys/recovery/external identities
are bounded child cleanup. Minimum terminal provenance remains through pending
ACK; 30 days is a minimum, not a promised maximum purge delay. On host, clear
mutable profile snapshots when authority, accepted-operation and replay references
end. Replace raw central issuer/person with a domain-separated keyed tombstone
when continued denial is needed; retain an unbound guard rather than converting
the user into a recoverable local guest. Unresolved provenance is minimal and
explicit. Disclose that messages, attachments, participant history/audit records,
past profile events and server-owned backups may remain.

### Cleanup, deployment and acceptance

First additive migration installs BEFORE DELETE ON persons: only terminal +
purge_ready + every child absent can physically delete, including active legacy
persons without an account_deletions row. Test the literal old Worker DELETE.
Incomplete guest provisioning explicitly removes its exact known children before
terminal/purge_ready parent purge. Inactive writer and active-restore guards are
mandatory. Parent session/device/server cascades cannot evade bounded cleanup.

`persons.purge_ready INTEGER NOT NULL DEFAULT 0 CHECK(purge_ready IN (0,1))`
is durable readiness, owned only by existing bounded cleanup. In one budget-reserved
D1 batch: CAS terminal person to ready after >=30 days, `legacy_unknown=0`, all
exact host ACK/replay/provenance dependencies complete, and every non-ledger child
absent; remove its matching account_deletions ledger only for that ready person;
then delete that ready parent under the BEFORE DELETE guard, which rechecks every
child including ledger absence. Any statement failure rolls back the whole batch;
0-row CAS removes neither ledger nor parent. Unknown/pending never becomes ready.
Cleanup bounds rows/statements/indexed writes under its existing limits. No separate
readiness job; unissued guest cleanup may mark only its exact failed provisional
person ready after explicitly removing its known children, never an issued account.

Before the blocking migration, deploy a schema-independent guest-provisioning
floor Worker to all traffic: person/device/recovery/session creation is one D1
batch and a failed session issuance rolls everything back, without deleting an
active parent. Prove zero partial rows and same-device retry on local D1 before
rollout. Test this separately from rejection of literal old raw account DELETE.
Any still-needed internal cleanup explicitly removes only the exact unissued
person's known children before terminal/purge_ready parent deletion.

Account-deletion demotion differs from permanent duplicate retirement. Keep the
old person and epoch terminal and shut only that ingress; explicit local-admin
registration with a fresh person and fresh central epoch may open a new connection
only after exact old-host tombstone/ACK and the central epochless-child cleanup
barrier. Bounded cleanup removes all old server_endpoints, person_servers aliases/
bookmarks, server_icons, old nonce/grant and other presentation/relationship children,
and resets row-owned label/icon/presentation fields before epoch transition. Signed
registration CAS proves this barrier in the same transaction; until then return
cleanup-pending, without exposing old data or inventing an epoch-key migration.
Do not automatically clear restrictions
on login; duplicate-retired installations retain their permanent restriction.
Old grants/membership/custody cannot bind to the new epoch.

Persist `account_deleted_pending` as the existing sender's termination-only mode.
Closing public ingress cannot return from that sender or disable its terminal-only
signed transport. Endpoint/name/live member publication stays off; durable cleanup,
release and ACK continues on actual work wakes and restart. After stable final ACK
the sender may quiesce; restart with pending outbox resumes without a second task
or periodic poll. Permanent duplicate retirement keeps ingress, live publication and re-registration
disabled forever. The same terminal-only sender drains already durable exact
release/ACK work before exit, including restart with pending outbox. No second task,
periodic poll or restoration of live authority is allowed.

Reuse daily cleanup and event sender; no polling/heartbeat/new periodic schedule.
Stay within existing 10,000 indexed writes/day and 49 statements/invocation,
pages <=100, existing day/crash reservation semantics. Measure new index/trigger
costs; do not freeze speculative 1,000/4,370 pool repartition. Pending delivery
has bounded finite retries (1/3/7 days) and explicit startup/ingress/online/retry
wakes. Empty work sends no requests. Public hint is a coalesced boolean wake,
never freshness or authority. At most one reserved sync per server/cooldown uses a
non-borrowable cleanup lane; repeated hints cannot advance timers or consume more
reservations. No signing-key/wake-token ceremony. Exact saved public ingress,
no redirect/private-network fetch; retired
key authenticates sync/ACK only, never live host authority. Provenance survives ACK.

Acceptance observes signed HTTP + real D1 atomic rollback, quota saturation,
both orderings of redeem/delete/activation and late ACK/result, local SQLite
upgrade from 86, all devices/all rooms/idle sockets/AI parents, offline restart,
guest/Google wrong or stale proof, receipt loss and explicit new registration,
data preservation and idle zero central calls. Include blocked/absent optional summary and custody/ownership churn during
proof/DELETE, lost owner redeem response/exact same session replay, mixed old-writer
floor preservation/closure, disappeared pre-cutover custody, crash after demotion
before ACK, epochless-child re-registration denial, and 16/17 floor-item wire bounds. Also exercise legacy owner floor mapping/replay,
G1/G2 concurrent owners disconnecting in both orders and restart, a single room
with many invite/session children with <=100-row/128KiB page evidence, and new
bookmark/alias writes (including literal old SQL) rejected on effective terminal.
Shared trusted web and isolated
packaged app must exercise roomless/owner/member/guest confirmation, cancel,
pending/complete/error, and cache cleanup. Build/unit results cannot replace UI
evidence. Real Google step-up remains unknown until actually verified on both
clients, and missing auth_time never falls back to session-only deletion.

Design requires Daybreak Blue xhigh APPROVE within four additional rounds;
code requires APPROVE within three rounds.
If unavailable or rounds exhausted, stop and record why in VERIFICATION.md. Commit
under 1,000 changed lines with the requested coauthor trailer; mandatory gates
remain unchanged. After code approval push both branches, record production
version/migration state, migrate first (one retry), build isolated central assets
at Rust HEAD, deploy with existing cleanup-on config, and smoke / 200,
/v1/bootstrap 401 and /member-join 200. Durable evidence belongs at the top of
VERIFICATION.md. Do not claim unverified product-flow completion.

### Code-review acceptance correction (2026-10-07)

Duplicate-owner bootstrap and registration `server_exists` projections must show
event endpoints as published only for online state and the current registration
epoch. Lease expiry applies only to legacy endpoints. Preserve offline/stale-epoch
denial so the irreversible keeper-selection flow cannot mislabel the active host.

Shared remote directory/session surfaces, dock entries and companion invitation
custody use the host-signed channel origin, never the central/Tauri document origin.
Returning to the server list and admitting another host in the same document must
retain each server's exact authority boundary without mistaking the common shell
for the host. Closed-channel bearer tombstones retain their origin but cannot send.
Central owners selecting server-computer AI creation or stopped-session settings
must list/import persona cards and load private thumbnails through the admitted
owner channel. Local operators retain their existing private API; members,
plaintext requests and another channel cannot use owner persona authority.
Verify unequal shell/host origins, A-to-B switching, invite creation/copy, and
owner success plus member/plaintext/cross-channel denial before re-review.

## Secure admission and event-only publication (owner task, 2026-10-07)

### Packaged admission corrections (2026-10-09)

The two signed desktop entries (UI Check owner host and UI Check 2, same owner)
must connect on the first attempt, retain committed custody through directory/SSE
and room socket setup, and load/edit profiles and host avatars through that exact
encrypted channel. WebSocket control frames must not become application records;
application schema/AEAD/order checks, admission deadlines and durable late-disconnect
barriers remain unchanged. Verify concurrent initial directory/SSE/room requests,
cancellation and plaintext/cross-channel denial. The original intermittent first
failure must be distinguished from a controlled reproduction if no trace identifies it.
The rail derives an admitted server's display from live workspace custody, not the
central list's disconnected default; pending directory authority still blocks room
actions. Profile failures use plain Korean 해요체. Local AI setup retains the full
host/port as small muted secondary text, including inside the AI configuration dialog.
Commit only, no push; .agents/ and scripts/__pycache__/ remain untouched. Packaged
testing closes external access, quits only its apps/children and leaves no owned tunnel.

Status: Daybreak round 3 APPROVE (C0/H0/M0/L0, 2026-10-07); implementation and
runtime acceptance pending. Required entries:
local start/register/claim/retirement; owner startup chooser/rail → remote directory,
create/open room; member rail reconnect/selection; invite → login/consent/join;
shared app/web API, attachments, room WS and owner-directory SSE; exact retries,
revocation, ingress replacement/stop. Existing host room transactions, Joined and
entitlement remain sole authority; connected lifetime remains host-owned. Central
atomic redeem rechecks current logout/device/account/ownership/registration/key/
endpoint at every new admission. This task authorizes scoped Worker/transport,
commit/push/deploy/signing/E2E; .agents/, scripts/__pycache__ and cleanup configs stay.

Trusted packaged or central ASSETS UI owns remote workspace and all private keys/
credentials throughout admission/API/streams. Host-origin JavaScript and fragments
never receive central grant, browser credential, bearer, private key or child ticket.
Event central invitation creation opens trusted central /member-join from the outset
with versioned query containing protocol, server_id, registration_epoch and aaj1;
existing aaj1 invitation capability is initial trusted-entry query input, immediately
removed from history, retained in memory and sent only through the verified encrypted
host channel. No invite/input URL logging or asset-query forwarding. Existing opaque
invite credentials and host invite row/scope/expiry/consumption remain unchanged.
Web Google login runs in a trusted popup while the opener retains invitation in
memory. Correlate exact origin/popup/nonce/state; completion message contains no
credentials. Opener reads existing central session after correlated completion and
rechecks account. Invitation is never stored in web storage, OAuth state/callback or
popup; blocked/closed/expired popup or opener reload requires explicit original-link
retry. Popup expires at min(handoff expiry,300s); cancel/unmount ends its owned wait.
No navigation/storage fallback or central polling. Legacy host-origin links and
existing non-central guests stay at their existing owner; no upgrade from that origin to event central admission. The invitation contract
owner records this affected creation/link/challenge transition before implementation.

Reuse server-info/challenge identity owner and persisted Ed25519 host key. Keep
legacy POST challenge stateless. Secure handshake runs in /api/secure-channel:
ClientHello → signed ServerHello → AEAD confirmation, with existing public connection
permit and 10-second deadline; no unauthenticated pending map. Signature canonical
JSON array binds domain AA-SECURE-ADMISSION-1, secure_admission_v1, server ID, epoch,
exact origin/generation, purpose, fresh 32-byte nonce, client P-256 public key, host
P-256 ephemeral key, random channel ID and timestamp. Client pins fresh central
registration/key/endpoint, validates every tuple field, ±60s and Ed25519 signature
before sending secrets. WebCrypto and maintained Rust ring provide P-256 ECDH,
HKDF-SHA256 and AES-256-GCM. HKDF salt=client nonce/context=transcript, separate c2h/
h2c direction keys; 96-bit AEAD nonce=zero prefix + big-endian 64-bit counter.
AAD binds protocol/channel/direction/counter; serialized strict contiguous frames,
no wrap/reuse. Invalid authentication/order/schema/size terminates without fallback.

One secure outer WS transports admission/HTTP/responses/resources/SSE/virtual room
WS. The existing route registry declares secure_remote eligibility once, excluding
private/static/registration/login/recursive channel/upgrade routes; no parallel path
allowlist or fabricated loopback authority. Existing HTTP connection admission,
public ingress provenance, capacity/body limits and handler authorization remain.
Verified SecureClient context adds key/pinned tuple; original public router and room
socket/ticket owners execute operations. User login/preview/consent precedes channel creation. After 10s handshake, first
successful admission has a 30s deadline; before admission dispatch only its encrypted
purpose-specific challenge/admission, never other product/guest/pairing APIs.
At most 32 virtual requests/channel; each additionally acquires existing process-wide
128 total/127 public HTTP permits before buffering/dispatch, held through response/
SSE or accepted-mutation terminal completion. Room socket retains 128 global/8 principal/
64 room limits; ceilings unchanged. Immediate encrypted capacity failure on exhaustion.
Encrypted receive ACKs bind the host record sequence; only one host record is in flight until the trusted consumer accepts it, providing bounded stream backpressure.
Transport chunks <=64KiB; product WS frames <=256KiB. Cipher queues <=512KiB/direction/
channel and <=8MiB server aggregate, charged before enqueue. Trusted workspace resource/
Blob URL custody <=64MiB with original 10MiB attachment cap; release on resource unmount/
download completion and revoke all URLs on workspace close/replacement. Backpressure/
visible capacity failure, no unbounded buffers or in-use eviction. Central/Tauri connect-src permits https:
and wss:, with restricted scripts/navigation/images. Frontend authority selection
injects one RemoteTransport into shared flows; no global fetch override, feature
transport copies or local operator authority inherited by a remote workspace.
The legacy host-served frontend build notice remains on host pages only: trusted
entries execute their own bundle, validate the host protocol/product surface on
admission, and never poll central or compare their bundle with host static assets.

A fresh non-extractable client ECDH key is approved by central device-signed grant
issue. AEAD fresh transcript confirmation proves client possession to host before
central redeem/local session commit. Strict signed issue/redeem fields and response
echo bind protocol, epoch, origin/generation, purpose, client key, server-generated channel_id and existing member
challenge. Add grant protocol default legacy/client key/channel_id columns; reuse tuple owners.
Atomic SQL joins current session/device/person/registration/registered host key and
endpoint mode/epoch/tuple, including owner redeem's key recheck. Keep existing caps.
Member host transaction remains final Joined/entitlement owner.

Preserve durable aad1 browser/device identity; persist secure_client_key_fingerprint
and secure_channel_id as additional owner/member session bindings. Exact retry and secure custody use
require both identities, exact channel_id and current tuple. Secure child tickets require the same key
and channel; plaintext routes cannot use secure grants/sessions/tickets. Exactly one outer channel owns committed custody. Same key/grant cannot attach to
another channel; signed issue/D1/response echo/redeem/local result bind channel_id.
Outer cancellation ends unaccepted body/read, response forwarding/SSE/socket waits;
already accepted/dequeued mutations finish their original durable result/events,
tracked with permits to terminal. Do not abort authoritative mutations. Client shows
uncertain failure; exact durable retry is only inside same live channel. Close/adoption
is serialized; admission finishing after close cannot adopt authority and terminal
cleanup durably disconnects any late row before releasing tracked work. Only virtual reconnect
inside a live channel is central-free. Outer close/restart/ingress replacement/revoke
ends authority; new channel requires fresh central issue/redeem. Initial incomplete
admission may retry exact same grant/key/browser/channel and virtual admission identity
inside that live channel within original 300s window, recovering only
its matching live committed result, never a disconnected/revoked session. Reload
requires fresh admission; no credentials/private keys persisted outside trusted owner.

Add endpoint mode default legacy_lease and nullable registration_epoch. Event row
lease_expires_at=0; additive DB constraints/triggers reject nonzero event lease,
missing event epoch and event→legacy transition even for older Worker writes. Old
Worker legacy live-endpoint SQL therefore denies event rows while mixed serving;
old host writes cannot mint a positive event lease. No new lease/TTL. Legacy
rows retain lease gates, formats and expiry. Event_secure_v1 accepts only secure
admission; legacy writes cannot downgrade event rows. Old clients get no usable
origin and offline/unsupported; secure clients require explicit protocol echoes and
never fall back. Bootstrap capability preference is representation only; divergent
preview/issue/redeem/publication version fields are signed. Migrations expand first;
secure Worker/shared UI/host and actual encrypted-flow verification precede event
activation; roll-forward only under Rule 10 pre-release exception.

Event generation is reserved durably at actual start/origin-change/graceful-stop,
fixed across retries and above all prior reservations at replacement start.
Local ACK/park compares exact epoch/generation/state/origin/mode against current pending;
superseded responses cannot clear/park/increment failure of newer event. Reread latest
pending; new actual event resets retries, delivery retry never reserves generation. One
endpoint row stores epoch/mode/state/origin/generation. Event reads/issue/redeem
require row epoch=current server epoch; new current epoch replaces old row, old epoch
cannot write. Signed mutation atomically rechecks live registration/key/epoch and
CAS: higher applies, identical ACKs, same generation different payload conflicts,
lower is stale. Offline retains empty origin and high-watermark while registration
active; no TTL/history/client offline publication. Grant expiry for event endpoints
is min(now+300, central session expiry); direct client host liveness creates no
central writes. No polling, heartbeat or unknown-protocol fallback.

Replace five-minute renew/two-second scan with ingress/registration/profile/member
outbox notifications. Endpoint delivery retries same fixed event at 0/2/8s then parks,
visible failure until new actual event/explicit retry. Graceful stop reserves one
offline event and attempts one immediate final send under existing 8s HTTP timeout,
no retry sleeps; exact pending compare-and-clear, report failure then terminate.
Unchanged same-origin readiness fluctuations do not publish central health. Existing
name/member durable retry policy remains, scheduler waits only actual pending work's
due time/events. No /v2 family, GET-preview, quota/cleanup, online TTL/history redesign.
Independent transport and scheduler owners keep existing file/structure ceilings.

Acceptance: stolen grant/other key cannot commit; wrong-key bearer/child tickets,
tampered transcript/AEAD replay/order and authority races fail; exact member retry/
leave/kick stays correct; old clients denied event and legacy expiry preserved;
late offline/idempotency/conflict/epoch retention; controlled time shows zero idle
renew/publisher polling and fixed retry generations. Test actual WebCrypto↔ring
interoperability and bounded cancellation/resource cost. Run affected tests/mandatory
gates, <1000-line independent commits and full-range Daybreak approval. Record deploy
version/migrations/exact HEAD assets; signed two-app owner room E2E/member if feasible.
UI states use 해요체: 확인 중이에요 / 서버를 확인하지 못했어요 / 연결이 끝났어요 /
잠시 후 다시 시도해 주세요. Protocol/key/epoch/channel jargon stays off screen.
Unknown runtime/Windows/member outcomes stay separate from test/review evidence.

## Event-driven central directory (owner decision, 2026-10-07)

Signed GET `/v1/bootstrap` through `useCentralDirectory` must never poll or retry
on a timer. Refresh only at app start/mount, window focus or visibility becoming
visible, browser online, explicit refresh, and the existing immediate checks before
rail admission/connect and after mutations. Focus/visible events skip a check when
the current session's last successful check was less than five minutes ago; online
and explicit/admission/mutation checks bypass that throttle. A failed check waits
for the next eligible event/action, with no scheduled recovery loop.

Affected entries: desktop startup/rail (`StartupIdentityGate`, `useAppController`),
shared directory display/refresh, and remote-owner dialog loading. Preserve one
in-flight request per session, account isolation, cancellation, signed 401 clearing,
display-only outage cache, immediate admission authority and host-owned sessions.
This supersedes the 0309ebc2 30-minute observation/backoff below and the original
step-1 polling contract. No Worker/schema/provider/transport change is authorized.
Acceptance: fake-clock regressions prove no idle or failure-driven calls, exact
five-minute focus/visible throttle, immediate online/explicit recovery, coalescing,
account isolation, 401 termination and unmount cleanup; affected tests, frontend
build and mandatory architecture/source-growth gates pass. Commit without pushing.

## Central GENERAL exhaustion correction (2026-10-07)

Required behavior: idle desktop hosts, another owner computer, and remote room
pages with the agent-add dialog must not exhaust central capacity by keeping a
directory observer open. Affected entry points are `useCentralDirectory`, desktop
startup/rail, remote-owner dialog directory loading, and explicit rail open/refresh.
Keep signed authentication, fresh bootstrap before admission, one in-flight request,
account isolation, cancellation, visible outage/authentication states, automatic
recovery, and host-owned connected sessions. The original client mitigation did
not authorize Worker/schema changes; the manager subsequently approved the bounded
Worker correction described below. Deployment and remote migration remain excluded.

Approved Worker follow-up: exact signed GET `/v1/bootstrap` and authorized icon
GETs retain signature, clock-window, live authority, authorization and edge rate
limits, but may repeat within the clock window without nonce persistence/debt.
Other methods/routes retain replay protection. Additive migration
`0016_general_actor_budget.sql` atomically caps GENERAL nonce debt at 90 units per
account, 45 per session and 45 per host per UTC day, derived from existing verified
session/server ownership. The global 700 pool and cleanup bounds stay unchanged.
The shared central response owner maps `actor_quota_exhausted` (429) to Korean,
identifying the daily actor allowance and UTC-midnight retry; existing temporary
error classification and retained-directory/authentication behavior stay intact.
Apply migration before Worker code; retain schema on code rollback. This local
slice does not deploy, change polling again or apply production migrations.

The shared observer checks a healthy directory every 30 minutes instead of every
30 seconds. Transient retries use 1/2/4/8/... seconds up to 30 minutes; successful
checks reset backoff. Existing online events and explicit user refresh wake the
same observer immediately. Directory changes/outages can take up to 30 minutes to
appear when idle; host room streams and explicit admission checks remain immediate.
This supersedes the step-1 30-second observation/backoff contract below. One idle
client now schedules 48 rather than 2,880 checks/day (144 rather than 8,640 GENERAL
nonce units at three units/check), plus initial/event/action checks. This is only a
client mitigation; once the approved Worker correction is deployed, exact
bootstrap reads spend zero GENERAL nonce units. The global 700-unit pool remains.
Existing host endpoint renewal remains five minutes (ENDPOINT),
default-name publication remains revision/event-driven (GENERAL; failed sends
back off 2/4/8/16/32 seconds), and durable member-sync retry remains
1/2/4/8/... minutes up to six hours with 80-120% jitter, at most 48 sends/day
(successful acknowledgements impose a one-minute gap).

Acceptance: demonstrate pre-fix failure through bootstrap boundary responses under
controlled time; verify bounded idle checks, delayed automatic outage recovery,
fresh explicit refresh, 401 termination, stale-account rejection, unmount
cancellation, and remote-owner dialog naming. Run affected frontend tests/build
and mandatory architecture/source-growth gates; commit only scoped client changes,
with no push. Production recovery and packaged acceptance remain unverified.

## 시작 실패 안내 보정 (2026-10-07)

공용 desktop/web StartupIdentityGate와 StartupIdentityBoundary는 한국어 해요체 안내를
기본 표시하고, 원본 오류·절대 로컬 경로는 닫힌 자세히 안에만 표시한다. 기존 재시도와
실패 전이를 유지하며 새로운 권한·복구 동작은 만들지 않는다. 연결된 방은 표시용 중앙
목록 조회의 로그인 실패로 시작 화면으로 돌아가지 않는다. 영향받는 Vitest와 타입 검사로
검증하고 실제 패키지 검증 여부는 별도로 기록한다.

## 계정당 서버 하나 — 호스트·공용 프론트 (2026-10-06)

확정 one-server-design v2 → v3 → v4 → v5 → v5.1 순으로 초안을 덮어쓴다.
등록 진입점은 공용 registerLocalServer/StartupIdentityGate와 Rust registration-proof,
관찰 진입점은 central directory의 모든 host-signed 요청이다. 409 server_exists는
현재 계정 UI에서 등록을 중단하고 기존 owner connect/pairing의 기기 연결로 안내한다.
다른 계정 소유 호스트의 전역 영구 제한은 쓰지 않는다. B 호스트 → A claim 409 → B 복귀도 호스팅을 유지한다.
410 server_retired와 저장된 epoch가 있는 registration_absent는 구분된 Rust 오류이며,
단일 멱등 강등 전이가 강등 상태를 저장하고 managed/manual 모두 readiness·새 공개
권한·디렉터리 발행을 끊는다. managed 소유 프로세스도 종료하되 admitted 세션은
취소하지 않는다. 다른 4xx의 기존 실패/재시도 의미를 보존한다. 재시작도 강등을 유지한다.
공용 시작 화면은 자기 서버를 바로 열고 다른 컴퓨터에는 계정 서버 이름과 ‘이 기기로
연결’을 보인다. 중복은 이름·연결 상태·마지막 접속 목록에서 keeper 선택 후 별도
확인하며, 중앙의 account-scoped keeper epoch/revision과 정확한 loser incarnation으로
은퇴한다. 은퇴의 비가역성과 기기로만 연결 가능함을 안내한다. 단독 서버 삭제의
409 server_move_unsupported는 ‘서버 옮기기는 아직 지원하지 않아요’로 표시한다.
강등 표시는 기존 방 목록 스트림의 hosting_restriction으로 로컬 호스트에만 전달한다.
강등 영속 쓰기 실패는 대기 이유를 보존하고 제한 빠른 경로보다 먼저 재시도한다.
한 강등 작업에서 100ms 간격 최대 3회 쓰기 후에도 실패하면 네트워크 서빙을 종료한다.
로컬 loser의 성공한 (server_id, registration_epoch)는 로컬 확인까지 보존하며 중앙 은퇴를
반복하지 않고 로컬 멱등 전이만 재시도한다. 성공 결과는 비밀 없는 로컬 대기 기록으로
남기며 서버 목록 화면이 1초 간격으로 재시도하고 확인 시 삭제한다. 언마운트 시 타이머를
정리하되 기록은 유지하고 다음 로컬 상태 조회/화면 진입에서 재개한다. 실패는 화면에 보인다.
로컬 요청은 저장 epoch와 원자적으로 비교하고 은퇴 커밋 뒤 늦은 epoch 변경도 거부한다.
기존 2초 directory 관찰/30초 중앙 갱신 주기를 유지한다.
runtime_metadata의 한 상태를 추가하며 스키마 상승·방/세션 삭제는 없다.
중앙 Worker는 별도 작업 소유이며 읽기만 한다. identity-directory/README.md의
v5.1을 정본으로 확인했다: 중복은 owner 항목이 없는 servers[]와 owner_server_conflict,
resolve-duplicates의 keeper_server_id/keeper_registration_epoch/expected_revision 및
loser server_id/registration_epoch를 사용한다. 로컬 loser의 은퇴 성공도 같은 강등 전이로 연결한다.
중앙 테스트 대역으로 각 상태 화면, 409/410, 두 ingress 모드·멱등 강등을 검증하고
바뀐 범위 테스트 및 푸시 직전 make verify 한 번을 실행한다. 기능별 <1,000줄 커밋·푸시,
VERIFICATION.md 건별 한 줄. 서명 빌드·수동 검증·배포, 서버 교체·재승격,
동반 AI 네이티브 handoff(2단계)는 범위 밖이다. .agents/와 scripts/__pycache__/는 건드리지 않는다.

## 화면 보고서 후속 수정 (2026-10-06)

공용 app/web의 MemberJoinPanel, StartupIdentityGate → CentralServerList(첫 서버와
기존 서버 편집), LobbyComposer가 대상이다. 참가 확인/이름 미리보기 문구는
`{서버 이름}에서 열린 방`으로 통일한다. 미리보기 방 이름은 기존 방 목록에서 해당
서버의 첫 방 이름, 없으면 `새 회의실`이며 표시 전용이다. 줄은 keep-all로 보존한다.
서버 행 제목은 이름만, 보조 줄은 연결 상태 · OS(로컬 상태는 `이 기기`)다. 편집할 때만
입력 → 미리보기 → 강조 저장/보조 취소를 보이고 바깥 편집/열기를 숨긴다. 기본 이름이
아닌 경우만 작은 기본 복원 링크를 보인다. 비편집 상태는 연필 아이콘과 열기를 쓴다.
900px 이하 채팅 입력의 첨부/앱/멘션/이모지는 기존 메뉴 컴포넌트의 + 메뉴로 접고
보내기는 유지한다. 넓은 폭, 각 동작/비활성 권한, 초안/실패/재시도는 보존한다.
저장/권한/프로토콜/동시성 소유자를 변경하지 않으며 새 요청·타이머·저장소를 만들지 않는다.
변경 화면 회귀와 ui-report/reproduce.md의 모의 경계+Chromium 프로덕션 렌더링으로
1280×800/800×900 after만 v2- 캡처한다. 기능별 커밋, 푸시 직전 make verify 한 번,
VERIFICATION.md 건별 한 줄. 서명 빌드/배포 및 실제 제공자 호출은 범위 밖이다.

## Signed-app/browser feedback corrections (2026-10-05)

Shared app/web entry points: room composer/member panel, invite list/public-access
controls, startup chooser and server rail. At 768–1100px with sidebar, members and
companion card present, the chat textarea must retain positive usable width; reuse
the compact overlay rules and preserve search stacking. The side-chat panel shares
the same width owner and must preserve the composer too. Public-access startup
failures show “외부 접속을 열지 못했어요. 잠시 후 다시 시도해 주세요.”; raw errors
remain diagnostic-only. Invite expiry uses local calendar/time labels (today,
tomorrow or a short date), never raw ISO. Saved-session startup fetch/body timeouts
retain display-only server cache and existing background retries, while explicit
cancellation, logout, 401, malformed data and storage failures retain their separate
semantics. Existing local authority alone permits this-device access. All user-facing
중앙 wording becomes login/account/server wording; server lists/rails must not show
UUIDs as names or metadata. Identifiers, request identity and logs stay unchanged.
No new storage, protocol, abstraction, authority or provider behavior. Acceptance:
affected automated regressions, one pre-push make verify; no signing, manual
verification or deployment.

Rail feedback correction (2026-10-05): shared app/web `openRailServer` keeps a
missing-server/open failure visible across successful background directory updates
until the next server-opening action clears it. The shared `RoomSyncNotice`
connection banner occupies its own layout row above the room shell, including
wrapped messages, so the room-name header remains unobscured. Keep existing refresh, authority,
concurrency guard and retry behavior; no protocol/storage changes. Verify the
missing-server race, later refresh and next-open clearing with a frontend regression;
one pre-push make verify, no packaged/manual verification or deployment.

## Path-only folder grouping (2026-10-05)

Move the six server `central_*` source files into `central/`, then the 17
frontend `lib/central*` sources/tests into `lib/central/`, dropping file prefixes.
Preserve central login, registration, directory, owner HTTP/stream, CLI and
app/web identity entry points, authority, state and retry/failure semantics.
Only paths/imports change; no compatibility module aliases or behavior changes.
Acceptance uses server build/tests, frontend typecheck/tests and one final
`make verify`; deployment, signing and manual verification are excluded.

## Discord-style rail, step 1 — central outage continuity (2026-10-05)

User-required entry points: `StartupIdentityGate`, explicit this-device selection,
`CentralServerList`, `RoomRail`, `DisconnectedRoomView`, and the app's live room
directory. A previously logged-in desktop can open its existing local server when
central bootstrap fails with a network failure, HTTP 429 or 5xx. Use the last
account presentation only; local bootstrap/tickets/directory lineage remain the
sole operator authority. No session, explicit logout, expired/revoked session or
401 requires login; malformed responses, storage/key failures and other 4xx are
not transient network failures. A remote account deletion can only be learned
when central responds; a confirmed rejection clears retained presentation.

Persist the last successful central server directory per account, allowlisting
ID, name, icon reference, OS and relationship only. Do not persist endpoint,
bearer, connect grant, recovery code or host proof in this display cache. Account
switch/logout clears it. Cached entries never authorize open/join: reconnect must
obtain a fresh central directory and the existing bound connect grant/host checks.

The central observation owner permits one request at a time, ignores stale account
results, retries transient failures at 1/2/4/8/16/30 seconds (30-second cap), and
checks a healthy directory every 30 seconds to observe outages/list changes.
Online/manual retry wakes that owner; unmount cancels its timer/request. Successful
recovery automatically refreshes the visible directory. Authentication rejection
stops retries and returns the desktop startup boundary to login.

Connection presentation is per server: connected, connecting, disconnected or
central-unconfirmed. Local status comes from the existing host directory stream,
not central health; a stopped runtime dims local rooms too. Local directory stream
reconnect continues until unmount with 0.5/1/2/4/8/16/30-second capped delays,
10-second header deadlines and the existing 45-second stream silence deadline.
Explicit authority rejection stops that attempt. Web owner retry/termination stays unchanged. Saved remote rooms
remain visible and cannot execute room operations while disconnected. Selecting
one shows the shared disconnected view, a specific reason and retry action.
Central-dependent cached servers appear disconnected/central-unconfirmed, never
online merely because cached metadata or an old lease exists.

Use shared app/web components with subdued server/room icons, a small status dot
or badge, and one thin top outage banner. Do not add an outage modal. Preserve the
web owner workspace's existing terminal modal behavior. No member admission,
provider, meeting or research behavior is added; no deployment, signing, manual
verification or edits to `.agents/` and `scripts/__pycache__/` in this task.

Acceptance: demonstrate failing pre-fix regressions for transient startup/local
open, authentication rejection, display-cache isolation/account switch, automatic
recovery, local-runtime disconnection and disconnected-room retry. Run affected
frontend tests during work and `make verify` once immediately before scoped
commits/push. Packaged visual and manual acceptance remain explicitly unverified.

Daybreak M3 correction contract (2026-10-05): delete the unscoped legacy
`agentsassemble.centralServers.v1` directory (including endpoint origins and host
keys) before network work, including offline upgrade and every account transition;
never migrate it. At the shared signed-fetch boundary, any HTTP 401 clears the
session and display cache only while the rejected token is still current. Notify
`useCentralDirectory` immediately so bootstrap, icon reads/writes, rename and
registration all require authentication without waiting for polling. A late 401
for an old token must preserve the new session/cache/directory. A successful
bootstrap must not publish if the current token differs, including a null session.
Seed legacy storage and control response completion to demonstrate pre-fix failures.
This correction is one commit and push; frontend tests only during implementation,
then one `make verify` immediately before commit/push. Manual verification,
deployment and signing remain excluded.


Status: Phase 5 locally verified and approved by Daybreak through `1e24adf`, C0/H0/M0/L0.

## 초대 멤버를 중앙 계정에 묶기 — C1 승인 계약 (2026-10-05)

### 최소판 H1 (2026-10-05, 승인 설계 v2/v2.1)

H1은 Rust 서버 API까지만 구현한다. 기존 invite preflight, 방 큐, human admission,
profile 생성, 세션 bearer, AA-HOST-1 중앙 클라이언트를 재사용한다. 중앙 W1 redeem은
신원만 증명하고 호스트 트랜잭션은 invite/방/상한/Left·Kicked를 재검사한다.
(issuer, person_id) binding은 기존 사용자를 재사용하며 프로필을 덮어쓰지 않는다.
(binding, invite) 결과와 (binding, room) UNIQUE를 유지한다. 기존 Joined 멤버는 같은 방의
같은 scope 새 초대로 다른 기기에서도 기존 admission을 재사용하며 새 초대를 소비하지 않는다.
원래 초대의 폐기·만료는 재입장에 영향을 주지 않는다. 다른 scope는 conflict, Left/Kicked는 거절한다.
새 challenge는 invite/browser fingerprint, server incarnation, 만료에 묶이며 상한 있는
메모리에서 원자적으로 redeeming을 선점한다. 중앙 응답 불명·오류는 폐기하고 새
challenge/grant를 요구한다. commit 직전에도 만료/epoch를 검사하며 익명 fallback은 없다.
멤버 세션은 정확한 member_admissions 출처를 검증한다. C4a의 일반 초대 재생,
recovery, Google 연결, retirement, 기기 binding 거절을 유지하고 기기 credential은 만들지 않는다.
검증: 최초/다른 기기/정확 재시도/같은 방 다른 초대/Left·Kicked/browser 불일치/
challenge 재사용·만료/동시 입장/세션 사용/C4a 거절/중앙 오류를 테스트 대역으로 검사한다.
스키마 결정: binding은 C4a 상수 DDL 그대로 생성하되 실제 H1은 **82**다.
기존 reusable 세션의 CHECK/FK가 로컬 device credential을 강제하므로 정확한 member 출처에
한해서 이를 대체하는 세션 테이블 migration이 필요하다. v81의 기존 테이블·credential 의미를
바꾸지 않는 additive 계약을 넘는다. C4a(80/81 reader)는 82를 열기 전에 거절한다.
80/최소81에서 기존 세션·profile·native pairing·bootstrap revision 80을 보존하여 upgrade한다.
세션 수명/종료는 기존 human 세션과 동일하다. Joined 멤버의 새 요청은 기존 admission을
출처로 새 세션을 발급하며, 이전 세션 만료·종료는 재입장을 막지 않는다.

H1 API (양쪽 요청 모두 기존 `x-device-token` 브라우저 credential 필수):
- `POST /api/room-invite/member-challenge` `{invite_token}` →
  `{challenge_id, challenge_hash, server_id, registration_epoch, expires_at}` (Unix seconds, 300초).
  사용 상한에 도달한 초대도 이미 commit된 결과 복구를 위해 challenge만 발급할 수 있다.
- `POST /api/room-invite/member-join` `{invite_token, challenge_id, grant_token, request_id, client_id}` →
  기존 `/api/room-invite/join` 성공 응답 (`session_token`, 사용자/방/서버 정보 포함).
  `client_id`는 기존 join의 공통 정규화를 사용하며 빈 값은 거절한다. 최초/재입장 HTTP 응답은
  이번 요청의 `request_id`와 `client_id`로 기존 프론트 join 검증기를 만족한다. durable 재생 결과는
  최초 request/client ID와 bearer를 보존하고, 출처·멱등성 판단 및 초대 소비 규칙은 바꾸지 않는다.
- 실패 시 session 없이 기존 error envelope를 반환한다. challenge 오류는 401
  `member_challenge_invalid`, 중앙 거절/불명/장애는 502 `member_redeem_failed`,
  challenge 상한(호스트당 1024)은 429 `member_challenge_capacity`; 방/초대 거절은 기존 코드다.
  UI는 명시적 재시도로 challenge와 중앙 grant를 새로 발급하며 자동 재시도하지 않는다.

### 최소판 F1 (2026-10-05, 승인 설계 v3/v3.1)

2026-10-05 입장 화면 보정: 웹 `/join`과 Tauri 공용 `GuestJoinProfilePanel`의
아바타·프로필 사진 버튼 줄은 카드 상단의 일반 레이아웃으로 유지하고, 전체 화면
오버레이는 `.dc-guest-join-panel`만 소유한다. 1024×768에서 익명 “입장”과
“중앙 계정으로 입장”의 중앙 히트 타깃과 실제 클릭을 화면 회귀로 검증한다.
입장 권한·상태 전이·실패/재시도·저장·플랫폼 전송 계약은 변경하지 않는다.

기존 공용 app/web 초대 화면에 중앙 계정 입장을 추가하며 익명 입장과 기존 세션
처리를 보존한다. 기존 중앙 로그인·AA-DEVICE-1 서명·join 응답 검증·세션 적용을
재사용한다. 앱은 메모리에서 challenge→로그인→동의→grant→join을 진행한다.
웹은 호스트 sessionStorage에 무작위 handoff_state별 초대/challenge/서버/epoch/hash/
만료/방 정보를 보관하고 중앙 `/member-join`으로 이동한다. 이동 fragment에는
server_id, registration_epoch, challenge_hash, handoff_state만 둔다. 중앙은 로그인
왕복 동안 대기 정보를 자신의 origin에 보존하며, 현재 계정·중앙의 서버 이름/ID·
정확한 endpoint origin을 표시한 명시적 동의 전에는 grant를 발급하지 않는다.
동의 전 W3 `POST /v1/servers/{server_id}/member-preview`에 기존 중앙 세션/기기 서명으로
`{registration_epoch}`를 보내고 `{server_id,label,endpoint_origin,endpoint_generation}`을
검증한다. 동의한 계정과 preview의 origin/generation이 발급 시 바뀌면 이동/입장을 거절한다.
W2 응답의 endpoint_origin은 정확한 HTTPS origin이어야 하며 고정 `/join` 콜백만
붙인다. 콜백은 파싱/렌더 전에 central-member fragment를 무조건 제거하고 일치하는
호스트 기록을 take-and-delete한 뒤 기존 browser credential로 member-join한다.
grant는 메모리/일회성 fragment 외 저장하지 않는다. 불일치·만료·연결·중앙 오류 및
Left/Kicked/다른 초대 conflict는 한국어 오류와 명시적 재시도를 제공하며 challenge부터
다시 시작한다. 기록이 없으면 원래 초대를 다시 열어야 한다. 자동 재시도·익명 fallback은 없다.
중앙 오류의 명시적 재시도는 preview로 확인한 origin에 grant 없는 retry 콜백을 보내 새
challenge를 시작한다. preview 이전 오류는 신뢰할 반환 주소가 없으므로 이전 초대 화면으로
돌아가며, 로그인 왕복으로 이전 화면도 잃었으면 원래 초대를 다시 연다.
검증은 변경 화면의 앱/웹 왕복·로그인 복귀·동의·origin/상관관계/만료·fragment 선제 제거·
실패 재시도·익명 회귀를 테스트 대역으로 검사한다. Rust/Worker 수정·배포·서명 빌드·
수동 검증은 이번 F1 범위가 아니며 `make verify`는 푸시 직전 한 번 실행한다.

V1 packaged 검증, 예약/outbox/결과 확인/중앙 membership projection 및 목록 동기화,
leave receipt, 익명 merge·전환/scope 변경/reapproval은 유예한다. leave 응답 유실은 결과 불명이다.

2026-10-05 Daybreak Low 2 보정: fragment 입장권이 돌아오는 호스트 `/join` 및
`/join/`의 non-connector HTML은 기존 문서 보호 헤더를 재사용해
`Cache-Control: private, no-store`와 self-only CSP (`frame-ancestors 'none'` 포함)를
반환한다. 공통 CSP 레이어는 경로가 지정한 정책을 보존한다. 일반 앱/자산 및
connector 응답 정책은 유지하고 실제 HTTP 응답의 헤더 값으로 회귀 검증한다.
Low 1 중앙 grant/handoff_state 결합은 관리자 위험 수용으로 이번 수정에서 제외한다.

### 파일 지도

- `crates/agentsassemble-persistence/src/schema_version.rs`, `member_schema.rs`: 현재 82, 고정 C4a binding DDL 및 H1 세션 migration.
- `crates/agentsassemble-persistence/src/central_identity_bindings.rs`: C4a의 일반 identity 경로 거절 유지.
- `crates/agentsassemble-persistence/src/member_admission.rs`, `human_admission_identity.rs`: H1 binding 조회/생성, durable admission 및 정확한 세션 출처.
- `crates/agentsassemble-server/src/member_invite_web.rs`: H1 challenge와 member join; 중앙 redeem은 기존 `central/directory.rs` 소유.
- `crates/agentsassemble-persistence/src/server_memberships.rs` (예정): membership/version CAS; 종료는 기존 removal 소유자와 조정.
- `crates/agentsassemble-persistence/src/member_admission_results.rs` (예정), `membership_outbox.rs` (같은 디렉터리, 예정): durable intent/결과/예약 및 단조 동기화.
- `crates/agentsassemble-persistence/src/human_admission_identity.rs`, `account_identity.rs`, `google_accounts.rs`, `account_guest_retirement.rs`, `guest_identity_recovery.rs`: 기존 profile 생성·credential·retirement·recovery 권위 확장.
- `crates/agentsassemble-persistence/src/host_owner_session.rs`, `crates/agentsassemble-server/src/owner_session_lifetime.rs`: member가 따를 기존 owner 연결 수명 계약.
- `crates/agentsassemble-server/src/central/directory.rs`, `server_identity_web.rs`, `central/registration_web.rs`: 중앙 통신·등록·server-info 검증 경계.
- `crates/agentsassemble-protocol/src/central_member.rs` (예정), `crates/agentsassemble-server/src/central_member_web.rs` (예정): member 전용 타입/issue·redeem·connect·결과 경계.
- `frontend/src/lib/central/identity.ts`, `frontend/src/views/components/CentralServerList.tsx`, `frontend/src/lib/centralMemberConnect.ts` (예정): 앱/웹 공통 목록·재입장·불확실성 UI, 플랫폼은 transport/capability만 분리.
- 중앙 외부 소유 계약: `/Users/seinel/Projects/AgentsAssemble-owner-session/infra/identity-directory/README.md`의 `Security model`, `Server icons (backend contract, 2026-10-04)`, `Abuse and expiry contract (C2)` 및 그 `Grant retry decision`, `Bounded scheduled cleanup and capacity`, `Compatibility and verification` 절을 **C3에서 갱신**한다. 이번 C1은 그 저장소를 수정하지 않는다. member enrollment/projection·삭제/floor 하위 절도 해당 README에 추가할 예정이며 별도 병렬 계약을 만들지 않는다.
- 입장/entitlement/세션 세부 소유자는 [invite 계약](human-invite-admission-session-slice.md#중앙-member-입장과-세션--c1-승인-계약-2026-10-05), participant/server 종료와 room generation은 [lifecycle 계약](room-lifecycle-slice.md#중앙-member의-방-local서버-종료--c1-승인-계약-2026-10-05)이다.

### 요구 동작, 범위와 권위

이 절과 연결된 두 계약은 Daybreak가 6라운드 끝에 승인한 설계 5판+보완의 **구현 전 요구**다. 기존 완료 기록이 member 구현/검증 증거가 되지 않는다. 아래 member 전용 확장은 기존 익명 계약을 대체하지 않는다.

중앙은 신원·발견, 호스트는 로컬 사람·서버 멤버십·방 권한·세션을 소유한다. 중앙 목록은 입장 권위가 아니다. 방 status/role/mute/join/leave는 기존 participant, 방 접근 폐기는 `revoke_room_access`, 계정/서버 삭제와 guest retirement는 각 기존 소유자를 확장한다. 병렬 권위를 만들지 않는다.

첫 슬라이스는 중앙 로그인 후 **새 로컬 사람 생성 또는 기존 binding 재입장**만 제공한다. 기본 avatar만 사용하며 다른 scope의 추가 invite는 conflict다. 익명 합치기·scope 변경·reapproval은 제외하되 후속 요구를 아래 및 각 소유 계약에 보존한다. 기존 익명/Google 흐름은 binding 없는 사람에게 유지한다. 중앙/호스트 floor, non-D1 남용 제한, owner 격리, grant 재사용, bounded cleanup, 삭제/미확정 복구는 노출 선행 조건이다.

진입점은 기존 초대 UI에서 중앙 로그인·기기 서명·연결/목록 동의, member 전용 issue/redeem, `member_connect`, 중앙 서버 목록의 재입장/숨김/숨김 해제/미확정 정리/탈퇴, 기존 account/server delete·registration·logout/device revoke다. 중앙 목록 탈퇴는 호스트 `member.leave`와 같은 명령을 사용하며 오프라인 성공을 기록하지 않는다. 정확한 HTTP 경로는 기존 transport 소유자에서 C3–C5에 연결하며 owner endpoint를 member용으로 재해석하지 않는다.

### Binding, membership, profile과 중앙 모델

- `central_identity_bindings`는 issuer·중앙 person·local user·생성 시각 및 issuer별 person↔local user 양방향 UNIQUE를 갖는다. 이름/이메일/invite/새 credential은 기존 사람의 소유 증명이 아니다. 자동 병합과 binding 이동은 금지한다. 후속 기존 사람 연결에는 정확한 human session·저장 browser credential·기존 recovery 권위로 소유를 증명해야 한다.
- `server_memberships`는 binding PK, `active/left/banned`, 단조 authorization revision, 대표 enrollment를 갖는다. revision CAS가 변경을 소유하고 종료 사유는 호스트에만 둔다. 정확한 버전 참조가 남은 `membership_versions`는 삭제하지 않는다. 일반 입장으로 종료 membership/participant를 복구하지 않는다.
- 필수 `profile_json`/participant ID는 기존 identity/admission 생성 경로에서 만든다. 등록 호스트가 직접 redeem하여 검증한 중앙 display metadata의 길이 제한·정규화 snapshot으로 최초 이름을 정하고 avatar는 기존 기본값을 쓴다. browser 입력/invite 이름/외부 avatar URL은 초기 profile 권위가 아니다.
- 초기 profile은 최초 binding 커밋에서 한 번만 확정한다. 동시 요청은 UNIQUE 경쟁 후 기존 local profile을 재사용하며 서로 다른 중앙 snapshot은 admission conflict가 아니다. 이후 `user_profiles`만 profile 권위이며 re-entry/다른 기기/새 invite는 이름/avatar를 덮어쓰지 않는다.
- 모든 중앙 관계/grant/enrollment/sync는 불변 `server_incarnation=(server_id, host-key fingerprint, 재사용 불가 registration epoch)`에 결합한다. 옛 registration 재활성화는 금지한다. 새 등록은 새 무작위 epoch이고 옛 enrollment/grant/projection/예약/sync를 승계하지 않는다. 클라이언트 지정 person ID API는 만들지 않는다.
- opaque member grant는 별도 prefix·해시, 최대 300초이며 person/session/device/incarnation/origin/generation/purpose/challenge/browser에 결합한다. member 전용 issue/redeem endpoint·Rust 타입·`MemberAdmission`·HMAC bearer purpose를 사용한다. 새 서명 assertion은 도입하지 않고 기존 owner SQL의 owner 일치 조건을 보존한다.
- `member_enrollments`는 고엔트로피 ID·인증/입장 결합·예약 참조·초기 profile snapshot·호스트 결과를 저장한다. 전이는 `pending→redeemed|cancelled`, `redeemed→confirmed|host_rejected`뿐이다. pending은 300초 후 cancelled이며 redeem/cancel/삭제가 같은 행에서 원자 경쟁한다. redeemed는 grant 만료와 독립 보존한다. `abandoned_unknown`은 별도 복구 표식이지 실패/terminal 결과가 아니다.
- `member_projection`은 person/incarnation·host revision/state만 투영한다. visibility는 별도 사용자 권위이며 표시 우선순위는 `owner > active member > bookmark`다. 예약만으로 목록을 만들지 않는다. 같은 person/incarnation 관계는 참조를 공유하고 exact-result tombstone은 visible/관계 용량과 별도 계산한다.
- 호스트 `member_admission_results`는 enrollment별 durable intent·입력·단계·성공/거절·membership revision·parent 참조·전달 상태를 저장한다. **중앙 redeem 전** intent 및 결과/권위 슬롯을 영속 예약한다. `membership_outbox`는 incarnation/membership별 최신 revision/state/event ID·ACK·재시도를 유지한다. 개별 admission 결과는 별도 유지하고 가입 전에 종료 통지 슬롯도 확보한다.

### Enrollment 확정, 예약과 영구 미확정 복구

호스트는 durable 성공 또는 늦은 커밋을 차단한 durable 거절만 서명한다. 중앙은 enrollment/incarnation/호스트 키/결과 일치를 검사하며 성공·거절은 상호 배타적이다. 응답/DB 장애는 enrollment ID로 중앙 상태와 호스트 intent/결과를 재조회한다. 불확실성을 거절로 바꾸지 않고 grant 만료 뒤에도 커밋된 성공을 confirm한다.

Enrollment 생성은 진행·visible/관계·결과·unknown 수용 용량을 원자 예약한다. 아래 참조 변경은 결과 전이와 같은 트랜잭션이다. 공유 관계/visible 슬롯은 참조 0이고 실제 projection/owner/bookmark가 없을 때만 해제한다.

| 전이 | 진행 슬롯 | visible·관계 참조 | 결과 보존 |
| --- | --- | --- | --- |
| 생성→pending | +1 | 필요한 공유 참조·용량 예약 | 결과·unknown 슬롯 예약 |
| pending→redeemed | 유지 | 유지 | intent 결과 수용 예약 유지 |
| pending→cancelled | 해제 | 해당 enrollment 참조 제거 | 별도 bounded exact-result tombstone으로 전환 |
| redeemed→confirmed | 해제; unknown이면 이미 해제 | 실제 projection 참조로 전환; hidden/삭제 유지 | 확정 결과·ACK 보존 |
| redeemed→host_rejected | 해제; unknown이면 이미 해제 | 해당 enrollment 참조 제거 | 별도 bounded exact-result tombstone으로 전환 |
| redeemed→unknown 표식 | 해제 | 불필요한 visible 참조 제거; 숨김 관계 참조 유지 | compact 결과·provenance 예약 유지 |
| unknown→늦은 확정 | 추가 해제 없음 | 성공은 hidden projection; 거절은 해당 참조 제거 | 성공/거절과 동일 |

중복 전이는 refcount를 다시 바꾸지 않는다. tombstone 압축/정리는 재생 방지와 결과 재조회 계약 충족 뒤에만 수행한다. 사용자 명시 정리는 unknown 표식과 예약 전환을 원자 적용하며 “호스트 결과 미확인·목록 숨김”으로 표시한다. 탈퇴 성공이 아니다. 늦은 결과는 확정하되 숨김 해제나 parent 신규 발급은 하지 않는다. unknown은 진행 한도를 회복하나 무한 보관을 보장하지 않는다. 보존 용량 포화는 명시적 오류로 신규 가입만 제한하며 기존 미확정을 삭제하거나 거짓 실패로 만들지 않는다.

### C3c-1b 호스트·공용 프론트 epoch 전달

등록/bootstrap에서 받은 opaque `registration_epoch`는 호스트의 기존 `runtime_metadata`와 프론트 서버 목록에 server identity와 함께 저장하고, 등록 갱신·claim·endpoint publish/renew/offline·grant/redeem·이름·아이콘 및 존재하는 삭제/북마크 요청의 최상위 JSON에 포함한다(스키마 상승 없음); epoch 없는 업그레이드 상태는 다음 응답까지 필드를 생략하며 등록/claim proof는 epoch가 있으면 Worker의 LF 구분·마지막 LF 없는 `AA-HOST-REGISTER-2`/`AA-HOST-CLAIM-2`를 사용한다.
자기 서버의 일반 등록만 409 `incarnation_conflict`에서 저장 epoch를 지우고 같은 키로 한 번 재등록하며, claim·다른 경로 및 두 번째 실패는 오류로 남긴다; 저장·실제 요청 body·Worker와 동일한 proof 바이트·단일 재시도·epoch 없는 상태를 영향받는 자동 테스트로 검증하고 배포·서명 빌드·수동 검증은 제외한다.

### 기존 account/server deletion 및 registration 확장

중앙 `Security model`의 계정 삭제/서버 등록 삭제 및 owner account의 서버 등록 terminal 전이, `Server icons`의 삭제 계약은 C3에서 다음 요구를 적용한다. 삭제 상태·최소 person/incarnation/key provenance를 물리 삭제와 분리한다. redeemed 결과·예약·검증키를 cascade 삭제하지 않고 삭제 incarnation은 terminal/hidden으로 유지한다.

삭제가 먼저면 pending 취소와 신규 issue/redeem 차단을 원자 수행한다. redeem이 먼저면 provenance·예약을 보존하고 결과/ACK만 처리한다. **member 계정 삭제, owner 계정의 서버 등록 terminal 전이, 명시적 server 삭제** 모두 같은 계약이다. 삭제는 pending만 취소하며 redeemed/unknown 참조를 결과 수용 전까지 보존한다. 삭제·미확정 정리는 redeemed를 실패로 추정하지 않고 늦은 결과 검증 provenance/수용 용량을 보존하며 삭제 계정/incarnation을 부활시키지 않는다.

결과/ACK 뒤 개인정보·상세 기록은 최소화하되 incarnation 종료 표식·최종 revision·재생 방지 결과 식별자는 유지한다. 미확정 provenance는 시간만으로 삭제하지 않는다. 중앙 삭제를 호스트 탈퇴로 표시하지 않는다. 검증키 보존은 늦은 결과 검증만 허용하며 삭제 호스트의 신규 입장 권위가 아니다.

### 재입장, 목록 동기화와 장애

재입장은 저장 incarnation과 중앙 live 등록, `/api/server-info/challenge`의 키/fingerprint/same-origin/generation을 대조한다. 삭제/불일치 시 grant/credential 전송과 서버 UI 신뢰를 중단한다. `member_connect`는 invite를 고르지 않고 active membership과 현재 canonical entitlement만 사용하여 기기별 parent를 발급한다. child 재발급은 현재 방 접근·Joined participant·entitlement를 검사하며 중앙 호출을 하지 않는다.

Signed enrollment/incarnation/revision/state/event ID에서 같은 revision/내용은 ACK, 같은 revision의 다른 내용은 conflict, 낮은 revision은 무시한다. hidden·삭제·종료 revision은 부활시키지 않는다. 숨김 해제는 사용자 명령으로 live incarnation/용량을 재검사한다. 숨김·bookmark 삭제·서버 탈퇴는 별개이며 독립 bookmark는 유지해도 삭제 incarnation 입장은 차단한다.

중앙 장애는 기존 연결을 유지하고 신규 중앙 연결 실패를 표시한다. 호스트 커밋 후에는 “입장 성공·목록 동기화 대기”와 durable 재시도를 제공하며 익명 fallback은 없다. 중앙 logout/기기 폐기는 다음 입장부터 적용한다. 계정 탈퇴는 위 Account deletion 계약의 비동기 tombstone/ACK를 통해 이미 입장한 권위와 파생 연결도 종료하며, 호스트 확인 전에는 cleanup_pending으로 남긴다. member는 owner/operator/pairing 생성 권위를 얻지 않는다. 메시지·room bearer·방 역할·ban 사유는 중앙에 저장하지 않는다. 비밀값은 로그·URL query·분석 이벤트에 남기지 않는다.

### 중앙/호스트 예산과 노출 장벽

수치는 승인 설계의 구현 요구이며 측정 완료 주장이 아니다. 모든 상한은 원자 적용하고 기존 더 엄격한 제한·사용자 데이터를 보존한다. 포화는 신규 생성만 막고 leave/kick/ban·폐기·기존 권위 검사·예약 결과 확정을 막지 않는다. 호스트 challenge/세션 상세 수치는 invite 계약이 소유한다.

- non-D1 limiter는 owner issue/redeem/publication과 member issue/redeem/sync의 **비차용 예산·장애 영역**을 분리한다. 서명/D1 nonce 전 목적/IP, 인증 후 account/session/device/server/host 예산을 fail-closed로 적용한다.
- 동일 유효 grant 결합은 재사용하며 redeem 때 현재 권위를 재검사한다. grant 활성 account 16, session/server 4; issue account 10/분·session 5/분; redeem account/server 20/분·host 120/분; sync host 12/분. burst는 각 분당 한도 이하이다.
- 중앙 visible은 예약 포함 256, 보존 관계 1,024, 진행 enrollment 16, unknown compact 결과 1,024다. **terminal exact-result tombstone·leave receipt·삭제 provenance의 별도 행/byte/전역 예산과 보존 기한은 노출 전에 확정**한다. 승인 설계에 없는 수치를 C1에서 임의로 만들지 않는다. leave receipt 보존은 lifecycle 절의 24시간이다.
- host binding/membership은 10,000; outbox는 membership당 1행, 총 10,000행/10MiB, 배치 100건/128KiB; 미전달 admission intent/결과는 256건/1MiB다. 종료/receipt 슬롯은 admission 때 확보한다.
- grant/nonce/challenge 정리는 작업당 100행/1초 이하이다. 재시도는 host당 단일 작업, 1/2/4/8/16/30분+jitter, 시간당 최대 6회이며 재시작 후 이어간다. 영구 오류는 중단·표시한다. 노출 전에 정상/공격/장애의 D1·인덱스·CPU·정리·일일 예산을 측정한다.

### C2–C6 순서, 중앙 floor와 host schema

1. **C1 (이번 변경): 계약만.** 아래 acceptance는 향후 필수이며 코드·migration·테스트 코드나 외부 중앙 저장소 변경은 포함하지 않는다.
2. **C2군:** 목적별 limiter/owner 격리 → grant 재사용 → bounded cleanup·생성/재시도/결과 보존 예산을 책임별 구현·검증한다. 완료 전 member 기능을 노출하지 않는다. 기존 중앙 C2 기록이 member 예산/격리까지 증명하지는 않는다.
3. **C3a 중앙 호환 floor:** 중앙 README `79a181cd`의 결정에 따라 서버 ID tombstone·terminal 재등록 거절 대신 C3c에서 등록마다 새 무작위 registration epoch로 incarnation을 구분하여 재등록이 옛 관계·grant·enrollment를 상속하지 않게 한다. member 소유 레코드는 cleanup이 지우는 `sessions`·`server_connect_grants`를 FK로 참조하지 않고 필요한 출처를 값으로 복사한다. 과거 C3a의 migration 없는 FK RESTRICT/409 `deletion_restricted` 배포는 역사적 기준선이며 현재 계정 탈퇴 장벽으로 대체한다. 현재 순서는 schema-independent atomic guest provisioning floor → blocking additive BEFORE DELETE guard → deletion/host custody writers → 공용 UI다. 옛 physical DELETE/cascade를 FK 실패만으로 보호한다고 가정하지 않는다.
4. **C3b 중앙 전환 장벽:** 모든 트래픽/배포 경로가 floor 이상이고 구 Worker가 D1을 변경할 수 없음을 확인한 뒤 member migration/경로를 배포한다. 혼재는 floor↔new만 허용하며 floor 이전 rollback은 금지한다. 구 코드가 남으면 장벽 실패로 진행하지 않는다.
5. **C3c군:** incarnation/삭제 provenance → enrollment·예약/terminal 해제·unknown → 단조 결과/projection을 내부 경로로 구현한다. 중앙 floor/new 양방향 삭제·재등록·결과 경쟁을 검증한다.
6. **과거 C4a host floor (역사적):** v80 유지·v81 인식 floor는 2026-10-05 당시 배포 순서이며 현재 schema 지시가 아니다. 현재 계정 탈퇴는 직접 확인한 schema 86을 기준으로 다음 미사용 버전(현재 87 후보)을 예약해 additive lifecycle을 구현한다. 이미 적용된 v81–86을 재번호화하지 않고 frozen binding DDL와 구조 게이트를 보존한다.
7. **과거 C4b군 (역사적 배포 순서):** host floor 확인 후 **81 migration** → binding/profile/membership → canonical entitlement·기존 participant/invite/retirement → lifecycle 폐기·서버 명령/receipt·intent/outbox를 외부 도달 불가로 연결한다. 다른 작업이 먼저 81을 쓰면 member는 다음 미사용 고유 번호로 재배정하고 이 계약을 갱신한다. floor 이전(v79/v80) 코드는 member DB를 열지 못한다. floor는 미지원 입장을 거절하고 독립 익명 권위를 재발급하지 않는다.
8. **C5:** 초대·재입장·목록·unknown/삭제·방 lifecycle·방-local/서버 종료·전 기기 폐기의 완전 수직 슬라이스 검증 후 앱/웹 공통 UI/route를 함께 노출한다. 플랫폼별 별도 제품 흐름은 허용하지 않는다.
9. **C6군:** 실제 두 기기 앱/웹, 중앙 floor/new 혼재, 현재 계정 탈퇴의 `v86 → 다음 미사용 lifecycle schema` upgrade (과거 `v80 → floor(81 인식) → member schema` 검증은 역사적 증거), migration 후 각 floor로 허용 rollback·floor 이전 rollback 금지, 필수 구조/실행 게이트와 계획 소유자 리뷰를 완료한다. 모든 단계에서 native pairing `session_expires_at = 0`, last-use, 30일 미사용 만료, 단일/전체 revocation을 보존한다.

각 구현 커밋은 문서/테스트 포함 1,000줄 미만으로 독립 빌드·검증·허용 floor rollback이 가능해야 한다. 중앙 floor와 host floor는 별도 장벽이며 구 Worker 잔존이나 미확정 보존 예산 미결정은 기능 노출을 막는다.

### C4a: schema 81 최소 저장 계약 (2026-10-05, 역사적 floor)

C1 `e5134655` 당시의 v80 유지/v81 인식·82 이상 거절 및 migration 미실행 지시는
역사적 floor 동작이며 현재 계정 탈퇴의 schema 지시로 쓰지 않는다. 현재 source가
86을 소유하므로 다음 미사용 버전을 직접 확인·예약하고 이미 적용된 migration은
재번호화하지 않는다. 아래 v81 binding DDL의 frozen 구조와 익명 권위 재발급 거절은
계속 보존한다. 추가 lifecycle/tombstone/custody는 별도 additive 저장으로 구현한다.

```sql
CREATE TABLE central_identity_bindings (
    binding_id TEXT PRIMARY KEY NOT NULL,
    issuer TEXT NOT NULL,
    person_id TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES user_profiles(user_id) ON DELETE RESTRICT,
    created_at INTEGER NOT NULL,
    UNIQUE(issuer, person_id),
    UNIQUE(issuer, user_id)
) STRICT;
CREATE INDEX central_identity_bindings_user ON central_identity_bindings(user_id);
```

`binding_id`는 불변 host 식별자, `issuer`는 검증된 중앙 issuer의 canonical 식별자,
`person_id`는 해당 issuer의 불변 person, `user_id`는 기존 local profile ID,
`created_at`은 UTC Unix microseconds다. binding은 membership 종료/중앙 삭제에도
존재 표식으로 보존하며 이동/익명 전환하지 않는다. floor는 membership 상태와
무관하게 **어느 issuer든 해당 user_id 행 존재**만으로 미지원 member를 판정한다.
81 DB open은 `sqlite_schema`에서 `tbl_name = 'central_identity_bindings'`이고
`sql IS NOT NULL`인 객체가 정식 테이블과 user 인덱스 두 개뿐인지 읽기 전용으로 확인한다.
각 `sql` 원문은 `central_identity_bindings.rs`의 정식 DDL 상수와 바이트 그대로 비교하며
정규화하지 않는다. C4b migration은 같은 상수를 그대로 실행한다.
기존 변형·충돌 정책·추가 제약과 겹친 따옴표 식별자, 추가 트리거·UNIQUE 인덱스는
거절하고 정상 v80/v81은 계속 허용한다. 이 이상의 로컬 DB 쓰기 권한자의 적대적 조작은
위협 모델 밖이며 검사 범위를 넓히지 않는다.

C4b는 기존 80 테이블/열/credential 의미를 바꾸지 않는 additive migration이어야
한다. 81은 DB 스키마 번호이며 bootstrap 권위 계약의 새 revision이 아니다.
기존 `local_bootstrap_authority.schema_revision`/digest는 유지하고, 새 81 DB도
floor가 이해하는 기존 bootstrap 계약 revision 80을 사용한다. bootstrap 의미
변경은 별도 floor가 필요하다. member 전용 parent/child/결과는 익명/owner 권위
행으로 인코딩하지 않는다.
추가 member 테이블의 FK/trigger는 floor의 기존 쓰기가 member 권위·revision·폐기·
결과를 삭제/재활성화하지 않도록 해야 한다. floor는 member 전용 테이블을 쓰거나
정리하지 않는다. C4b는 실제 전체 migration DB로 이 호환 행렬을 재실행해야 한다.

영향 진입점은 DB open, 기존 human invite 최초/정확 재시도, human session 조회/
재검증, account device 해석·binding, recovery 발급/최초 redeem/정확 재시도,
Google 연결 및 guest retirement다. 각 기존 소유자 트랜잭션에서 binding을 검사해
`central_member_unsupported`로 거절하며 독립 credential/입장 결과를 반환하지 않는다.
브라우저 credential의 기존 user binding 검사는 초대 종류 및 정확 재시도 분기보다 먼저
수행한다. 1회용 최초 입장·동일 요청 반복과 binding 전 사용한 초대의 정확 재시도도
무변경 거절한다. 새 member route나 입장 API는 만들지 않는다. binding 없는 익명/Google 및 native
pairing의 idle 수명·last-use·30일 미사용 만료·단일/전체 revocation은 보존한다.

C4a 자동 검증은 변경 전 실패 재현 후 80의 schema/데이터 보존, 최소 81 DDL 및
추가 member 상태 행 보존, member 입장/기존 session 재사용 거절, recovery 발급 및
최초/재시도 redeem·새 device/Google binding 거절, 익명 정상, 82 이상/잘못된 81
거절과 native pairing 회귀다. C4a에서는 배포·서명 빌드·수동 검증을 하지 않으며
C6의 실제 migration/혼재/두 기기 검증 완료로 보고하지 않는다.

### 필수 테스트와 수용 기준

- Binding 양방향 UNIQUE 경쟁, 자동 병합/독립 익명 권위 재발급 금지, 기존 익명/Google 흐름 보존; 두 기기 snapshot이 달라도 단일 local profile·기본 avatar·재입장 비덮어쓰기.
- 모든 예약 전이의 refcount/중복 무효과, cancelled/host_rejected 반복 후 용량 회복, phantom 목록 없음, 공유 owner/member/bookmark 보존, 예약 confirm 성공, terminal tombstone 별도 포화.
- Redeem 전 intent/슬롯 영속성, DB rollback·응답 유실, 16건 unknown 이동, 늦은 성공/거절과 hidden 유지, compact 포화, redeem/cancel/세 종류 삭제 각각 양쪽 경쟁 순서, provenance/ACK·비부활.
- 구 Worker 잔존 시 장벽 실패; 중앙 floor/new 양방향 삭제·재등록·cascade 방어·금지 rollback; 같은 ID/다른 키·옛 sync/stale endpoint의 관계 상속과 credential 전송 차단; host floor rollback의 권위 보존 및 위 native pairing 전체 행렬.
- 잘못된 issuer/incarnation/key/epoch/origin/generation/purpose/device, 만료·동시 redeem, member→owner/operator 교차 사용 거절과 owner 흐름 회귀. 역순 revision·hidden·표시 우선순위 및 중앙 장애 표시/재시도 검증.
- Source/browser/invite 회전, 행/byte/receipt 상한, 정리 장애·재시도 예산, member 과부하 중 owner 입장/publication 보존. 단위/구조 통과만으로 수용하지 않는다.
- 기기 A 초대 → 기기 B 중앙 목록/재입장 → owner 서버 kick으로 양쪽 차단을 **실제 앱/웹 두 기기**에서 확인한다. invite/lifecycle 및 아래 retirement/recovery 절의 필수 검증도 모두 통과해야 한다. 미검증 흐름과 비용은 완료로 표시하지 않는다.

## Definition and dependency order

Complete the retained account, friend and human admission flows discovered in the
original reachable account/social/invite routes, native login return broker and
frontend callers. Existing Rust human admission, profile, recovery and local
bootstrap behavior remains authoritative after cutover. Do not restore v0 meeting
or agent-research code, Freebuff, or managed Antigravity.

First export browser-device credential and human-session bearer wire constants
from their distinct protocol domains (C-13 human portion). Generation, parsing,
fingerprinting, authorization and boundary-specific errors stay with their current
owners. Audit every human signed-invite claim for a finite production consumer;
remove fixed self-description whose only consumer repeats its literal (D-07).
Preserve checked scope, expiry, target room, ingress and durable credential binding.
No compatibility parser or migration of old user data is introduced.

Then connect the phase-wide owners and one complete vertical flow for each target:

- Local/public account status, Google challenge, verified connection and disconnect.
  The server persists links to the existing human profile and binds proof to the
  requesting identity/device. A Google account already linked to another identity
  requires the original explicit guest-discard decision and an atomic server-owned
  switch. A paired operator cannot create durable account authority.
- Central Google native handoff through the existing central identity service and
  PKCE flow. The native host permits only the exact Google authorization surface;
  the local return owner binds state, bounded capacity/expiry, cancellation and
  returned code without persisting or logging credentials. Central guest creation,
  bootstrap and recovery remain separate from local server account links.
- Durable saved friends and their reachable add/edit/delete/filter/invite flow.
  A saved contact is not a room admission grant or proof of live provider status.
  Preserve supplied provider identity and distinguish human invites from external
  AI admission, whose execution belongs to Phase 7.
- Operator-pairing create/redeem/revoke bound to an exact room incarnation, current
  local host, ready ingress origin, device and short expiry. Atomic redemption
  prevents duplicate use and cannot upgrade an ordinary human invite. Revocation,
  room closure and departure use existing session/lifecycle owners. Local host
  claim stays with current bootstrap rather than a parallel browser host identity.
- Human invitation presentation, preflight/join/recovery/leave and errors continue
  through the existing canonical admission and retained retry owners. Do not merge
  Human Invite, Room Connector and AgentBridge credentials or lifecycle state.

## Authority, failures and verification

`SqliteStore` remains the durable writer; current HTTP admission and local-purpose
control tickets remain transport boundaries. Add only necessary identity/account/
friend/pairing records at the clean current schema. Revalidate mutable authority in
its committing transaction, publish room changes through the existing event owner,
and retain exact retry semantics where an external effect or admission can commit
before the response arrives. Do not expose success for uncertain saves or expired
proof, invent an account from an invalid token, or use browser state as authority.

Use maintained libraries for OAuth/JWT infrastructure and the existing bounded
HTTP client and task-cancellation owners. Any callback polling must have the real
browser-return lifetime, a fixed bound and termination on success/cancel/expiry;
no new background heartbeat is justified by this phase.

Acceptance covers every visible account/friend/invite control with complete server
behavior and no absent startup route. Verify authorization, wrong identity/origin/
room, replay/conflict, expiry/revoke, rollback and restart at the affected boundaries;
reuse current profile/admission tests. Directly operate the packaged app at desktop
and 390px widths through open/edit/cancel/save/error and invite flows, preserving the
UX guide's margins, focused dialogs and concise menus. Run affected tests and all
unchanged structure/security/CSS gates, measure phase resource costs and obtain
Daybreak whole-phase approval before Phase 6. Real providers and the final Pro
review remain deferred by user instruction; local fixtures do not prove those runs.

### Native startup return boundary

`StartupIdentityBoundary` admits the central login screen only in the desktop host;
ordinary browser startup is unavailable without invite/pair/recovery authority.
Central login precedes local profile bootstrap. Its transient start/poll/cancel
therefore uses the private host-to-authentication-process control pipe, carrying no
fake operator principal and granting no room/account authority. The authentication
service owns one bounded pending-return collection (16 entries, 10-minute expiry), and the
loopback-only HTTP callback accepts only an expected unguessable state. No public
start/poll endpoint or new reusable credential is necessary. The native host derives
the redirect URI from its exact owned callback listener and opens only the validated Google
authorization URL. The existing central service remains the OAuth/PKCE exchange
owner. Completion/abort/failure retires transient state; expiry is checked on access,
and process shutdown drops it without adding a polling task or persistence table.

The client retires the transient native return before exchanging its captured code
with the central service. Retirement failure remains visible and prevents exchange;
cancellation is checked before exchange. A received completed exchange is persisted
immediately, with no later cleanup or abort check that can discard the issued session.
Earlier failure/cancellation still attempts native retirement exactly once.

### Google profile defaults and central logout (2026-10-01)

User request: import Google's name/photo as initial profile defaults and expose
logout in desktop account settings. Google subject remains the identity key;
`openid profile` supplies display metadata, never account authority. The central
Worker stores the first verified profile (including one upgrade of existing
Google placeholders). No email scope or account merge is added. The local profile
owner imports only a pristine revision, stores the photo through the existing
bounded raster attachment pipeline, and preserves all later user edits. Remote
photo reads require exact HTTPS Google image origins, no redirects, bounded bytes
and a finite timeout; unavailable photos produce a visible retryable failure.

Logout revokes only the current signed central session before persisting explicit
logged-out presentation state and returning to startup. Failure retains the
session and settings with a retry. Restart must remain on login; successful login
replaces the logged-out state. Local operator authority, rooms, profile and other
devices' sessions are retained. This is central logout, not local server shutdown
or an OS-user security boundary. Browser room-account linking is unchanged.

Correction for the reported Windows guest logout -> Google login flow: explicit
logout makes the next login use a fresh durable central device slot. Commit removal
of the previous local credential before persisting the fresh-slot logout marker;
storage failures remain visible and login blocked. Existing logout markers from
the previous build take the same transition. The old central guest/device records
are preserved. A failed/cancelled login reuses the new slot until another logout.

Only a session obtained after logout or invalid/expired central authentication requests a native host ownership
claim (`AA-HOST-CLAIM-1`), distinct from ordinary registration. Central device
signature, verified native host signature for the exact destination account, and
matching previously registered host key are required. The Worker consumes the
claim nonce once and atomically moves that server's central owner relation;
preserve the previous account's bookmark, the endpoint and all local room data.
Enforce the existing 20-server bound inside the atomic update. Persist pending
claim state until successful registration; startup retries unfinished claims with
a fresh native proof. No identity merge, profile retirement, room ACL change,
server key replacement or registration-conflict fallback is permitted.

The outage-continuity section above supersedes only the transient-failure branch
of this earlier startup contract. When central login is configured, a completed local
profile never substitutes for a current central login. Validate the saved session
through `/v1/bootstrap` before opening the room directory; a missing/revoked session
requires login, and transport failures remain visible. Rejected or expired sessions
retain the logged-out marker and renew the durable account slot at the next login
so switching accounts cannot reuse the revoked account binding. A session removed during
validation cannot use cached person data to enter. Refresh cached display metadata
only for the same still-current token, and show the actual central account name
separately from editable local profile fields. Preserve the existing explicit
profile-edit contract and the latest callback-page styling.

Acceptance: verified Google name/photo import; edited profile survives re-login;
current central bearer is rejected after logout while another device remains valid;
startup stays logged out across restart; failed logout is retryable. Verify affected
Worker, native URL, profile and frontend contracts, mandatory gates and a signed
isolated package while preserving the active Windows room.

### Host-owned owner workspace and devices (2026-10-04, supersedes renewal)

이 기능을 맡는 파일 (paths relative to this repository unless marked Worker):

- `crates/agentsassemble-persistence/src/schema_version.rs`: additive upgrades and retirement of historical owner authority.
- `crates/agentsassemble-persistence/src/host_owner_session.rs`: durable admission, live custody and transactional revalidation.
- `crates/agentsassemble-persistence/src/owner_devices.rs`: account-scoped device listing and atomic dependent revocation.
- `crates/agentsassemble-server/src/central/directory.rs`: signed central admission and endpoint publication generations.
- `crates/agentsassemble-server/src/central/owner_web.rs`: browser admission, owner directory/room HTTP and event stream entry.
- `crates/agentsassemble-server/src/owner_session_lifetime.rs`: transport retention, last-disconnect, ingress/runtime end and closure.
- `crates/agentsassemble-server/src/room_socket.rs` and `room_socket_session.rs`: pre-frame retention through socket-loop exit.
- `crates/agentsassemble-server/src/owner_devices_web.rs`: native/remote device HTTP routes and committed revocation publication.
- `crates/agentsassemble-protocol/src/central_owner.rs`: canonical owner/device wire types; `frontend/src/types/generated/` derives from these.
- `frontend/src/lib/central/ownerWorkspace.ts`: in-memory owner credential exchange, device description and room requests.
- `frontend/src/lib/ownerSessionTransport.ts`: root HTTP rejection notification owned by the mounted workspace boundary.
- `frontend/src/lib/roomDirectorySubscription.ts`: bounded stream recovery and network-online listener cleanup.
- `frontend/src/views/components/CentralOwnerConnectGate.tsx`: verified new-entry gate without an ongoing lease timer.
- `frontend/src/views/components/CentralOwnerWorkspaceBoundary.tsx`: active/ended workspace presentation preserving drafts.
- `frontend/src/views/components/UserSettingsPanel.tsx`: shared native and remote owner settings entry, including empty servers.
- `frontend/src/views/components/OwnerDevicesPanel.tsx`: identifiable device rows and single/all revocation confirmation.
- `frontend/src/api/ownerDevices.ts`: shared settings transport and strict device response decoding.
- `crates/agentsassemble-persistence/src/host_owner_session_tests.rs`: durable lifetime, generation recovery and account-scope regressions.
- `crates/agentsassemble-server/tests/central_owner_boundary/`: HTTP/socket/stream and dependent revocation acceptance.
- Worker `infra/identity-directory/src/server_connect_grants.js`: atomic current-account admission; no ongoing owner renewal.
- Worker `infra/identity-directory/test/local_owner_connections.mjs`: isolated workerd/D1 admission and logout verification.

Security review corrections (2026-10-04): upgrades from v72/v73 and already-v75
must revoke every `central_owner = 1` room session without a host-owned parent,
while preserving room data and native pairing authority. Socket establishment must
revalidate and retain host owner custody before any frame, carrying that lease
through the socket loop; cancellation before retention sends no snapshot. Directory
recovery has at most four attempts per network-online trigger, with owned listener
cleanup and no polling. Root-authenticated requests receiving 401/403 end the
workspace and preserve drafts. Regression tests must fail before each correction;
production deployment, remote commands and signed/manual app/web checks stay open.

Endpoint publication generation validates new admission only. Recovery after a
central outage (including publication failure beyond 600 seconds) must not retire
an admitted owner or its derived room authority. Existing host identity, device,
origin, revocation, disconnect and ingress/runtime lifetime checks remain in force.
Regression verification advances the publication generation with active authority,
rejects stale new admission, and confirms explicit revocation still ends access.

Latest user decision: central identity is admission/discovery authority, not an
ongoing gatekeeper. The entry grant remains short and host-signed redemption checks
the current session/person/device, owner relation, server and exact endpoint. After
entry, server-owner and directly derived room authority last until host-owned
revocation, complete workspace disconnection, ingress replacement or runtime end;
there is no periodic owner renewal, 60-second lease, or central-expiry timer.
Central logout, device revocation and ownership transfer affect the next admission.
An already connected owner retains administrative power until host revocation or
disconnection for these events; the user accepts this tradeoff. Account deletion
instead follows the Account deletion asynchronous tombstone/ACK contract above:
the host terminates existing owner and derived authority when it confirms the
terminal identity; central result remains cleanup_pending until exact host ACK.
Unreachable central identity blocks a new admission, while admitted workspaces
continue through central outages. Central credentials/private keys stay on the
selecting device. The unused central `0009_owner_connections.sql` and its routes,
tasks and tests are removed before any production deployment.

Reconnection means a new page/workspace or return after every authenticated
workspace transport has closed, including host restart. A cached owner credential
cannot open a new workspace after that boundary: a fresh central admission is
required. Room changes and additional room transports within a still-connected
workspace require no central request. The host owns admission, live retention and
disconnect settlement; client flags/storage never confer authority. A bounded
entry window permits the initial directory/room connection and exact same-device
issuance retry, but never refreshes an ongoing session. Exact grant replay returns
the same still-admitted session, rejects a different browser and cannot revive a
disconnected or revoked session. A different new grant may admit a new session.
Preserve previously stored data during the local schema upgrade; old leased
authority is not promoted to new host-owned authority.

Shared settings expose `기기` on the host app and centrally admitted owner
workspaces, including an empty server or no selected room. Rows show a bounded
device name or browser/OS, last connection time and a server-derived `이 기기`
marker. Display metadata is untrusted description, not proof of identity; do not
expose raw credentials, their fingerprints, central private data or network addresses.
Root admission and operator pairing redemption accept that description; operator
socket-ticket exchange may update it, while a physically empty exchange retains
the stored description and still records the connection time. Ordinary human
socket-ticket requests retain their empty-body contract. Malformed or oversized
metadata fails before its authority/storage effect. Product surface revision 19
records the additive operator exchange body.
The current native host is an explicit non-revocable recovery anchor. Native
authority can list/revoke all remote device sessions; remote authority can only
list/revoke the exact same central account's sessions. Thus an old owner retained
across transfer cannot revoke a new owner's sessions. Single and account-wide
revocation include the caller if selected and settle atomically in storage before
publishing transport closure. Failed persistence never reports successful revocation.

Direct room sessions and owner-issued operator pairings retain their host owner
provenance. Revocation also invalidates those dependent credentials/attendee
authority and closes idle sockets and directory streams without a heartbeat or
subsequent command. Owner-issued pairing is not an escape from device revocation;
ordinary native-issued pairing and independently invited guests keep their existing
authority. Issuer disconnection does not silently revoke a separately paired
device; explicit issuer revocation still reaches it. Revocation racing a mutation
is ordered by its authority transaction: already committed work remains committed,
and later work fails authorization. Closing a waiting socket does not cancel an
accepted room command or erase its receipt.

User decision (2026-10-05): the v77 upgrade, including databases already at v76,
once revokes all remote operator pairing grants/sessions without a host parent,
regardless of `central_owner`, and cuts off their derived authority at pairing
redemption, session authorization, further issuance and attendee admission/use;
legacy provenance cannot distinguish native-issued from old central-issued pairings.
Remote devices must reconnect once; the native host and local operator authority
remain intact, and pairings created after this upgrade survive later opens.

Central request budget, excluding retries/login/registration/other product actions:
connected owner renewal **0 requests and 0 D1 writes per user per day**, independent
of duration or user count. Each admission uses **2 Worker requests** (grant issue
and host redemption); fetching the signed server list adds **1**. Their SQL changes
are **4 logical row writes per admission**, plus **1** for list-request nonce
custody, before indexes and bounded expired-record cleanup. D1 meters rows, not
HTTP calls, and indexed writes add to this count; verification records measured
local D1 metadata rather than presenting logical rows as billed rows. Existing
5-minute endpoint publication is discovery cost, **288 requests per online server
per 24 hours**, independent of connected users. The removed 20-second owner loop
would have used **4,320 requests per user per 24 hours** (23 users: **99,360**),
before endpoint, entry and other requests. Free allowances currently total
100,000 Worker requests/day and 100,000 D1 rows written/day:
[Workers](https://developers.cloudflare.com/workers/platform/pricing/) and
[D1](https://developers.cloudflare.com/d1/platform/pricing/).

Acceptance: one central admission and zero owner-related central calls across more
than five minutes of messages, idle time and room changes; central logout leaves
that admitted workspace usable but its next admission fails. Exercise host and
same-account other-device list/single/all revoke, self revoke, empty server, foreign
account denial after transfer, immediate idle socket/stream closure, derived pairing
revocation, storage failure, disconnected/foreign-device/grant replay denial, runtime
end and additive data preservation. Common UI keeps drafts and reports loss of
authority explicitly; checking/unavailable invite status never claims external
access is off. Run affected tests, mandatory gates and direct signed-package
manipulation. Actual signed-in web proof and required code review remain open until
completed. Production Worker deployment and migration require explicit user approval.

### Central owner server reopening (2026-10-01)

The admission and host-proof boundaries remain current. Its short room-session
lifetime below is superseded by Host-owned owner workspace and devices.

After a current central session is validated, the desktop server chooser may reopen
only a server whose current central relation is `owner`. Bookmarks remain discovery
metadata and continue through their explicit invite or pairing authority. Selecting
an owned server asks the central Worker for a five-minute, server-bound connect grant.
The grant is bound to the issuing central session, person, device, server, exact
published endpoint origin and endpoint generation. Logout, session/device/person
revocation, owner transfer, endpoint replacement, lease expiry or server revocation
invalidates it. The central bearer and device private key never leave the selecting
device and are never sent to the remote room host.

Desktop reopening passes the frontend-generated `/pair#central-owner=...` URL
through `open_central_owned_server`. Its bundled-UI caller check remains required;
the URL boundary permits only HTTPS with a host, no userinfo or query, the exact
`/pair` path, and a `central-owner=` fragment of at most 16,384 bytes. `/app` and
other paths are rejected. This uses the same public pairing shell as web reopening;
grant redemption and host proof remain the authority owners. Desktop regression
tests must accept `/pair`, reject `/app` and unsafe URLs, and cover the fragment
length boundary. For this path correction, run desktop and related frontend tests,
then one pre-commit `make verify`; deployment, signed builds and manual flows are
outside the requested verification scope.

The selected host redeems the opaque grant directly with the fixed central Worker,
signing the exact request with its durable Ed25519 host key. The Worker revalidates
all mutable authority and the current endpoint lease before returning the authorized
central person/device and grant expiry. Before any room directory is shown, the
browser verifies the same-origin server challenge against the selected central
record's server ID, public key and fingerprint. Redirects, userinfo, non-HTTPS public
origins, substituted keys and stale generations are rejected.

The host publishes its ready public ingress as a renewable central endpoint lease
and publishes offline on owned shutdown. Endpoint generation is durable and strictly
monotonic across restarts. Publication uses bounded requests and one cancellation-
owned renewal task; failures remain visible and retry only while the runtime and the
same ingress generation remain active. No central session token is stored by the
host. An unavailable or unpublished ingress leaves the server visible but unopened.

Grant redemption exposes a read-only room-directory projection and can mint one
short operator session for an explicitly selected room. That session is bound to the
requesting browser device, verified public origin and exact room incarnation, and
cannot outlive the central grant. It reuses the existing room-session authorization,
revocation and WebSocket owners without granting native lifecycle, profile, provider
credential or cross-room authority. Failure before the durable room-session commit
returns no credential; a committed response may be retried only with the same grant,
device and room while both grant and room remain current.

Acceptance requires owner-only issuance; host-signature, origin, generation, expiry,
logout and ownership-transfer rejection; durable monotonic endpoint publication;
wrong-key server-challenge rejection; directory read followed by one selected-room
session; and unchanged local-room data on both machines. Verify Worker and Rust
boundary tests, frontend startup/navigation tests, mandatory gates, a signed packaged
Mac host plus Windows client flow, and a standard diff security scan. Do not use Deep
Scan. Preserve the already-running Windows room and its owned children throughout.

### Desktop client-first entry (2026-10-02)

Account startup validates the central session and lists existing owned servers
before starting any local room runtime. Online servers use the existing bound
owner grant; offline servers remain visible with refresh and never cause implicit
local hosting. The native chooser reads only the public installation ID through a bounded,
read-only sidecar command, without creating/migrating data or starting any room
runtime. It matches registered rows by that ID, labels the matching row "이 기기",
and opens it locally only on explicit selection. An unregistered installation
appears as a local row in the same list; no separate bottom hosting button remains.
Inspection failure is shown explicitly and disables local opening; it must not
block validated remote-server entry. Refresh retries the read. The browser never
infers a local host from a name, OS or endpoint. Selecting the
local row starts or initializes this installation's existing authority,
preserves its rooms/profile, and registers it before entering when central is
available. During a classified temporary central outage, the step-1 contract above
permits only existing local authority and defers registration. Local-only builds
retain their existing local entry. No account/profile migration is part of this slice.

Native Google login uses the bundled sidecar in an authentication-only mode with
an ephemeral loopback callback and the existing private start/poll/cancel protocol.
It must not open a database, construct room/provider state, publish an endpoint or
accept any room control. The existing state-bound callback and central PKCE exchange
remain authoritative. The desktop owns the separate child, starts it only for login,
and joins it after cancellation/completion or app exit. The child also expires after
ten minutes or parent-pipe loss; a bounded completion-page drain may precede exit.
No bearer/code is written to disk or diagnostic logs.

Acceptance: cold startup, restored login, guest creation/recovery and Google login
reach the chooser without room bootstrap/registration. Remote choice never touches
local authority, including offline and rejected grants. Explicit local choice alone
preserves/initializes local data and registers the host. Verify callback-only TCP
and private-control rejection, lifecycle cleanup, frontend regression tests,
mandatory gates and an isolated signed package. Windows behavior needs separate
real-device verification. No new scan, reviewer session or subagent is authorized.

### Browser central Google entry (2026-10-02)

Browser startup without an invitation links to the fixed central identity origin.
That origin serves the same frontend's account entry and owned-server chooser;
room hosts never receive central bearers, signing keys or Google credentials.
The shared Google button opens standard authorization-code OAuth with PKCE and
same-tab state. A ten-minute, one-use Worker handoff binds the device public key,
exact central-root callback and browser-held verifier before authentication. The
Worker exchanges the code using its existing Web client secret and checks Google
signature/audience/nonce and current account/device state through the same handoff,
identity and session owners as desktop login. Only exact same-origin web requests may use
this flow. Desktop loopback OAuth and invitation admission remain unchanged.

The browser validates the current central session before listing servers and uses
existing owner connect grants, host challenge verification and room session minting.
Offline servers remain visible but disabled; bookmarks require their existing
invitation authority. Web startup cannot initialize a host, claim ownership or
create rooms. Expired/revoked sessions, authorization failure and cancellation expose
retryable UI. Logout revokes the central session and clears its local account slot.
No schema migration, fallback, widened CORS or account merging is introduced.

Acceptance: same Google subject resolves the existing desktop person; bad audience,
nonce/verifier, replay and foreign-origin completion fail without issuing sessions;
the real private-window flow lists existing servers and opens an online owned room.
Verify Worker HTTP/durable-state tests, frontend startup/session/navigation tests,
production build and mandatory structure gates, then real Google/private-browser
entry. Record unavailable host or Google-provider dependencies as unverified.

### App and web behavior correction (2026-10-02)

User requirement: use the existing product in both the app and browser. The browser
entry implementation above proves only a subset of this requirement. Sharing the
room view while replacing startup and account management is not full acceptance.
This correction supersedes any interpretation that permits separate product flows
merely because the entry point is a browser. Shared startup and account components
are implemented; full behavioral verification remains open.

Reuse product UI and state transitions across entry points. Isolate native process,
credential custody and browser-origin transport at their existing authority owners.
Never move central credentials to a room host or grant native/operator capabilities
to a browser to make the UI appear equivalent. Existing invitation and server-local
account contracts remain reachable where applicable; a central-account user must
not silently receive a different account-management flow based on environment.

| Required flow | Current evidence / correction status | Acceptance evidence required |
| --- | --- | --- |
| Startup, Google login, cancel and retry | Shared startup component; web-only Google widget removed | Shared product presentation and outcomes in packaged app and browser; actual Google return, cancellation and retry |
| Guest start and recovery | Both entries now expose the same guest/recovery controls | Preserve applicable guest/recovery paths and authority; verify successful, rejected and interrupted recovery in both entries |
| Account settings and logout | Common central account section; explicit server-local binding retained | Correct account identity and management in both; logout/reload, revocation failure and retry preserve rooms/profile and other devices |
| Profile name/photo and edits | Cross-entry behavior not yet verified | Same authorized profile on the same server, persisted edits after reload/re-entry, explicit account vs server-profile distinction |
| Server listing, naming, selection and room history | Shared server-list component; full flow unverified | Same owned servers and stored history; online/offline, rename, expired grant and retry behavior |
| Room permissions and local-device actions | Environment branches require audit | Same room role yields same authorized room operations; local host/provider operations retain their actual device owner |

Real-flow findings on 2026-10-02: both Chrome and the signed macOS package complete
Google login as the existing person and render the shared server chooser. Remote
owner navigation previously targeted private `/app` and returned 403; it now uses
the existing public `/pair` shell. The private route remains denied. Chrome reaches
the existing room and history, but the paired-session presentation still suppresses
owner profile settings. This remains an open product defect.

The central-owner correction must retain server-issued provenance in its durable
session. Only a session minted after validated central owner redemption may read
or edit the existing server-wide owner profile and avatar. Revalidate its exact
room incarnation, origin, device, revocation and expiry in the profile transaction.
Ordinary device pairings and existing rows remain room-only; a client flag cannot
grant profile authority. This does not grant host filesystem/provider credential
access, change central identity ownership, or merge accounts. Preserve edited
profiles, revision conflicts, avatar quotas and existing profile projection events.

Central-owner provenance is now implemented as an additive schema 72 column with
existing sessions defaulting to false. Packaged startup preserved the exact room,
history and profile rows through migration. Real Chrome reads/edits the same profile
and retains it after reload; Mac room projections update. A further shared defect
is confirmed: UserPanel only hydrates on identity change and ignores committed
profile revisions. Every changed profile revision now publishes through the existing participant
projection event; only its existing public fields and revision are exposed. UserPanel
fetches the authenticated profile when its revision lags. An open editor retains its
draft and save revision; the existing conflict path remains authoritative. There is
no polling or browser-owned replacement profile. The signed macOS package and Chrome now verify both directions without reload,
including the bottom user panel, room member and timeline projections. The original
name is restored. This closes the observed profile defect, not the other parity rows.

Use affected existing frontend/API tests for regression coverage and exercise the
same scenarios through packaged and browser entry. Record each result and platform
in `docs/VERIFICATION.md`; unavailable authentication/device dependencies stay
unverified. No full-completion claim while required rows lack evidence or contain
unapproved differences. A discovered shared cause expands this audit to its other
affected flows; these rows are a minimum, not a ceiling on investigation.

### Remote owner workspace completion (2026-10-02)

Its shared product/transport requirements remain current. The per-request central
redemption, bounded workspace custody and expiry correction described below are
historical and superseded by Host-owned owner workspace and devices.

Live directory correction (2026-10-03): retain Claude's current shared UI. Native
and central-owner web workspaces must receive committed room creation, settings,
archive/restore/close, pending/completed deletion and cleanup changes without
restart or manual refresh, including an empty workspace and changes to another
room. Reconnection must reconcile changes missed while disconnected. Ordinary
room invitations and device pairings do not acquire a server directory stream.

The persistence owner publishes a coalesced invalidation only after commit; it is
not another directory or authority. A separate authenticated directory connection
is necessary because an empty workspace has no room socket. Native entry consumes
its existing one-use operator ticket; web entry validates its existing central
grant, exact origin, generation and device custody before opening and before each
invalidation. Expiry/shutdown ends the connection. Notifications contain no room
data or credentials; both clients read the existing authoritative directory API.
Use existing connection budgets, bounded reconnect and visible failure. No polling
or substitute authorization. Preserve foreground create/lifecycle continuity and
reject stale asynchronous reads across workspace/authority changes.
Preserve canonical empty metadata: an empty room topic remains empty after a
directory reconciliation and must not be replaced by its name.

Acceptance: two clients see creation and renaming without reload; archive/restore,
close and deletion update the rail/management list; an initially empty client sees
the first room. A reconnect reconciles missed commits. Wrong origin/device,
ordinary guest/pairing, expired/revoked grant and stale generation receive no
directory notifications. Validate affected existing API/concurrency checks and the
signed isolated package; record browser/Windows evidence separately.

The same server owner must reach the same saved friends, room directory/create,
room switching and invitation management from native and web entry, including an
empty server. Ordinary human invitations and operator device pairings remain
room-scoped. Native process, filesystem, provider credentials and public-ingress
lifecycle stay on their existing local-device authority; remote room actions do
not acquire native IPC. The published 0.1.4 artifacts remain immutable.

Reuse verified central-owner provenance and existing domain transactions. Every
remote read/write must revalidate current origin, device, expiry, revocation and
owner provenance before reading private data or committing changes. Keep existing
revision conflicts, idempotent request outcomes, deletion tombstones, quotas and
room-generation binding. Shared UI receives explicit transport authority; hiding
controls is never authorization, and a remote failure never retries with local
operator privileges. Credentials stay out of projections, logs and URLs.

Directory/create and room admission reuse the existing five-minute central grant.
The host redeems it against the configured central authority for each request;
storage binds its fingerprint to the first browser device, origin, endpoint
generation and exact expiry in the transaction. Schema 73 adds this bounded
custody table without changing existing room/profile/session rows. New grants
clean expired custody; at most 128 live grants are retained. No bearer is stored
in that table, and no polling or extra process is introduced. The browser keeps
only this bounded grant in tab-scoped storage; reload rechecks host proof and
central authority. Each room admission has a distinct idempotent session bound
to its canonical room UID, with no lifetime extension.

Real-flow correction (2026-10-03): when that exact lifetime ends, unmount the
remote workspace and use the existing connection gate to explain expiry and offer
the central server list. Do not leave the owner in an invitation-guest composer or
retry an already expired grant. A single deadline and foreground checks derive
from the same expiry; they neither refresh authority nor replace server checks.

Connect these owners in buildable slices, then verify the whole flow: native and
web observe the same stored records; ordinary pairing, wrong device/origin,
expired/revoked authority and stale room incarnations are rejected without writes.
Exercise empty/nonempty room lists, create/switch/reload, invite create/revoke,
friend save/conflict/delete, and visible failure/retry in the shared UI. Reuse the
existing boundary/UI tests and packaged/browser verification; no new automated
security scan or separate regression harness is authorized by this correction.

### Shared invitation transport correction (2026-10-03)

Required behavior: the room header, room menu and room settings open the same
Claude-edited invitation dialog in the native and central-owner web workspace.
People links retain scope, name, use limit, expiry, copy and revocation; connector
and saved AI friend invitations retain their existing request-ID replay and entry
instructions; device links retain room binding, expiry, copy and revocation.
Selecting another room must admit that exact room before using its invitation
session. Closing or changing authority retires pending results without hiding an
uncertain dispatched write or retrying it with local privileges.

The existing room-manager storage owner revalidates native manager provenance or
an admitted central-owner operator session inside each invitation transaction.
Browser routes require the current same-origin, device-bound, unexpired central
owner session. Human guests and ordinary device pairings are denied these owner
entry points. Pairing links minted here remain ordinary room-only pairings; they
never inherit central-owner or native process authority. Existing private purpose
routes and one-use tickets remain private. The browser reads only the active public
invitation origin; it receives no tunnel controls, private host diagnostics or native
IPC. Native ingress start/stop and local-only AI reach retain their actual host
capability. Remote failure is visible; no fallback transport is selected.

Observed shared-flow requirements: the connector card uses the verified public
invitation origin in both transports, including MCP setup instructions. Manual
provider identifiers must not be changed by OS spelling/capitalization correction.
Owner profile reads wait for actual profile authority; admission-in-progress must
not send an unauthenticated private profile request. Empty-workspace profile and
friend access remains part of the broader account contract. Existing per-device
dock ordering is retained by the same merge owner in both transports.

Acceptance uses existing manager boundary and shared-dialog checks plus the signed
isolated app and current Chrome review: issue/copy/revoke people and device links,
create connector and saved-AI instructions without running a provider, change rooms,
and reject wrong device/origin, expired/revoked sessions and stale room generations
without writes. Retain Claude's layout. This closes invitation transport only;
remaining app/web acceptance and the user's duplicate macOS-server diagnosis follow.

### Server-wide account access without room admission (2026-10-03)

Required behavior: an authenticated server owner sees the same profile settings and
saved friends when no room is open, while a room is being admitted, and after the
last room is archived. These server-wide records must not require a fabricated
room, membership or operator pairing. App and browser keep the shared settings and
friends composition; the account transport is distinct from room-only authority.

Reuse the existing centrally redeemed, five-minute, device/origin/generation-bound
server-owner grant. Profile reads, revisioned writes and bounded avatar uploads,
and saved-friend reads/writes, revalidate that owner in the same storage transaction.
The host redeems against its configured central authority for each grant request.
Explicit grant credential dispatch must reject invalid/expired/revoked custody,
wrong device/origin/generation and ordinary room credentials without native fallback.
No schema migration, extra token, timer, process or privilege inheritance is needed.
Actual guest and paired room capabilities remain unchanged. Room invitations still
require their exact admitted room session. Cancelled editors retain existing behavior;
profile revisions and mutation event publication keep their existing owner.

Use existing account/profile/friend and central-owner boundary checks; exercise the
signed Mac and existing Chrome review after product-UI cleanup of this run's own
regenerable test rooms, account/friend read/edit and first-room creation. The current
UI has no archive/restore entry point; do not fabricate one for verification.
No production data deletion or additional server registration is authorized here.

The real empty-workspace check exposed stale cross-client profile presentation:
profile storage committed, but only room participant events advertised its revision.
The existing authenticated owner-directory stream must also invalidate on committed
owner-profile changes. Its canonical directory read includes the existing host
profile revision from the same owner transaction, even with zero rooms. Shared
clients use that server revision for the existing UserPanel refresh; an open draft
keeps its save revision and conflict behavior. No new stream, polling, local revision
authority or expanded guest access. The extra read is one profile in the existing
directory transaction; room-only profile projection events are preserved.


### Local Google account binding and guest retirement

On an already bootstrapped room server, the retained public Google flow accepts a verified ID token with a short-lived,
one-use nonce bound to the current server identity and presented browser credential.
A local operator ticket, an exact admitted human plus its browser credential, or a
standalone durable browser credential are distinct inputs. Pairing is never an
account credential. Bare browser credentials may start unbound login but cannot
read another identity; only verified Google proof can create or recover that link.
The persistence owner resolves identity and revalidates it when committing a link.

One Google subject links to one profile; one profile has at most one Google link.
Multiple devices may explicitly authenticate to that account. The clean schema
therefore removes the old one-device-per-profile constraint while preserving the
exact credential/profile foreign key on reusable human sessions. Earlier schema
versions remain rejected without automatic conversion or deletion.

A switch to a different already-linked account requires explicit guest discard,
a device still bound to that guest, and a guest with no linked account or owned
room. One transaction leaves its memberships, revokes access, retires mutable guest
profile/device data and binds the requesting device to the destination. Room history
remains. Committed revocations/events use the existing runtime publication owner;
no frontend cleanup substitutes for the transaction. One-use admission identity
is not silently promoted by normal room joining; explicit Google linking is the
account operation which may bind that current identity to the device.

Google proof uses `jsonwebtoken` 11 with AWS-LC RS256 verification, required issuer,
audience, expiry and subject, plus identity-bound nonce and issued-at checks. Only
`https://www.googleapis.com/oauth2/v3/certs` supplies public keys; redirects are
rejected, response size is capped at 64 KiB, and the request has an 8-second bound.
`http-cache-semantics` owns freshness, including Age and Cache-Control. No heuristic,
stale or immutable fallback is enabled; retention is capped at one hour. Unknown
key IDs and unavailable/expired keys share one refresh attempt per minute, recorded
before network I/O regardless of its outcome. During cooldown, only still-fresh
matching keys can be used; other requests fail unavailable without stale-key use.
This prevents an upstream outage or attacker-controlled key IDs from serializing
one external timeout per public connection.
The challenge owner retains at most 512 entries, at most 64 unbound identities,
with five-minute expiry checked on access and one pending challenge per subject.
Invalid proof does not consume a legitimate challenge; successful proof consumes it
before account persistence revalidates the current identity. Shutdown drops all
in-memory state. Tests use newly generated RSA keys, never real Google credentials.

#### C1 member binding의 guest retirement 차단과 후속 전환

파일 지도: `crates/agentsassemble-persistence/src/account_guest_retirement.rs` (`retire_guest`), `google_accounts.rs`, `account_identity.rs` (같은 디렉터리); 진입은 기존 local Google guest-discard다. 중앙 member의 공통 모델/예산/floor/C2–C6는 위 C1 절을 따른다.

첫 슬라이스에서 retirement 소유자는 **DB 변경 전 같은 트랜잭션에서 binding 존재를 검사**하여 bound guest-discard를 무변경 거절한다. FK cascade로 membership/미확정 결과를 지우거나 다른 local user로 옮기지 않는다. 기존 binding 없는 익명 guest retirement는 보존한다. 익명 병합은 제외되며 후속 기존 사람 연결도 정확한 human session·저장 browser credential·기존 recovery 소유 증명 없이 허용하지 않는다.

후속 guest 전환은 server leave·전체 parent/child/ticket 폐기·중앙 종료 ACK 후에만 수행한다. binding 이동 없이 identity/revision tombstone을 유지하고 ACK가 불확실하면 완료하지 않는다. 필수 검증은 bound discard의 모든 DB 상태 불변, 미확정 결과/FK 보존, 익명 정상 흐름, 후속 전체 폐기와 ACK 불확실성의 완료 차단이다.

### Account presentation boundary

The local HTTP account flow is connected to the settings account section. Native
operator access stays on exact local ticket transport; public browser/session proof
cannot recover the local operator. Only configured browser servers admit the fixed
GIS script/style/frame/connect sources. Google proof remains transient until a
separate focused guest-discard confirmation; failed proof/persistence requests never
produce a connected view. The native startup central account remains separate.

### Saved friend directory boundary

UI preservation correction (2026-10-03): the pre-cutover reachable friends surface
has a home/category sidebar, online/all/add tabs, searchable contact rows and a
selected-contact profile panel. Removal at `daadd8d4` and the simplified replacement
at `d286225a` did not preserve that presentation. Restore this composition in the
shared app/web view using the current durable directory owner. Preserve revisioned
editing, confirmed deletion, retained failed drafts and explicit remote authority.
The profile displays stored metadata, not inferred live presence or admission.
Do not restore producerless candidate/DM code as a substitute for server contracts.
Acceptance compares the existing layout source with the packaged view and checks
category/search/selection plus existing save/cancel/delete flows. Account controls
remain the shared UserPanel; room channels and the room roster do not occupy the
friends surface. Browser functional checks alone do not establish visual fidelity.

The retained `App.tsx` home/friends entry and room invite picker consume the original
server-wide `/api/room-friends` address book. Its route supplies no live agent list;
saved metadata does not establish presence. Rust keeps this local operator resource
behind a private HTTP operator ticket and rechecks completed bootstrap inside each
storage transaction. Public guests and paired room operators cannot read or edit it.

The persistence owner stores stable UUID contact IDs, revisions, supplied name,
handle, participant type, provider and connection identity, source agent/room and
timestamps. Provider text never changes participant type. Creation uses a client
UUID retained across retries, while edits require the observed revision. Identical
replays return the committed contact; a stale differing edit fails visibly. Delete
is idempotent; it erases contact metadata and retains only the ID to prevent a delayed
creation retry from resurrecting it. A later edit cannot recreate a deleted record. Old JSON files and
schema versions are not silently imported. A failed read remains an error.

The home entry presents searchable/type-filtered contacts, add/edit drafts and a
focused deletion confirmation. The room invite picker uses the existing human
invite owner for people; external AI invitations remain separately owned by Phase 7.
Local storage/restart and conflicting edit checks precede private HTTP/UI connection;
packaged desktop/mobile verification completes the vertical flow before phase review.

The private route is now registered with the shared product/ingress inventory and
uses one-use server-operator tickets. The room rail opens the saved directory with
search, type and last-saved online filters. Unread/failed initial data cannot enable
mutations; failed saves retain drafts and their creation IDs. The human invite picker
lists only human contacts and passes the selected display name to the existing
managed invitation owner, without treating the saved contact as identity proof.
AI contacts remain outside human admission. No directory polling or persistent
browser copy is introduced. Packaged acceptance remains pending at phase closure.

### Room mutation session provenance

Remote pairing must retain session provenance through queued operator commands. A
wire `AuthenticatedPrincipal` is only a public projection and cannot carry a secret
fingerprint or substitute for durable session authorization. Persistence mutation
entry points therefore take explicit trusted-principal, human-session or paired-session authority.
The existing human-session owner revalidates the latter inside the command transaction,
before replay or mutation; room capability checks remain at the mutation owner.
The local command path retains its existing transport authorization. No public
credential may be converted into that path. This shared input is connected in
independently buildable groups before pairing enables any privileged remote command.

Resident pause/resume and busy-turn interrupt carry this provenance through runtime
proof and into acceptance. Once interrupt acceptance durably creates its exact
provider effect, the existing effect/recovery owner completes it independently of
the requesting session. Revocation prevents new acceptance and replay access; it
must not strand cleanup already authorized by a committed command.

Agent start/resume/re-add preparation and the transition to `EffectInflight` also
carry the request session. The latter is the provider-start authorization point;
subsequent exact receipt/failure/recovery belongs to the durable lifecycle operation,
not a fresh browser request. Before pairing dispatch is enabled, revocation before
that point must terminate its prepared intent through the existing failure owner.

### Operator pairing persistence boundary

The clean schema stores one pairing grant and its optional consumed session in one
`operator_pairings` row. Creation revalidates the exact local manager; redemption
serializes device selection in a write transaction. The grant expires in 120 seconds,
and native-issued consumed sessions have no fixed one-hour expiry. They expire
after 30 days without authenticated use, measured by durable last-use time.
Central-owner-derived pairings retain their one-hour and parent-revocation rules.
A still-live same-device retry returns the same bearer even after grant expiry; another device, revocation,
expired session, changed room incarnation or changed host lineage cannot redeem it.
The ordinary human bearer remains unchanged; a distinct operator prefix and HMAC
context use the same existing derivation mechanism. Only fingerprints are persisted.

The record cap is 128 per server and 32 per room. Creation removes expired records
before checking capacity; revoked consumed records cannot be redeemed again.
Cleanup follows fixed or idle expiry and never revives a consumed grant. There is no
background task. Current manager resolution
loads the membership once and shares its bootstrap/profile proof with the principal
projection. Queued room mutations accept persistence-issued paired provenance and
revalidate it in their transaction. Public HTTP and socket admission are connected
below; frontend management and packaged acceptance remain required.

Stopped creation, stopped-profile selection/configuration, create/start inspection,
preparation and pre-provider approval now preserve the same request provenance.
Creation revalidates both its replay snapshot and its commit after filesystem
selection validation. Filesystem checks remain outside write transactions; server
selection is preceded by the guarded inspection/candidate owner. The existing agent
control capability check is shared across these mutation owners. Post-effect
completion/failure retains the exact durable operation rather than a new request.


Definitive session-revoked or permission-denied refusal before provider authorization
now cancels the exact prepared start/create-start/stop through the lifecycle failure
owner. Start reservations are released first; committed errors and state events use
the existing publication path. Refused stops preserve the existing live runtime and
turn state. Effect-inflight work cannot enter this cancellation path; uncertain
storage/authorization results remain unresolved for their existing recovery owner.

Room closure, archive and deletion revoke both unused pairing grants and consumed
sessions in the existing room-access transaction. Consumed fingerprints join the
existing post-commit revocation publication. Restoring an archived room cannot
restore either grant redemption or a previously issued paired session.
Lifecycle and deletion mutations resolve explicit request provenance in the same
transaction before local-manager checks or replay. Trusted native ownership still
supports closed/archived rooms and retained deletion receipts; remote sessions
cannot access those paths after revocation.

The room queue now retains one explicit human/operator session enum, with public
principal fields remaining only a projection. Ordinary human sessions retain their
finite conversation-only dispatch. Paired operator commands reach the existing
operator mutation owners with session provenance. Session message, edit/delete and
random mutations revalidate inside their transaction; the native deletion receipt
shortcut is unavailable to room sessions.

Paired departure revokes only that session. It preserves the host membership and
other paired devices, records an `operator_session_ended` event without device or
credential data, and publishes the exact revocation through the existing channel.
HTTP/socket admission retains the exact session variant; internal runtime proof
is not packaged pairing acceptance.

The public socket grant now retains `RoomSessionAuthorization` through one-use
consumption, subscription, command dispatch and outbound revalidation. Human and
paired variants share the existing bounded grant partition, absolute session expiry
and exact revocation stream; neither enters the local observer branch. Vote-summary
reads resolve the same session authority inside their read transaction. Socket
integration proves paired departure acknowledges once, preserves native host membership,
and prevents a previously issued ticket from reviving the ended session.

### Operator pairing HTTP boundary

The native host creates/revokes pairings using the existing one-use server-operator
HTTP credential, then resolves the exact requested server, authority lineage, room
and room UID at the local manager owner. Creation requires ready public ingress and
returns a 32-byte CSPRNG grant in a `/pair` URL; only its fingerprint is stored.
The public redemption route requires that ready canonical HTTPS Origin and the
canonical browser device credential. Same-device retries retain the durable bearer.
The `/pair` entry and assets are now same-origin public; creation/revocation stay private.

Socket-ticket exchange and departure dispatch by credential prefix without retrying
another authority domain. Paired credentials require current device and the ready public origin;
ordinary human admission remains with its existing owner. Revocation commits before
notifying existing room subscribers, without creating another task or timer. Paired
sessions cannot obtain account or native server authority. Paired operator controls and packaged phase acceptance remain required.

### Browser room-session device propagation

The canonical browser room connection passes the existing browser device identity
through its socket-ticket exchange. Its accepted projection and transport scope
include that device identity; a device change retires the old connection and late
callbacks. The explicit HTTP leave helper also carries the caller's device identity.
Human sessions retain their existing optional-device exchange contract; paired
sessions remain device-required at the server authority owner. No device is inferred
from the session bearer and no additional browser storage or periodic work is added.

### Paired room HTTP resources

Room preferences, lobby search/context and pins, message-attachment upload/read, and
bound room-appearance reads retain `RoomSessionAuthorization` into their existing
storage transactions. Human and operator variants resolve at their original owners;
room capabilities and attachment/message reachability checks remain in place.
Account profile and profile-avatar mutation continue to accept only their prior
human-account or native authority, independently of room operator capabilities.

The existing ingress owner now passes its verified public origin as request
provenance. Session HTTP authorization requires that origin to match the current
ready ingress before resolving the stored device-bound bearer. This supports browser
GETs without an Origin header, without trusting a caller-supplied origin as a proxy
proof. Existing proxy, Host, forwarded HTTPS and optional-Origin checks are unchanged;
redemption still requires the explicit matching Origin header. No routes or ingress
acceptance rules are broadened, and no retry, task or polling owner is introduced.

Browser room HTTP callers now carry the existing device identity through preferences,
search/context, pins, message attachment upload/read and room-appearance reads.
The existing search/pin/appearance projection and attachment-operation lifetimes
include the device binding, so device changes retire prior requests and installed
resource URLs through their current owners. No additional browser credential store,
request retry or interval is introduced.

### Native operator pairing management

The local invite modal now offers a separate own-device connection card. Creation
uses the existing private server-operator HTTP transport, resolves the current exact
manager after ready-ingress refresh, and guards dispatch again after ticket issuance.
A confirmed response arriving after modal closure is retained for revocation, with
copy disabled. Clipboard dispatch rechecks the current origin, manager, link expiry
and grant state. The shared app/web own-device tab also renders a browser-local
QR of the exact link, only while these same presentation checks permit copying.
No external QR service receives the link; expiry, retirement and revocation remove
it. An unknown revocation stays non-copyable and retries the same recorded grant rather than minting another.

A single nearest-expiry UI deadline disables expired links and is cancelled on
owner unmount. It neither polls ingress nor infers that the grant was redeemed.
Expired links retain the separate revocation action because a consumed session can
outlive its link. Actual paired operator controls and packaged desktop/mobile flows
remain part of whole-phase acceptance.

### Paired room management presentation

The active canonical snapshot owns room-management and agent-control presentation.
Remote routing remains session-bound; its browser session never becomes native
manager authority. Desktop and mobile settings/agent-creation entries use the
current room capabilities. A retired projection removes those entries and dismisses
remote settings/agent creation. Existing WebSocket command and persistence owners
remain responsible for committing authorization. Native server directory, friends,
and invitation creation retain their separate host boundary.

This presentation adds no credential, storage, timer or retry owner. Room/agent
image upload and remote lifecycle HTTP controls still require their complete owner
connection, followed by packaged desktop/mobile verification before phase closure.

### Paired room-owned image writes

Existing agent-avatar and room-appearance storage accepts explicit local-manager
or paired-operator provenance. The same committing transaction revalidates the
exact local manager or durable paired session before modifying bounded pending
asset custody. Human sessions cannot upload agent avatars or room appearance;
paired sessions cannot select the account-profile upload purpose. Existing raster
normalization, storage limits, expiry, replacement and binding owners are unchanged.
No pending-image public read, native credential conversion or new route is needed.

Both browser upload callers carry the actual paired bearer and device. Agent avatar
binding uses the existing projection-bound profile command. A room-image upload
retired by a device/session change cannot bind its result. Remote room appearance
is presented after the canonical settings commit, when the image is bound and its
existing remote read is authorized. Local pending-preview behavior is preserved.
No new background task, retry, timer or persistent state is added. Remote lifecycle
HTTP controls and packaged whole-phase acceptance remain outstanding.

### Paired lifecycle HTTP entry

`/api/room-session/lifecycle` is a same-origin public entry accepting only a
verified paired session. It checks the requested server/lineage and exact session
room, then passes session provenance to the existing queued lifecycle/deletion
owner. The native `/api/rooms/lifecycle` route remains private. Both share their
existing action whitelist, payload contract, response projection and failure owner;
ordinary human sessions cannot use the paired entry.

Close/archive replies can confirm their committed room state even though that
commit revokes the requesting session. Deletion acknowledges its durable pending
intent through the existing unresolved result and finishes via the existing runtime
cleanup/publication owner. It requires no surviving browser session. A paired
caller cannot replay after revocation; deletion completion and a lost terminal
response must be checked at the native host. No new completion credential, polling,
background worker or native authority conversion is introduced.

### Canonical room identity projection

The browser socket snapshot now uses the generated `Room` type instead of the old
directory/unknown-object union. Its room record and lifecycle events share the same
exact generated-key parser. The accepted socket projection retains that verified
record alongside its existing scope and display origin, and clears it with the
projection on device/session/room retirement. Paired lifecycle presentation consumes
this record; stored browser flags cannot supply room state or authority.

### Paired lifecycle presentation

The existing room-management dialog now has a separate paired controller for the
one canonical active room. It shares presentation and strict result decoding with
the native controller, without accessing the native directory. The advertised
session route and current room-management capability enable entry. Confirmation
retains the room incarnation and request ID; uncertain retries cannot retarget it.

An owned terminal response can update the originating dialog after its own session
expires. Device or identity replacement and owner unmount retire that response.
A confirmed close/archive or accepted deletion disables further mutation immediately,
even before the socket retirement arrives. Deletion acceptance remains distinct from
completion, directing the user to the native host after paired access ends. Manual
refresh uses the existing socket resync; no polling or new background owner is added.

### Paired account presentation

The paired room client presents its admission identity without requesting the durable
account profile. Its existing stored pairing label only restricts presentation; current
server capabilities continue to authorize room controls. The profile explains that
account edits belong to the native host. Session and device credentials stay intact;
ordinary human profile hydration and editing remain on their existing authority.
Switching presentation retires queued profile work through the existing generation
owner. The focused UserPanel suite passes 10 cases, and TypeScript/build, original CSS,
architecture/source policy, formatting and artifact checks pass. Packaged acceptance
remains part of the phase exit. No new task, timer or persistent state is introduced.

### Local guest recovery completion

The retained recovery settings and URL-entry panel currently call two absent local
HTTP routes. Complete those routes at the human identity/session persistence owner.
A live human session may rotate its identity's one-use recovery code. Redemption
requires the current ready HTTPS origin, a canonical new-device credential and an
active existing human membership in the requested room. Paired/native operator
identities cannot mint this durable browser authority. Device conflicts, unavailable
membership and invalid/used codes fail without consuming the code.

One transaction binds the new device, retires the prior room session, issues a human
session with the membership's latest admitted scope, and rotates the recovery code.
Recovery is an explicit human-session provenance with a durable device binding;
it has no fabricated invitation, invitation request ID or invitation payload hash.
It consumes no invite use and cannot rejoin removed members. Existing room-session revocation publication retires
old sockets. Recovery storage contains one fingerprint per identity and no plaintext
code. A consumed code retains one receipt for the same device, room and client until
the issued session ends; that retry returns the same session and replacement code.
A different device cannot replay it. A subsequent rotation retires the receipt.
No automatic compatibility parser or user-data conversion is introduced. Verify code
rotation/use, wrong device/membership, rollback and session revocation with controlled
local cases; connect and exercise both retained panels in packaged phase acceptance.

Persistence acceptance: five recovery cases pass (80 ms), including durable restart,
one-winner competing devices and injected insert rollback; 23 schema cases (130 ms)
and three account/device cases (30 ms) pass. Clippy passes all persistence targets
and features; unchanged architecture/source policy, formatting and artifact checks
pass. Existing device binding moved intact from Google linking to the shared account
identity owner. Schema 65 adds explicit recovery provenance and rejects older data
without conversion. Storage adds one fixed-size recovery/receipt row per identity,
reuses session capacity/expiry, and starts no background work. HTTP and packaged
acceptance remain outstanding for this vertical flow.

The local HTTP recovery routes now use exact human/device account authority for
issuance and the verified ready HTTPS ingress for redemption. `/recover` and its
assets are the explicit public recovery entry; the native root remains private.
The response carries the existing canonical room/server surface, and the runtime
publishes committed session replacements through its current revocation stream.
HTTP integration passes four cases (80 ms), including read-only scope preservation,
code retry, real entry/assets, wrong device/origin, paired rejection and excess
attempts. The static exposure inventory and unchanged Clippy/structure gates pass.

Recovery retains global/network/code attempt budgets using governor 0.10.4 GCRA:
bursts and per-minute refill rates are 256/16/8. The network key is the accepted
transport peer; clients behind the same proxy share that network budget. No caller
header supplies a separate network identity. Each network/code map holds at most
512 entries, and only fully replenished cells can be reclaimed when admitting a new
key at capacity. Two fake-clock cases verify budget/capacity and reclamation without
sleeping. The library owns replenishment; no timestamp mirror, timer or waiting task
is introduced. Browser credential redirects remain prohibited at the frontend owner.

The required artifact check measured 21,632,364,544 bytes against the unchanged
18 GiB limit. After confirming no Cargo/Tauri builds were active, the existing
`make artifact-prune` owner cleaned only this repository's regenerable Cargo target;
source, user data and other applications were preserved. Frontend recovery URL and
request-lifetime acceptance completes this flow in the following client change.

The recovery client now preserves the server-owned opaque code exactly, including
case, and uses the retained URL-consumption owner to remove it from browser history.
Issue/redeem requests prohibit redirects and caching; strict results confirm issuance
and the exact recovered room/client. Each existing settings/recovery panel retires
its transient results on device, identity or request changes. Duplicate submissions
are blocked, uncertain redemption retains the same input for retry, and an uncertain
code rotation cannot continue displaying the prior code as usable. Five focused
frontend suites pass 18 cases (1.39 s); TypeScript/build and original CSS pass (210 ms
bundle phase). No persistent browser authority or background work was added.
Packaged account/friends/invite/pairing/recovery acceptance remains the phase exit.


#### C1 member의 독립 재입장 credential 차단 (보완 H1)

파일 지도: `crates/agentsassemble-persistence/src/guest_identity_recovery.rs` (`issue_guest_recovery_code`, `redeem_guest_recovery_code`의 최초/재시도 경로), `account_identity.rs` (`AccountAuthority::HumanSession`, `bind_account_device`), `google_accounts.rs`; `crates/agentsassemble-server/src/guest_identity_recovery_web.rs`, `account_web.rs`. 모델/상한/floor/C2–C6는 위 C1 및 invite 세션 절이 소유한다.

첫 슬라이스의 member parent/child는 `AccountAuthority::HumanSession`의 durable-account/recovery 발급 권위가 아니다. 기존 recovery 발급(설계 기준 `guest_identity_recovery.rs:58`), local Google/device binding 등 **독립 재입장 credential 생성은 member 세션에서 거절**한다. 중앙 binding이 있는 local user는 세션 출처와 무관하게 기존 recovery code redeem(설계 기준 같은 파일 177·213의 재시도/신규 경로)과 새 local device binding 생성도 거절한다. Recovery/credential 소유자 내부에서 binding 존재를 트랜잭션으로 검사하며 member 경로에 우회 권위를 만들지 않는다.

Binding 없는 익명 human과 recovery는 기존 계약을 그대로 보존한다. Member recovery는 첫 슬라이스 제외이며 후속 제공 시 credential을 membership revision 및 parent에 결합하고 parent 전체 종료/membership 종료와 함께 폐기해야 한다. 필수 테스트는 member recovery 발급 거절, binding 사용자의 기존 code redeem 거절, 중앙 device/account 폐기 후 recovery/local credential 재입장 거절, binding 없는 익명 recovery 정상 동작이다.

### Current-session startup readiness

Packaged public recovery confirmed issuance, durable device binding and prior-session
revocation, but exposed a client transition defect: clearing the consumed recovery
request removed startup readiness because that projection still used the initial
stored session. The application now derives readiness from the current admitted
session. Pairing uses the same transition when its one-use entrance token clears.
Existing surface verification and session-bound socket authorization are unchanged.
The recovery entrance renders independently until acceptance, so private profile
and native creation controls are not mounted beneath it. Four affected suites pass
37 cases (2.34s); build/CSS and unchanged gates pass. Fresh packaged continuation
verification remains required; no retry, fallback or new authority was added.


### Terminal lifecycle response and retired-access presentation

Real paired archive committed and revoked the requesting session, but its successful
HTTP response lacked the private/no-store contract required by the browser decoder.
The existing directory router now applies the same shared cache-header layer as
other authenticated room resources, including failures. The decoder remains strict.
The existing paired close/archive/delete integration now asserts the actual headers:
it reproduced the missing header before the fix and passes all three paths after it.
No compatibility acceptance or secondary response authority is added.

The existing expired-admission state now renders one explicit Korean access-ended
view instead of a history loader and repeated composer errors. Its exit uses the
existing guest-surface exit owner and remains accessible at 390px. Session expiry,
revocation, stored admission cleanup and recovery semantics are unchanged.


### Paired departure acknowledgement

The existing socket departure owner commits only the paired session's revocation
and emits `operator_session_ended`; it does not mark the native host participant
left. Packaged departure exposed a frontend decoder that accepted only the normal
`participant_left` response. The decoder now accepts the paired discriminant with
the exact expected room, durable event sequence, result participant and human actor.
The socket's existing terminal-leave completion and cleanup remain unchanged.
The original delivered-ACK-before-close test now covers both human and paired wire
results: paired failed before the correction and both succeed afterward. No HTTP
fallback, new retry or inferred successful departure is introduced.

The focused confirmation records whether the selected operation ends a device or
leaves a membership, so expiry cannot rewrite its explanation. Device departure
explicitly preserves the host and agents. Successful exit clears existing guest
session state and goes to the public `/join` entrance, never the private root.


## Local phase acceptance

All implementation and packaged obligations marked pending in the incremental
entries above are locally complete under the configured-provider boundary. Current
evidence and explicit unconfigured-Google/final-provider limits are recorded in
[Phase 5 local closeout](../VERIFICATION.md#phase-5-local-acceptance-and-packaged-closeout-2026-09-08).
No genuine Google authentication was simulated or claimed. Daybreak approved the
whole local phase after both supported Google findings were corrected; see the
[completed review disposition](../VERIFICATION.md#phase-5-whole-phase-review-corrections-2026-09-08).
# Central server names (2026-10-02)

macOS name-source correction: use the System Configuration computer display name,
not the network hostname, which can come from a router's reverse DNS response.
The native host owns this read through `whoami::devicename`; missing/invalid names
remain errors, without a hostname fallback. Other platforms retain their existing
source. The registration schema, owner-only projection, durable custom alias and
identity/room authority do not change. Verify the macOS envelope against `scutil
--get ComputerName`, then re-register the existing signed app host and restart it;
the server ID, room/message data and custom-alias precedence must remain intact.

OS display extension: the private host registration envelope reports `host_os`
(`macos`, `windows`, `linux`, `other`) from the compiled native runtime, never the
viewer's browser. The directory persists this optional display metadata on the
existing signed registration/claim write. Existing rows remain NULL until a host
reports it; an older registration that omits metadata preserves the last report.
Only the current owner receives it in bootstrap, including while offline. Both
choosers display an OS badge; missing information is explicitly unconfirmed.
No OS version, architecture, hostname inference or public server-info change.

The private registration envelope supplies the host OS name as display metadata;
public server-info and identity signatures remain unchanged. Missing/invalid host
names fail registration visibly. `servers.label` is the host default and
`person_servers.alias` is the account-owned override. A one-time migration clears
only the historical automatic owner alias `이 기기` when its host label also matches.
Offline historical hosts cannot reveal their computer name until re-registration;
the chooser retains their short server ID and allows the owner to name them now.

Authenticated POST `/v1/servers/:id/name` stores a trimmed 1–80 UTF-16-unit name,
rejecting controls and invalid types. A single SQL update checks current ownership,
active registration and the displayed name observed by the editor; already-applied
names are idempotent, while stale different-name edits and unavailable/non-owned
registrations fail without mutation. Refreshed entries own the editor baseline.
Only owners receive the current host default; bookmarks show their own alias or
server ID. Unicode Cc characters are rejected. Registration and same-
owner claims preserve explicit aliases. Names do not affect host keys, admission,
endpoints, room authority or profile synchronization. Both desktop and web share
the editor; failed saves retain input. No runtime starts to rename an offline host.

Acceptance: default hostname, durable rename across registration/account reload,
non-owner/revoked/replay/stale edit rejection, legacy-only migration, shared UI
save/failure behavior, packaged user flow, mandatory gates and requested Daybreak
review. This does not claim Windows or live remote-room verification.


## Central server icons, backend only (2026-10-04)

Operational follow-through (2026-10-04): the shared app/web square cropper's actual
512x512 PNG must pass the central owner API; app upload, another device's web list
and removal are required user flows. After approved migration/deployment, the
packaged macOS cropper reproduced `invalid_server_icon`. Correct the incompatible
PNG boundary at its owner while preserving bounded decoding, signed authority,
ownership and observed-reference writes; do not bypass validation or substitute
a client-only icon.

User request: persist a server icon in the central directory and return it in each
CentralServer projection, including another signed-in device. Frontend UI/types and
room appearance are outside this slice. The existing Worker remains the authority;
no local engine or host endpoint is needed to edit an offline registration.

Migration 0008 adds servers.icon (empty for existing rows) and one bounded current
PNG blob per server. POST /v1/servers/:id/icon accepts icon (PNG data URL or empty
string to remove) and expected_icon (the observed list reference). Current ownership,
owner relation, active registration and observed value are checked in the write,
with already-applied writes idempotent as for alias changes. Blob/reference changes
are one atomic D1 batch. Stale/foreign/bookmark writes fail without mutation.
Registration and same-owner claims preserve the icon. No new token, public asset
access, remote image fetch, periodic work or room authority is introduced.

GET /v1/bootstrap returns icon as an empty string or a versioned relative central
image path. GET of that path uses the same device-signed central authentication and
checks current owner/bookmark visibility. It returns image/png with no-store and
nosniff. Shared web headers allow the resulting local blob image. Removed/replaced
references and unrelated accounts cannot read it. Frontend consumers must signed-fetch the reference and display a local blob URL; no bearer or
signature belongs in a URL. Compare exact expected_icon on the next edit.

Upload contract: exactly 512x512 static, noninterlaced, 8-bit RGB/RGBA PNG, at most
1,100,000 decoded-file bytes. Bound the upload stream and decompressed scanlines
before the maintained PNG decoder verifies checksums and pixel structure. Reject
external/SVG/JPEG URLs, animation, malformed/truncated PNG, wrong dimensions,
oversize bodies and compressed expansion.
WebKit's real canvas output includes a 68-byte uncompressed eXIf chunk. Permit
one pre-IDAT eXIf chunk of 8–4096 bytes; the PNG decoder skips this bounded opaque
metadata and verifies its checksum without parsing or inflating it. Compressed
ancillary metadata remains rejected. Icon removal leaves no orphan blob;
registration/account termination hides the icon immediately; bounded dependency
cleanup removes its row before physical parent deletion, without cascade. Server lists contain only the
small image reference, not repeated image data.

Acceptance: real Worker request handler with signature checks and migrated SQLite
proves cross-device lists/image bytes, owner-only set/remove, stale/replay/revocation
rejection, atomic rollback, preservation across registration and cleanup. Local
workerd/D1 verifies runtime/storage compatibility; production deployment and migration
remain separate authorized operations. No frontend or room-icon modifications.

### Native device persistence and QR acceptance (2026-10-05)

Required entry points: native pairing create, public `/pair` redemption and browser
credential/session restoration, device-bound HTTP and WS admission, queued commands,
attendee parent validation, native single/all device revocation, shared invite modal
and device settings. The persistence owner stores last authenticated use; successful
use refreshes it atomically after authority/device/origin validation. Failed
authority/device/origin checks and merely listing devices do not extend it. An open authenticated connection counts
as use when it exchanges authorized traffic. No new polling owner is introduced.
Native sessions survive host restart/browser reopen with the same browser credential;
30 days without use denies replay/admission. Revocation commits before the existing
HTTP/WS/derived-session notifications and cannot be undone by reconnect or retry.
Room incarnation, host lineage and room lifecycle security invalidation still apply.
The 120-second, single-device grant and central parent custody remain unchanged.

Schema upgrades preserve records and metadata, promote only still-live, unrevoked
native sessions, and never revive already expired/revoked authority. Browser-stored
operator deadlines are not admission authority: reconnect presents the retained
credential to the host, including after upgrade from an old one-hour projection.
The host still rejects expired central-derived pairings and revoked/idle native
sessions through the existing admission failure flow. The shared device
list displays macOS/Windows/Linux with proper casing, a device name or browser title,
and OS plus last-use time without duplicate browser/OS titles. The QR uses a maintained
local React library and retains Discord-style settings spacing and existing controls.

Regression acceptance: before/after failures for native persistence beyond one hour,
30-day inactivity and refresh, reopen, migration, device bearer rejection, single/all
revocation and derived denial, unchanged central-parent rules, QR rendering/removal,
and OS/title presentation. During this task run affected crate/screen tests only and
run `make verify` once immediately before feature commits/push. Production deployment,
signed app builds and manual verification are excluded by user direction; automated
evidence does not claim physical camera or packaged visual acceptance.

### Device idle-use correction (Daybreak M1, 2026-10-05)

Bearer resolution, authority revalidation and attendee-parent validation are read-only.
Only completed authorized product operations refresh native device last_connected_at:
HTTP reads/mutations after body, room, permission and capacity checks; socket ticket
exchange after successful issuance; successful authorized WebSocket traffic. Rejected
profiles, malformed ticket metadata, denied permissions, exhausted capacity and
NACK-only traffic must not extend the 30-day deadline. Successful transactional
operations record use in their transaction where available; transport-only success
revalidates the exact device/origin/session before recording. Revocation/expiry cannot
be revived by the success recorder. Coalesce native use writes to at most once per
minute; metadata changes remain explicit. No polling, schema change or fallback.
Regression tests must fail before correction, then cover rejected and successful HTTP,
tickets and frames plus unchanged expiry/revocation. Only affected crate tests during
work, one make verify before one commit/push; no deployment, signed build or manual QA.

### Atomic device activity correction (Daybreak M1/L1, 2026-10-05)

Room-session commands and reads (including committed-result retries) record native
operator use before committing their successful persistence transaction. Message,
channel, settings, participant, agent lifecycle, random/vote, history, provider
response and side-chat entry points retain exact session authority. A failed activity
write must roll back durable command state and its receipt; ACK transmission or
socket cancellation cannot own this write. Preparatory validation alone is not use.
Deferred external effects retain their originating device through completion without
reviving revoked/idle-expired credentials or discarding durable effect custody.
Post-send recording remains for subscription, Pong, catalog and live-event delivery,
not command ACKs. Existing revocation, expiry and failure/retry semantics remain.
Ticket metadata writes occur only for stale activity or actually changed supplied
fields; identical metadata within one minute affects zero rows. Regressions must
fail on the prior code and cover rollback, successful retry/read, and metadata write
counts. One commit/push, affected crate tests, then one make verify; no deployment,
signed build, manual verification, or changes to .agents/ and scripts/__pycache__/.

### 멤버 재입장·초대/동의 화면 보완 (2026-10-05)

진입점: member-challenge/member-join → persistence admission, 공용 app/web
AppOverlays → GuestJoinProfilePanel/MemberJoinPanel 및 로그인/callback/오류 화면.
호스트가 binding/멤버십/세션 provenance를 계속 소유하며 새 사용자·admission·초대 소비 없이
기존 canonical 세션을 재사용한다. 기존 세션 만료·종료 및 challenge/새 초대 검사는 유지한다.
SQLite writer 직렬화와 기존 스키마·API·명시적 재시도를 유지한다.
검증은 폐기/만료된 첫 초대 + 새 같은 범위 초대/다른 브라우저의 동일 사용자·provenance·
소비 불변, 다른 범위 conflict, Left/Kicked 거절을 포함한다.
화면 미감·문구는 관리자(Claude) 지정 그대로 사용한다. 초대 제목은 “‘{방 이름}’에 초대받았어요”,
아바타/이름 유지, “로그인하고 참가”(첫 강조)/“게스트로 참가”(보조),
“로그인하면 다른 기기에서도 같은 사람으로 참가할 수 있어요.” 안내를 표시한다.
중간 화면은 “로그인하고 참가”, “Google로 계속”, “참가를 준비하고 있어요”를 사용한다.
동의 제목은 “{서버 이름} 서버에 참가할까요?”, “계정  {이름}” 한 줄과 작은 origin 한 줄,
“참가하면 이 서버에 내 이름과 프로필이 보여요.”, “참가하기”(강조)/“취소”(보조)를 표시한다.
UUID·ISO 시각·내부 용어·영어 오류는 표시하지 않는다. Left/Kicked는
“이 방에는 다시 참가할 수 없어요. 방 관리자에게 문의해 주세요.”, 다른 범위는
“이 초대로는 참가할 수 없어요. 방 관리자에게 새 초대를 받아 주세요.”로 안내한다.
새 추상화 없이 기존 컴포넌트/보조 스타일 재사용. 관련 crate/화면 테스트 후 푸시 직전
make verify 한 번; 기능별 커밋/푸시와 VERIFICATION 건별 한 줄. 서명·수동 검증·배포 제외.

### 화면 구조 개편 C (2026-10-05)

공용 app/web AppOverlays → GuestJoinProfilePanel과 중앙/앱 MemberJoinPanel의 표시만 변경한다.
초대 전에는 단색 어두운 배경과 가운데 카드만 보이고, 64px 방 아이콘(없으면 이니셜),
초대 제목, 서버 이름 보조 줄, 아바타 사진 선택과 남은 폭 전체 이름 입력, 세로 로그인/게스트
버튼, 작은 안내 순서를 지킨다. 현재 preflight는 방 이름만 제공하므로 서버 이름은 제공될 때만
표시하고 아이콘은 이니셜을 사용한다. 동의는 서버 이니셜 → 굵은 서버 이름과 자연스러운
참가 질문 → 계정 한 줄 → 작은 주소 → 안내 → 전체 폭 참가 → 텍스트 취소 순서다.
서버 admission/권한/상태/실패/재시도/저장/프로토콜과 기존 버튼 활성 조건은 변경하지 않는다.
기존 토큰/스타일만 재사용하며 화면 테스트, 푸시 직전 make verify 한 번으로 검증한다.
서명 빌드/수동 검증/배포는 제외하고 최종 미감은 관리자 소유다.

### 종료된 멤버 세션 후 재입장 보완 (2026-10-05)

0.1.14 실기 재현: Joined와 member_admissions가 남아도 첫 session_key가 ended/expired이면
재입장을 SessionUnavailable로 거절한다. member-join → admission → session_provenance를
수정하여 같은 사용자/admission/범위로 새 요청별 세션을 발급한다. 새 초대는 소비하지 않는다.
신규 요청의 세션 키는 기존 admission seed, browser fingerprint, request UUID에 결합하고
각 세션의 출처는 기존 binding/user/participant/room/scope/input hash 및 키 결합을 검증한다.
기존 첫 세션 출처도 그대로 유효하다. 스키마 변경 없이 단일 active participant 세션 제약을
유지하고 새 세션 발급 시 기존 active 세션을 종료하여 기존 revocation 통지 경로로 전달한다.
동일 browser/request 재시도는 저장된 같은 결과·세션을 반환하며 종료 세션을 되살리지 않는다.
성공한 HTTP challenge는 기존 300초/1024개 한도 안에서 요청 fingerprint와 검증된 신원을
보관하여 정확한 재시도에만 재사용한다. 동시 redeem은 한 번만; 실패·불명 결과는 폐기한다.
새 초대 유효성, exact scope, Left/Kicked 거절, epoch/만료/browser 결합을 계속 검사한다.
Left/Kicked는 member_membership_ended로 구분하며 공용 UI에서만 재참가 불가를 안내한다.
admission_session_unavailable 및 기타 일시 실패는 “참가를 마치지 못했어요. 다시 시도해 주세요.”.
회귀: 만료/종료 뒤 같은·다른 기기 재입장, 새 세션/같은 사용자/출처/소비 불변, 정확 재시도,
동시 요청·교체·rollback, Kicked 및 오류 매핑. 관련 테스트 후 make verify 한 번과 기능 커밋/푸시.
서명 빌드·수동 검증·배포 제외.

### 멤버 다중 기기와 유한 재시도 보관 (2026-10-05, 233019d8 교정)

관리자 요구: Discord처럼 다른 기기는 동시에 유지한다. 위 단일 active participant 규칙을
대체하여 (admission, browser fingerprint)당 active 세션 하나, admission당 최대 8개를 둔다.
같은 기기의 새 요청은 그 기기만 교체하고, 아홉 번째 기기는 마지막 성공한 권위 사용 시각이
가장 오래된 세션을 종료한다. 동률은 admitted_at/admission_key 순서로 결정한다.
member-join → persistence admission → HTTP/WS authority/revalidation 및 기존 revocation 전달이
영향받는 진입점이다. 종료/만료 세션은 최신 32개 tombstone만 보관하므로 admission당 최대
40행이다(재입장 트랜잭션마다 정리; 퇴장/강퇴로 남은 active가 종료되어도 총 40행 이내). 기존 v82는 살아 있는 세션을 보존하며 v83으로 올리고 종료 이력을 같은 상한으로 정리한다.
정확한 재시도는 살아 있는 결과만 반환한다. 종료 tombstone은 거절하며 정리한 challenge의
만료 시각 이하 재시도도 거절한다. 기존 300초 challenge 창 밖은 새 challenge/grant가 필요하다.
유한 보관 이후 새 challenge로 옛 UUID를 제출하는 것은 새 시도지만 challenge에 결합한 새
세션 키를 쓰므로 옛 bearer는 부활하지 않는다. 무기한 UUID 이력은 보관하지 않는다.
동일 범위, Left/Kicked, 현재 초대 유효성, browser/challenge/epoch 결합, provenance, C4a 거절,
초대 소비 불변과 전역/방 세션 한도를 유지한다. 회귀는 두 기기 동시 유지, 같은 기기 교체,
LRU 실제 사용 갱신, 8/32/40 상한, 오래된 재시도 비부활, 동시성 및 원자적 rollback을 검사한다.
이번 검증은 persistence/server 테스트와 푸시 직전 make verify 한 번, 커밋/푸시 하나,
VERIFICATION.md 한 줄로 제한한다. 서명 빌드·수동 검증·배포는 하지 않는다.

## 서버 기본 이름과 방 중심 문구 (2026-10-05)

사용자 승인 범위: 공용 app/web와 호스트. 방은 채팅 공간, 서버는 이를 여는 컴퓨터다.
서버 기본 이름은 `{내 프로필 이름}의 {기기 종류}`이며 macOS Model Name, Windows
제품/폼팩터 이름을 사용하고 알 수 없으면 `컴퓨터`를 쓴다. 호스트명은 쓰지 않는다.
기본 모드에서는 프로필 변경이 중앙 label까지 반영되고, 기존 이름 변경 경로에서
직접 지정한 이름은 고정된다. 기본 이름 복원은 다시 자동 추종한다. 기존 자동 이름만
전환하고 수동 이름은 보존한다. 출처 구분이 불가능한 이름은 보존하고 보고한다.
최초 이 기기 서버 열기, 서버 목록/설정의 기존 편집 UI에 서버 이름·편집·기본 복원과
`초대받은 사람에게는 이렇게 보여요` / `‘{방 이름 예시}’ · {서버 이름}에서 열린 방`
미리보기를 제공한다. 새 추상화나 별도 제품 흐름을 만들지 않는다.

MemberJoinPanel과 useRoomAdmission → memberConnect fragment 경로는 방 이름을
표시용으로만 전달한다. Unicode NFC·공백 정리·제어문자 제거 및 80 UTF-16 단위
제한을 적용하고 React 텍스트로 렌더한다. 서버/주소는 중앙 preview만 사용한다.
표시명은 grant 요청·callback 상관관계·권한 판정에 포함하지 않는다. 제목은
`‘{방 이름}’에 참가할까요?`, 보조 줄은 `{서버 이름}에서 열린 방`이며 기존
계정/주소/안내/참가하기/텍스트 취소 구조를 유지한다. 구버전 초대의 방 이름 누락은
`이 방에 참가할까요?`로 표시하며 권한에는 영향이 없다. 이 변경은 보안 리뷰 대상
독립 커밋이다. Rust persistence/server의 member_admission 세션 로직은 수정하지 않는다.

사용자 화면의 합니다체는 같은 뜻의 해요체로 바꾸되 식별자·로그·개발자 오류는 유지한다.
작업 중 영향 범위 테스트, 푸시 직전 make verify 1회, 기능별 1,000줄 미만 커밋과 푸시,
VERIFICATION.md 건별 한 줄을 남긴다. 서명 빌드·수동 검증·배포는 금지되어 이번 작업의
검증 범위 밖이다. 자동 이름/고정/복원/기존 이름 보존/실패 및 재시도와 공용 진입점의
자동 검증을 실행하고 실제 화면·운영 배포 검증은 미확인으로 보고한다.

구현 소유: 호스트 저장 프로필 revision과 모델이 기본 label을 결정한다. 중앙의 빈
owner alias는 자동 모드이고 nonempty alias는 고정 모드다. 기존 데이터에 alias가 있으면
자동 유래처럼 보여도 보존한다(문자열 비교로 추정하지 않음). 호스트는 기존 directory
작업과 profile/epoch 변경 알림·bounded retry를 재사용하여 `/name`에 AA-HOST-1 서명
PUT을 보내며, 중앙의 additive name_revision으로 지연된 옛 프로필 쓰기를 거부한다.
POST 이름 편집은 동일 소유권·epoch·관측 이름 검사에서 reset_default를 처리한다.
새 루프/서비스/인증은 추가하지 않는다. 프로필 변경은 호스트에 먼저 영속화되어 중앙
일시 장애나 재시작 후에도 최신 revision을 다시 보낼 수 있고 name_sync_error로 실패를
노출한다. 표시 이름은 권한에 쓰지 않는다. 새 호스트 기본명은 400 UTF-16 단위까지,
수동 별칭은 기존 80 단위까지다. 전체 프로필 이름과 모델을 보존하기 위한 한도 확장이다.
중앙 기존 클라이언트의 revision 없는 등록은 수용하지만 갱신된 기본 label을 되돌리지
않는다. 중앙 0013은 로컬 검증만 하며 배포/운영 migration은 별도다. 최초 선택 화면의
기존 설치 프로필은 DB를 읽기 전용으로 검사하고, 최초 설치는 로그인 프로필을 사용한다.

Daybreak REVISE 보완 (2026-10-06): 기존 directory `/name` PUT의 변경 판단은
(epoch, 파생 기본 이름)으로 하여 아바타·상태·음소거 등 이름과 무관한 프로필 변경은
쓰기를 만들지 않는다. 실제 이름 변경 시 현재 프로필 revision을 보낸다. 영구 4xx는
동일 epoch/프로필 revision에 재시도하지 않고 보류하며, 일시 장애의 기존 재시도는 유지한다.
미적용 0013에서 revision 있는 label을 구 Worker의 label-only 쓰기로부터 보호하고,
80단위 넘는 기본 이름으로 new → old rollback → new 복원을 검증한다. reset_default와
expected_name_is_default는 nonempty registration_epoch를 필수로 하며 frontend도 누락 시
요청 전에 실패한다. 이름 쓰기는 NFC·Cc/Cf 거절·공백 축소 후 길이를 검사하고, 과거
행을 반환하는 bootstrap/member preview에도 같은 정리와 길이 제한을 적용한다.

### Rail (c): 호스트와 공용 프론트 (2026-10-06, 확정 v4.1)

원본 설계에 v2/v3/v4/v4.1을 순서대로 적용한다. Worker는 별도 작업 소유이며
README 변경 시 wire 필드의 정본이다. 중앙은 테스트 대역만 사용한다.
진입점은 기존 member-challenge/join, 새 초대 없는 connect challenge/redeem/방 선택,
공용 서버 레일/중앙 동의/hide/unhide다. 초대 없는 연결은 participant/admission/invite를
생성하거나 소비하지 않는다. SQLite가 binding→admission→canonical Joined→active room과
저장 scope를 재확인하며 방 목록은 50개 이하, 세션은 선택한 방에만 기존 8/32/40 한도로 발급한다.
기존 1024개/300초 challenge에 connect_redeemed/completed 상태를 둔다. 최종 요청은
challenge id/browser credential/room id를 결합하고 같은 방 동시 재시도는 같은 세션,
다른 방은 거절한다. 완료된 같은 방 재시도는 challenge/browser/만료/선택 방 검증 후
canonical 재선택 없이 저장된 응답을 반환한다. 첫 선택의 canonical 검증은 유지하고,
이후 강퇴로 폐기된 bearer는 응답 재전달로 복구되지 않는다. 재시작/만료 시 새 challenge가 필요하다.

Outbox는 (binding, immutable epoch)당 하나다. admission/leave/kick/export/close/delete
트랜잭션에서 하나라도 Joined+active이면 active, 아니면 removed로 계산하고 revision을
1 올린다(1..2^53-1). redeem의 projection_id 교체는 revision을 유지하고 ACK를 지워
현재 상태를 dirty로 만든다. 옛 id/revision ACK는 거절한다. 옛 epoch는 영구 보류한다.
호스트당 한 sender가 16개 배치, 영속 next_attempt_at, 60초×2/최대6시간/±20% jitter,
UTC 하루 48회 상한으로 전송한다. 429는 일시 실패, 영구 4xx는 revision 변경까지 보류한다.
중앙 v4.1 예산은 anchor 6, report 3+6×items, 합계1800/day이며 예산 거절은
grant/anchor 소비 없이 일시 용량 오류를 반환한다. 호스트 테스트는 중앙 대역으로 검증한다.

레일 member는 owner와 같은 모양이며 승인 allowlist(서버 id/관계/정리한 label/icon/epoch/
키 fingerprint/endpoint 상태·origin·generation)만 사용한다. 웹은 purpose connect 중앙
hand-off와 “‘{서버 이름}’에 다시 연결할까요?” 동의, 앱은 직접 연결한다. 방 하나면
바로, 여러 개면 선택한다. “목록에서 숨기기”는 중앙 hide이며 leave가 아니다. 숨긴 서버
재참가는 동의 시 unhide 후 새 grant를 발급한다. 내부 용어 없이 해요체를 쓴다.

회귀는 각 설계 규칙, 같은 방 동시 선택, 교체 projection의 늦은 ACK, 강퇴 직후 거절,
여러 방 중 하나 나가도 active 유지, epoch 회전 보류, 예산429를 포함한다. 변경 범위
테스트와 푸시 직전 make verify 1회, 각1000줄 미만 기능 커밋/푸시, VERIFICATION 건별
한 줄을 남긴다. 서명 빌드/수동 검증/배포와 Worker 변경은 제외한다. leave receipt,
capacity reservation, anonymous merge, 새 owner member 관리 UI는 범위 밖이다.

Rail (c) wire 정본 대조(Worker README 2026-10-06): connect는 별도
`member-connect-grants[/redeem]`, purpose=connect, aamc1 토큰을 쓴다. member
endpoint에는 lease_expires_at도 없다. preview는 숨김 여부를 제공하지 않으므로 동의
화면의 명시적 “목록에 다시 표시하고 참가” 동작이 unhide→새 grant를 수행한다.
거절을 자동 우회하거나 없는 anchor의 unhide 오류를 무시하지 않는다. 앱 직접 연결은
기존 서버 열기 명령의 /join#native-member 전달로 확장하며 서버/epoch별 browser
credential을 사용한다. 도착 origin 검증·fragment 즉시 제거 후 탭별로 보관하고,
멤버 세션에만 선택하여 기존 일반 browser credential을 덮어쓰지 않는다.

호스트 HTTP: `POST /api/member-connect/challenge` (`{}`),
`POST /api/member-connect/rooms` (`challenge_id`, `grant_token`),
`POST /api/member-connect/select` (`challenge_id`, `room_id`, `client_id`).
모두 기존 `X-Device-Token` browser credential과 same-origin/Tauri 경계를 사용한다.
최종 결과는 기존 JoinResponse이며 host 생성 request_id와 첫 선택 client_id를 유지한다.
중앙 예산429는 `member_temporarily_unavailable`/503으로 표시하고 sender는 ACK 없이
기존 영속 backoff를 유지한다.

### Rail (c) E2E 결함 수정 (2026-10-06)

공용 app/web: 세션 만료로 snapshot에서 빠진 Joined 중앙 멤버가 member-join 또는
member-connect/select로 재입장하면 호스트가 방을 다시 열지 않아도 기존
participant_joined 이벤트 경로로 즉시 목록을 복구한다. SQLite 세션 발급과 이벤트를
원자 기록하고 기존 방 publication owner가 전달한다. 정확 재시도는 중복 이벤트를
만들지 않으며 강퇴/만료 권위·실패 의미는 보존한다. 프론트 polling/강제 재입장 우회는 금지한다.
방 설정은 seq 읽음 커서를 표시하지 않는다. 중앙 멤버 접속 종료는 게스트 만료로
표시하지 않고 해요체를 사용한다. 멤버는 일반 사람과 같은 배지, 게스트만 게스트,
HOST/YOU는 방장/나로 표시한다. 서버·프론트 회귀, 변경 범위 테스트와 푸시 직전
make verify 1회로 검증하고 서명 빌드·수동 검증·배포는 하지 않는다.
