# Trusted Public Ingress Slice

Status: design approved; invite-use, exact local TCP trust, and route-exposure
prerequisites implemented; managed/manual trust, manager controls, and frontend
activation remain incomplete

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
  generated origin credential, managed cloudflared child, stable-entry state,
  ingress revocation, and cleanup completion.
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
loopback peer. `CF-Ray` is only corroborating data. A configured manual proxy uses
the same checks plus a startup-only high-entropy secret compared in constant time.
The secret is non-serializable, never enters HTTP or frontend state, and cannot be
changed at runtime. Without it, manual mode is unavailable. The local operator may
select or clear only the canonical HTTPS public origin.

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

Start, stop, child exit, and stable publish or clear are serialized by one generation.
The transition is a tracked task owned and joined by `PublicIngress`, not by an HTTP
handler. Caller disconnect, the HTTP connection deadline, or retry cannot drop the
transition or create another generation. Start becomes ready only after the exact
configured stable publish succeeds. A publish failure revokes ingress and cleans up
the child. Stop and shutdown revoke ingress first, await stable clear, request
graceful child termination, enforce a deadline, kill the owned group or job if
needed, and await the child and both output readers. Cleanup failure is explicit.

Stable configuration absent means exactly `unconfigured`. When configured, publish
and clear have explicit pending, ready, or failed results. The stable URL is not a
fallback invite URL; the current frontend continues to copy the direct quick-tunnel
URL.

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
surface and ingress lookup; the prior duplicate module inventory was removed. The
currently reachable routes are explicitly private or same-origin public. The
identity-probe variant is deliberately absent until its two routes are implemented,
so this prerequisite adds no unused future state.

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
bounded linear scan of at most 20 compile-time descriptors; static requests read
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
- Stable publish and clear had to be a prerequisite of completed trusted ingress,
  not later best-effort cleanup.
- Human create had to preserve the direct API's `meeting_id`, optional display name,
  arbitrary positive TTL, negative-to-zero use normalization, and non-preset use
  counts while removing agent identity fields.
- Ingress transitions needed a server-owned tracked task so HTTP cancellation could
  not abandon revocation, stable cleanup, child exit, or reader joins.
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
