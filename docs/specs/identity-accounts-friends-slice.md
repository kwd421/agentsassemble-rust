# Identity, accounts, friends and human admission

Status: Phase 5 active after Phase 4 approval at `53a82f1`.

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
therefore uses the existing private host-to-runtime control pipe, carrying no fake
operator principal and granting no room/account authority. The existing runtime
owns one bounded pending-return collection (16 entries, 10-minute expiry), and the
loopback-only HTTP callback accepts only an expected unguessable state. No public
start/poll endpoint or new reusable credential is necessary. The native host derives
the redirect URI from its exact running runtime and opens only the validated Google
authorization URL. The existing central service remains the OAuth/PKCE exchange
owner. Completion/abort/failure retires transient state; expiry is checked on access,
and runtime shutdown drops it without adding a polling task or persistence table.

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
key IDs can trigger one refresh per minute while the cache is otherwise fresh,
preventing attacker-controlled key IDs from creating an unbounded outbound fetch.
The challenge owner retains at most 512 entries, at most 64 unbound identities,
with five-minute expiry checked on access and one pending challenge per subject.
Invalid proof does not consume a legitimate challenge; successful proof consumes it
before account persistence revalidates the current identity. Shutdown drops all
in-memory state. Tests use newly generated RSA keys, never real Google credentials.

### Account presentation boundary

The local HTTP account flow is connected to the settings account section. Native
operator access stays on exact local ticket transport; public browser/session proof
cannot recover the local operator. Only configured browser servers admit the fixed
GIS script/style/frame/connect sources. Google proof remains transient until a
separate focused guest-discard confirmation; failed proof/persistence requests never
produce a connected view. The native startup central account remains separate.

### Saved friend directory boundary

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
and the consumed session expires one hour after redemption. A still-live same-device
retry returns the same bearer even after grant expiry; another device, revocation,
expired session, changed room incarnation or changed host lineage cannot redeem it.
The ordinary human bearer remains unchanged; a distinct operator prefix and HMAC
context use the same existing derivation mechanism. Only fingerprints are persisted.

The record cap is 128 per server and 32 per room. Creation removes expired records
before checking capacity; revoked consumed records remain until session expiry so a
retry cannot revive them. There is no background task. Current manager resolution
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
and grant state; the UI never renders the raw pairing URL. An unknown revocation
stays non-copyable and retries the same recorded grant rather than minting another.

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
