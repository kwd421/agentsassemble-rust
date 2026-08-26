# Trusted Public Ingress Slice

Status: design approved; invite-use policy prerequisite implemented; trusted ingress,
manager controls, and frontend activation remain incomplete

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
require a recognized bound loopback authority (`localhost`, `127.0.0.1`, or `[::1]`
with the exact port), no proxy provenance, and either no Origin, the same loopback
Origin, or one of the exact Tauri origins already owned by the CORS policy.

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

Final plan review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `f83707c` web review: `APPROVE — Critical 0 / High 0 / Medium 0`.

Commit `f83707c` Daybreaker review: `APPROVE — Critical 0 / High 0 / Medium 0`.
