# Trusted Public Ingress Slice

Status: design approved; invite-use, exact local TCP trust, route exposure, local
identity probes, configured-manual trust, and the direct managed quick-tunnel
lifecycle are implemented and verified; stable entry, manager controls, and
frontend activation remain incomplete

## Definition

This slice makes the existing human invite and room-session flow reachable through
one server-owned public ingress. It preserves the original direct human invite API,
Cloudflare quick-tunnel lifecycle, stable entry, public server identity proof, and
normal/read-only browser flow without retaining browser-owned host tokens, local
preview URLs, transport fallbacks, or legacy compatibility paths.

The behavior comparison baseline is original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`. The approved Rust design baseline is
`39b9311`. Current behavior comes from reachable original code and real flows, not
old product markdown.

## Authority boundaries

- `PublicIngress` is the only owner of the active generation, direct public origin,
  generated origin credential, managed cloudflared child, ingress revocation, and
  cleanup completion. Stable-entry state must join this same owner when implemented.
- The TCP accept boundary preserves the actual peer `SocketAddr` in one private
  request extension. Forwarding or client-IP headers never replace that transport
  fact.
- One ingress trust policy validates local, managed, and configured-manual requests.
  Route descriptors classify exposure; they do not implement a second trust policy.
- Local room-manager authority remains room-owned. A private one-use grant is bound
  to the exact room and create or revoke operation, is consumed before a bounded
  body, and is revalidated with current room-manager state in the persistence
  transaction.
- A ready-ingress snapshot is routing input, not durable room or invite authority.
  Its public origin enters the signed invite claim and immediate `join_url`; it is
  not copied into a SQLite column.
- Agent invite, operator pairing, and companion/friend external actions remain
  explicitly unavailable until their distinct server contracts are implemented.

## Trust and route exposure

Every accepted request has an actual loopback peer. Local requests additionally
require the exact bound numeric loopback address and port, or `localhost` with that
port only when bound to `127.0.0.1` or `[::1]`; no proxy provenance; and either no
Origin, the exact same HTTP authority, or one of the exact Tauri origins already
owned by the CORS policy.

A managed request requires the current generation's secret `.origin.invalid` Host,
the exact current public HTTPS forwarded authority and scheme, and the actual
loopback peer. `CF-Ray` is only corroborating data. A configured manual proxy instead
requires the exact current public HTTPS Host, exact forwarded `https` scheme, actual
loopback peer, and a startup-only high-entropy secret compared through fixed-size
digests in constant time. The secret is non-serializable, never enters response or
frontend state, and cannot be changed at runtime. Without both startup values,
manual mode is unavailable. The local operator may select or clear only the canonical
HTTPS public origin after that separate control contract is implemented.

Dynamic route registration owns method, path, and one of exactly three exposure
meanings: private, same-origin public, or identity-probe public. Dynamic requests use
Axum's matched path and actual method to select that descriptor. Static registration
owns mount, signed surface pattern, and exposure in one descriptor. A small
static-specific service wrapper passes that descriptor to the same trust policy;
it does not infer exposure from the raw URI or maintain another prefix allow-list.

The public static surface for this slice is only `/join`, `/join/`,
`/assets/{*path}`, and `/join/assets/{*path}`. Root, `/app/{*path}`, `/pair`,
`/pair/`, and `/pair/assets/{*path}` remain private or incomplete. A public guest
that leaves, is removed with its room, or exits an expired session clears its session
and navigates to token-free `/join` with query and fragment removed.

`GET /api/server-info` and `POST /api/server-info/challenge` are the only
identity-probe routes. They allow cross-origin, credential-free key discovery while
still requiring a complete local or public ingress trust decision. The challenge is
22-128 Base64url characters and signs the exact normalized current origin under
`AA-SERVER-CHALLENGE-1`, with protocol version 1, server ID, public JWK,
fingerprint, issue time, and signature. Other public APIs retain exact same-origin or
Tauri Origin checks.

## Managed and stable lifecycle

Managed mode starts only the maintained `cloudflared` executable against the local
loopback server with a server-owned empty config and a fresh high-entropy origin
Host. `process-wrap` supplies Unix process-group and Windows job custody plus
kill-on-drop safety. Output readers enforce bounded lines and total buffering,
accept only a strict `https://<valid>.trycloudflare.com` URL, and never retain or
return raw logs.

Start, stop, child exit, and direct-origin replacement are serialized by one
generation. The transition is a tracked task owned and joined by `PublicIngress`,
not by an HTTP handler. Caller disconnect, the HTTP connection deadline, or retry
cannot drop the transition or create another generation. Direct readiness begins
only after the same generation emits a strict quick-tunnel origin. Stop and shutdown
revoke ingress first, request graceful child termination, enforce a deadline, kill
the owned group or job if needed, and await the child and both output readers.
Cleanup failure is explicit and blocks restart.

Stable publish and clear are not implemented yet. Their eventual configuration-absent
state is exactly `unconfigured`; configured operations require explicit pending,
ready, or failed results under the same lifecycle owner. The stable URL will not be
a fallback invite URL, and the still-incomplete manager/frontend activation must not
claim stable-entry readiness meanwhile.

## Human invite activation

The create wire preserves `meeting_id`, `invite_scope`, positive `ttl_seconds`,
bounded integer `max_uses`, and optional bounded `display_name`. Empty display name
defaults to `Guest`. Negative maximum use count normalizes to configured zero; the
UI retains its 1, 5, and 0 presets without narrowing the API. Human/browser/manual
kind and the unique base participant ID are server-generated. Agent and provider
identity fields are not accepted.

After grant and input validation but before credential generation or a database
write, create requires one immutable current `ReadyIngress` snapshot. Not-ready
requests fail without writing. The signed origin and returned URL derive from that
one snapshot with no local or stable fallback. A later independent ingress stop is a
normal lifecycle race and does not introduce a cross-resource transaction.

HTTP owns bounded ingress status and control, invite CRUD, admission, profile,
preferences, and leave. WebSocket owns canonical snapshot, events, and room commands.
Neither transport is a fallback for the other.

## Invite-use policy prerequisite

Commit `f83707c` removes three independently changeable expressions of the effective
invite-use ceiling. One persistence helper now maps configured values to the current
effective limit. Durable decode, preflight, pre-join authorization, and admission
use that helper; the atomic consume update binds its computed result instead of
copying the policy into SQL. The schema checks storage shape and nonnegative counts,
while durable decode rejects a row above the effective product limit.

This DDL change creates clean schema 41. Schema 40 is rejected with
`SchemaVersionMismatch`; no migration, import, compatibility reader, or fallback was
added. Configured values, public results, one-use/reusable classification, retry,
and effective ceilings are unchanged.

The change removes duplicated code and schema policy rather than adding runtime
state. The consume query gains one integer bind and loses a SQL `CASE`; there is no
claimed throughput or latency improvement. The concrete benefit is preventing
preflight, decode, schema, and atomic consume from drifting to different limits.

Verification passed all 171 persistence tests, the competing-final-use one-winner
test, malformed durable-row rejection, schema-40 rejection, warning-denied
persistence Clippy, and the workspace architecture, source-growth, formatting, and
all-target check gates.

## Local TCP ingress prerequisite

Commits `b5b700e` and `4b54317` make the accepted TCP peer and the actual bound
loopback address the local trust owner. The concrete threat was that forwarded
provenance could be mistaken for transport identity, while the first implementation
lost the listener IP and treated distinct loopback Origin aliases as same-origin.
`LocalIngress` now keeps one `SocketAddr`; a request must arrive from a loopback peer,
carry no recognized proxy-provenance header family, use the exact bound numeric
address and port or the applicable `localhost` alias, and either omit Origin, use the
exact same HTTP authority, or use one existing Tauri Origin.

The correction replaces independent proxy-header lookups with one bounded scan of
the request header names so standard and product-specific provenance families have
one owner. This is a policy-drift correction, not a claimed throughput improvement;
it adds no runtime task, cache, fallback, compatibility path, or public state. The
accepted local HTTP, WebSocket upgrade, static frontend, CORS, body-deadline, and
security-header contracts remain unchanged.

An actual `TcpListener`/`TcpStream` test proves the accepted peer extension reaches
the middleware: exact same-origin receives 200, while `Via`, a mismatched Host port,
and a cross-alias Origin receive 403. The complete repository verification passed
the structure and source-growth gates, frontend build and 403 tests, desktop build
and 16 tests, all Rust workspace tests including 120 provider and 63 server unit
tests plus the new TCP boundary test, and warning-denied workspace Clippy.

## Route exposure prerequisite

The dynamic registration macro now owns method, matched path, handler, and exposure
together. One iterator over those descriptors supplies both the signed product
surface and ingress lookup; the prior duplicate module inventory was removed. At
this prerequisite commit, the reachable routes were explicitly private or
same-origin public and the identity-probe variant was absent. The later local
identity-probe increment adds that exposure only with its two implemented routes, so
the prerequisite itself added no unused future state.

Static frontend registration now owns its actual mount, signed surface pattern, and
exposure in one descriptor. The small static router wrapper passes that exact
descriptor through a server-owned request extension. Static directory services use
an exact GET wildcard route and strip only their descriptor's fixed prefix before
calling the maintained `ServeDir`; they do not use `nest_service`, whose implicit
bare-prefix routes exceed the declared wildcard. `/app` and `/app/` are explicit
private index descriptors. Bare asset prefixes are not registered and retain their
previous 404 response without inheriting public exposure. Dynamic lookup uses
Axum's matched path and actual method, maps HEAD to its GET route, and retains
registered path admission only for Axum-owned method mismatch and CORS preflight
handling. No raw-URI prefix allow-list, second trust policy, compatibility route,
new fallback, or client authority was introduced. The public static classification
is exactly `/join`, `/join/`, `/assets/{*path}`, and
`/join/assets/{*path}`; root, app, pair, and pair assets remain private.

The concrete threat was route-policy drift: product-surface signing, dynamic router
registration, static mounts, and the future public trust decision could otherwise
name independently changeable surfaces. A normal dynamic request performs one
bounded linear scan of the compile-time descriptor inventory; static requests read
their attached descriptor without a scan. Exact static directory dispatch replaces
Axum's internal prefix stripping with one bounded path/query string allocation and
URI parse per static file request. That accepted cost closes the observed exposure
gap without a second route policy. This slice adds no task, cache, map, process,
disk state, or claimed latency improvement. The fixed inventory is too small to
justify another runtime owner without observed cost.

Verification proves descriptor uniqueness and the exact public/private static map,
preserves a real TCP 405 for method mismatch and a real Tauri CORS preflight, and
keeps the existing packaged-static security/cache flow reachable. A real TCP/static
fixture additionally proves `/app` and `/app/`, query-bearing assets, all three
asset aliases, GET-only dispatch, and 404 bare asset prefixes. All 66 server unit
tests and 41 server integration tests passed. Complete repository verification
also passed the architecture and source-growth gates, frontend build and 403 tests,
desktop build and 16 tests, all 171 persistence and 120 provider tests, and
warning-denied workspace Clippy.

## Local identity-probe increment

`GET /api/server-info` and `POST /api/server-info/challenge` are now mounted and
advertised in every Rust server composition. Both descriptors alone own the new
`identity-probe public` exposure meaning. The existing database-bound
`CentralHostIdentity` remains the only Ed25519, public-JWK, and canonical-fingerprint
owner; the routes do not load another key, copy private material, or create a second
identity state.

The GET response preserves the stable server ID, exact public JWK and fingerprint,
protocol version, ready status, and an honest disabled central-directory publisher
status. The latter does not claim that the still-absent background publisher is
enabled merely because desktop registration is available. Challenge POST reads one
bounded 4 KiB JSON body, validates 22-128 base64url characters, normalizes the exact
trusted local request authority, and signs
`AA-SERVER-CHALLENGE-1\n{server_id}\n{origin}\n{challenge}\n{issued_at}`. It returns
the exact origin, caller challenge, whole Unix issue second, identity projection, and
raw Ed25519 signature in canonical base64url form.

The route-local CORS policy is credential-free, permits only GET/POST and
`Content-Type`, and emits the wildcard origin only after the common ingress boundary
admits the request. Preflight resolves its requested method through the same route
descriptor, so a module-wide CORS method list cannot authorize a path/method pair that
is not registered. Local Host, peer, proxy-provenance, and Origin checks are unchanged:
a foreign Origin against a loopback Host still fails closed. At that prerequisite
commit, cross-origin public probing remained unreachable because `PublicIngress` did
not yet supply and verify an origin; the configured-manual increment below now opens
that exact path without changing the local rule.

The concrete security requirement is endpoint-key substitution resistance: the
caller nonce and exact current origin must be covered by the already durable host key.
The first implementation repeated the loopback host policy as a fixed three-value
list, so a server successfully bound to another numeric loopback address admitted
the request but rejected its challenge origin. The common HTTP host classifier now
uses `IpAddr::is_loopback()` while preserving the exact trusted address and port;
the invite URL policy reuses the same classifier instead of retaining another copy.
Invite issuance therefore performs one additional bounded numeric parse of an
already length-limited host; it adds no allocation, task, cache, or disk state, and
that small accepted cost removes the observed policy-drift owner.
This increment adds no task, cache, process, disk row, secret, retry, fallback, or
compatibility state. A GET clones the bounded public identity projection. A POST adds
one bounded body decode, one origin parse, one short transcript allocation, and one
Ed25519 signature; no throughput improvement is claimed without an observed cost.
Focused verification used the real TCP server to prove stable GET/POST identity,
exact local-origin binding on both primary and alternate numeric loopback listeners,
signature verification, invalid-challenge rejection,
foreign local-Origin denial, the credential-free Tauri preflight, and path/method
preflight mismatch rejection. All 67 server unit tests and 43 server integration tests passed. Complete repository verification
also passed the architecture and source-growth gates, frontend build and 403 tests,
desktop build and 16 tests, all 171 persistence and 120 provider tests, and
warning-denied workspace Clippy.

## Configured-manual trust increment

`PublicIngress` now owns the immutable configured-manual public origin and proxy
credential projection. Startup enables it only when both
`AGENTSASSEMBLE_PUBLIC_URL` and `AGENTSASSEMBLE_TRUSTED_PROXY_TOKEN` are present;
one-sided configuration fails before the private control secret or database is read.
The origin parser accepts one root HTTPS origin, normalizes its default port and host,
and rejects userinfo, path, query, fragment, `localhost`, numeric loopback, and
unspecified numeric hosts. The proxy credential must be 32-128 visible ASCII bytes.
Only its SHA-256 digest is retained by ingress state, and each request hashes the
presented value before a fixed-size constant-time comparison.

The existing common ingress middleware remains the sole request trust decision. A
configured-manual request must have an actual loopback TCP peer plus one exact public
Host, `X-Forwarded-Proto: https`, and proxy-token header. It can reach only the route
descriptor's `same-origin public` or `identity-probe public` exposure; `private`
always fails. Same-origin public permits no Origin or the normalized configured
origin. Identity probes permit a foreign Origin only after the full proxy decision,
then receive the already trusted HTTPS origin through a private request extension so
the challenge handler does not independently infer scheme or authority.

The concrete threat was accepting forged forwarding headers from a local process or
letting a public Host bypass route and Origin classification. This implementation
adds no process, task, timer, cache, database state, compatibility path, fallback, or
runtime mutation. Each configured connection clones one small immutable projection;
each attempted public request performs one bounded authority parse and one SHA-256
operation, plus an origin parse when Origin is present. No throughput improvement is
claimed. The connection cancellation select moved into the HTTP connection owner so
the server accept loop stayed below its structural limit; the existing task tracker,
shutdown token, connection deadline, and cleanup order are unchanged. Repository-wide
search found the startup environment names only at the
executable owner and the proxy header name only at the common trust owner outside
tests; local provenance rejection reuses that same header constant.

Focused verification uses the real binary startup/control pipe and a real TCP
listener. It proves paired startup reaches the public identity route, one-sided
startup fails, exact and absent same-origin requests load `/join`, a foreign Origin,
wrong Host, wrong scheme, wrong token, private route, and unconfigured server all
receive 403, a foreign-origin identity probe succeeds, and its Ed25519 challenge
signature binds the configured HTTPS origin.

Complete repository verification passed the architecture, source-growth, policy,
formatting, and warning-denied Clippy gates; the frontend build and 403 tests;
desktop build, Clippy, and 16 tests; all 171 persistence and 120 provider tests; and
all 68 server unit plus 45 server integration tests.

## Managed direct lifecycle increment

Commits `0110e5b`, `db88968`, and `cc54242` implement the direct managed
quick-tunnel lifecycle without stable-entry publication. `PublicIngress` owns one
mutex-protected lifecycle containing the closed bit and optional active generation;
the projection separately contains the same generation's bounded public status and
trust snapshot. Start and stop require exact one-use operator HTTP tickets. The HTTP
handlers await the server-owned transition but never own its process or task custody.
Shutdown closes the lifecycle before joining the same stop path, and ingress cleanup
is awaited before unrelated room, provider, or reconciliation errors are propagated.

The concrete threats were observable rather than speculative: an HTTP caller could
cancel after removing the only generation handle; a ready line could restore trust
after stop; a racing start could spawn after shutdown; separate projection reads
could authenticate generation N but sign generation N+1's origin; terminal error
could be overwritten by stop; and an updater or descendant process could escape the
server's generation custody. The implementation therefore keeps the `JoinHandle`
inside the lifecycle mutex until its owner finishes, rejects readiness outside the
same generation's `Starting` or `Running` phase, returns identity authorization and
its origin from one projection read, fixes `--no-autoupdate`, and uses maintained
`process-wrap` process-group or job-object custody. Cleanup failure is retained as a
restart-blocking state rather than hidden by a fallback.

Running quick tunnels preserve the original reachable reconnect contract: another
valid trycloudflare origin from the same generation atomically replaces the old
origin and trust. Stale generations and `Stopping`, `Error`, or `Stopped` cannot
install trust. Output parsing accepts only one bounded single-label
`https://<label>.trycloudflare.com` origin from a line, lowercases it through the
canonical origin owner, and rejects deceptive suffixes, nested labels, and invalid
tenant labels. Raw cloudflared output, environment values, and the generated origin
Host are not returned to the browser or persisted.

The measured steady-state resource boundary is one cloudflared child tree and four
owned tasks: one generation owner, one child supervisor, and two bounded output
readers. The readers feed an eight-event channel and accept at most one 16 KiB line
each. Start creates one private temporary directory containing an empty config; after
the child is moved to its supervisor, the generation-owner future retains that
`TempDir` until cleanup completes. Stop has one five-second graceful deadline and one
five-second forced-stop deadline, while each reader has a five-second join deadline.
No polling task, durable row, cache, compatibility state, retry fallback, or
stable-entry state was added. No throughput or latency improvement is claimed; the
accepted bounded work purchases exact process, trust, and cancellation custody.

Commits `fd9e7e4` and `4249e12` add only contract tests. Four lifecycle tests cover
same-generation reconnect, stop and terminal trust revocation, one-snapshot identity
origin, cancelled-stop handle retention, shutdown closure, and cleanup-failure
restart denial. Parser tests cover exact valid and deceptive origins. A Unix
integration test starts the real server with a spawned fake cloudflared process,
crosses raw TCP for the accepted `/join` request, rejects forged forwarded Host,
scheme, Origin, and private status access, checks the exact child arguments, stops
the process group, observes both leader and descendant termination markers, and
proves the retired trust can no longer load `/join`.

The actual raw-TCP/process test passed three consecutive focused runs after the
final review correction. Complete `make verify` passed architecture, source-growth,
and policy gates; formatting; all-target workspace check; generated protocol types;
frontend build, original-CSS verification, and 403 tests; desktop Clippy and 16
tests; all workspace tests including 171 persistence, 120 provider, and 74 server
unit tests plus the server integration suite; and warning-denied workspace Clippy.
The test-only lifecycle module is separate from the production owner; relevant files
remain 625, 196, and 798 lines rather than weakening the 800-line gate.

## Verification requirements

- Header tests cover local Host and Origin aliases, Tauri origins, forged forwarded
  headers, stale generation credentials, configured manual secrets, OPTIONS, and
  the identity-probe exception.
- Lifecycle tests cover concurrent start and stop, child exit, stable publish and
  clear failure, cancelled HTTP callers, retry, shutdown, bounded output, and exact
  reader and process cleanup.
- Route tests load `/join` and both required asset forms through trusted ingress and
  prove root, app, pair, operator, provider, and manager surfaces remain private.
- Manager tests prove body-before-authority is impossible, room/action mismatch and
  stale manager fail, not-ready create writes nothing, and every issued credential
  and URL uses one ready-generation origin.
- Isolated-browser verification covers normal and read-only one-use and reusable
  admission, WebSocket readiness, normal posting, read-only denial, consumed and
  revoked invites, reload and server restart, exact leave, retired tunnel failure,
  and configured stable clear. Verification-owned browser, child, database, config,
  and bundle resources are removed or stopped afterward.

## Review findings

- Trusted peer provenance had to originate at TCP accept rather than forwarding
  headers or a loopback-bind assumption.
- Local trust had to preserve the three existing Tauri origins and recognized
  loopback Host aliases.
- Manual proxy secret provisioning had to be startup-only and server-owned.
- Stable publish and clear had to be a prerequisite of completed stable-entry
  activation, not later best-effort cleanup.
- Human create had to preserve the direct API's `meeting_id`, optional display name,
  arbitrary positive TTL, negative-to-zero use normalization, and non-preset use
  counts while removing agent identity fields.
- Ingress transitions needed a server-owned tracked task so HTTP cancellation could
  not abandon revocation, child exit, or reader joins; stable cleanup must join that
  same owner when implemented.
- Public guest exit had to navigate to token-free `/join` rather than private root.
- Effective invite-use policy was duplicated in Rust, SQL consume, and schema DDL.
- Invite creation needed a server-side ready-generation snapshot; frontend readiness
  was not authority.
- Removing schema policy required schema 41 and explicit schema-40 rejection.
- Public origin did not require a second durable invite field because the signed
  bytes and fingerprint already bind it.
- Static nested services do not expose Axum `MatchedPath`; static exposure therefore
  needed direct canonical-descriptor binding rather than a raw-URI fallback.
- The first exposure implementation used `nest_service` for wildcard static
  descriptors. Axum also registered each bare prefix and trailing slash under the
  same exposure, so the actual match set exceeded the signed surface even though the
  asset service returned 404 there. Exact GET wildcard registration and explicit app
  index descriptors removed that undeclared route authority.
- The first local ingress implementation omitted `Via`, `X-Real-IP`, and complete
  forwarded/proxy header families from its provenance rejection.
- The first local ingress implementation accepted every loopback bind but discarded
  the listener IP, so an exact alternate-loopback Host was rejected after startup.
- The first local Origin check validated Host and Origin aliases independently,
  allowing cross-alias origins that were not same-origin with the request Host.
- The first review record still described local Host authority as three fixed
  aliases, contradicting the implemented exact-bound numeric loopback policy.
- Identity routes could reuse the existing persistent signing projection; a second
  identity key, cache, or response-specific fingerprint owner was unnecessary.
- A local challenge must sign the already trusted request authority. Deriving or
  accepting an HTTPS public origin before `PublicIngress` owns that origin would be a
  placeholder trust path.
- Identity origin normalization repeated loopback policy as three fixed host strings,
  so an admitted alternate numeric loopback listener could not issue a challenge.
- The route-exposure prerequisite retained a pre-identity historical statement and a
  fixed descriptor count after the identity routes changed both facts.
- The first managed lifecycle review found cancel-sensitive generation-handle
  removal, readiness after stop, restart after cleanup failure, a shutdown/start
  race, delayed ingress cleanup joining, split trust/origin projection reads,
  terminal-state overwrite, and missing `--no-autoupdate`; `db88968` closed them.
- The managed reconnect review found that accepting only the identical origin while
  `Running` narrowed the original same-generation reconnect behavior; `cc54242`
  restored atomic replacement without reopening terminal trust.
- The first test review found that terminal readiness rejection and descendant
  process-group cleanup were not directly asserted; `4249e12` added both proofs and
  moved the cohesive lifecycle tests out of the production owner at its responsibility
  boundary.
- The first lifecycle evidence record omitted the child-supervisor task, assigned
  the temporary directory to that task instead of the generation owner, and left the
  canonical frontend/backend exposure inventory saying all public ingress was
  incomplete; the follow-up corrected both resource custody and exposure status.

Final plan review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `f83707c` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `f83707c` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `b5b700e` web review: `REVISE — Critical 0 / High 0 / Medium 2`.

Commit `b5b700e` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 2`.

Commit `4b54317` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `4b54317` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `c25bcea` web review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `c25bcea` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `a2e4216` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `a2e4216` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `aa123ef` web review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `aa123ef` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `3a38736` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `3a38736` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `c0b59a9` web review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `c0b59a9` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 2`.

Commit `2d97e77` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `2d97e77` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `d9d798d` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `d9d798d` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `4e333ad` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `4e333ad` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `0110e5b` web review: `REVISE — Critical 0 / High 0 / Medium 5`.

Commit `0110e5b` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 4`.

Commit `db88968` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `db88968` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `cc54242` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `cc54242` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `fd9e7e4` web review: `REVISE — Critical 0 / High 0 / Medium 2`.

Commit `fd9e7e4` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `4249e12` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `4249e12` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `82538b2` web review: `REVISE — Critical 0 / High 0 / Medium 1`.

Commit `82538b2` Daybreaker review: `REVISE — Critical 0 / High 0 / Medium 1`.
