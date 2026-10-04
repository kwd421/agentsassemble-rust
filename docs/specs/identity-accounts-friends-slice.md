# Identity, accounts, friends and human admission

Status: Phase 5 locally verified and approved by Daybreak through `1e24adf`, C0/H0/M0/L0.

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

Follow-up startup contract: when central login is configured, a completed local
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

### Central owner server reopening (2026-10-01)

After a current central session is validated, the desktop server chooser may reopen
only a server whose current central relation is `owner`. Bookmarks remain discovery
metadata and continue through their explicit invite or pairing authority. Selecting
an owned server asks the central Worker for a five-minute, server-bound connect grant.
The grant is bound to the issuing central session, person, device, server, exact
published endpoint origin and endpoint generation. Logout, session/device/person
revocation, owner transfer, endpoint replacement, lease expiry or server revocation
invalidates it. The central bearer and device private key never leave the selecting
device and are never sent to the remote room host.

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
preserves its rooms/profile, and registers it before entering. Local-only builds
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
oversize bodies and compressed expansion. Icon removal leaves no orphan blob;
registration/account deletion cascades the icon row. Server lists contain only the
small image reference, not repeated image data.

Acceptance: real Worker request handler with signature checks and migrated SQLite
proves cross-device lists/image bytes, owner-only set/remove, stale/replay/revocation
rejection, atomic rollback, preservation across registration and cleanup. Local
workerd/D1 verifies runtime/storage compatibility; production deployment and migration
remain separate authorized operations. No frontend or room-icon modifications.
