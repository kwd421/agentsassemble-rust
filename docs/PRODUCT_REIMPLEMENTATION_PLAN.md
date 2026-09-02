# Product reimplementation master plan

Status: approved planning owner at reviewed content checkpoint `9711232`; Phase 0B
foundation correction is active from public baseline `4ab5ee1`.

Comparison baseline: original product commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`; audited Rust baseline `8a5f75a`.

## Purpose and authority

This file owns the complete retained-product inventory, dependency order, and
phase exits for the asynchronous Rust reimplementation. It does not restate the
detailed contracts held by `docs/specs/*`, implementation facts held by
`docs/ARCHITECTURE.md`, exposure evidence held by
`docs/FRONTEND_BACKEND_GAPS.md`, or execution evidence held by
`docs/VERIFICATION.md`.

The original commit is behavior-discovery evidence, not authority for legacy
structure, compatibility paths, fallbacks, or known defects. A feature is complete
only after the same reachable entry point, authority owner, state transition,
failure/retry semantics, and real user flow are verified through Rust.

## Product boundary

Retain:

- local desktop bootstrap, account and identity flows, rooms, human admission,
  participants, room settings, ordinary conversation, and server/public-ingress
  management;
- Risu, CCv3, and CHARX persona-card import, explicit Agent Session selection,
  safe prompt application, and inert storage of unsupported executable card data;
- ordinary `ordered` and `ambient` room conversation, agent-initiated web search
  and tool use when authorized, and normal human-requested synthesis, decisions,
  planning, or task-assignment discussion;
- room-owned participant roles, permissions, mute and membership lifecycle;
- message history, pagination, edit/delete, search/context, pins, attachments,
  votes, custom text channels, side chat, friends, and provider lifecycle;
- the copied frontend as the starting implementation, with verified geometry and
  behavior corrections rather than a from-scratch visual approximation.

Exclude permanently from this reimplementation:

- the old scripted v0 meeting runner and every v0-only research, forced-round,
  agenda, automatic moderator/decision/task, artifact, model, adapter, template,
  demo, seed, test, and documentation path listed in `AGENTS.md`;
- Python fallback, legacy compatibility or migration logic, placeholder authority,
  fake data, disabled synchronization, client-owned substitutes for server
  contracts, and speculative future frameworks.

Defer until the retained core is complete and the user explicitly reopens them:

- voice presence/chat, Mafia, and the RimWorld plugin;
- PostgreSQL central-hosting support;
- CLIProxyAPI-style model-source extraction and alternate-harness experiments;
- any new provider not present in the verified original inventory. Grok is one of
  the retained original sixteen, but its implementation and real verification wait
  for the official client to be installed and for explicit provider-run approval.

## Product-wide invariants

1. SQLite is the current durable local authority. PostgreSQL is not a parallel
   authority while deferred.
2. A person profile is owned by the person's profile record. The left-bottom human
   profile is its canonical UI projection.
3. Room membership, role, room mute, room permission, and join/leave state are
   owned by the room participant authority. A profile never overwrites them.
4. An Agent Session owns its own display name, avatar, provider, model, runtime,
   persona selection, and provider-session identity. It is not merged with the
   owner's human profile. A room participant may add only room-owned role and
   membership state to its projection.
5. Human admission, current-session Room Connector admission, external
   `assemble room attend` AgentBridge admission, and server-managed AgentBridge
   runtime custody are distinct protocols and principals. None widens or aliases
   another route, credential, permission set, or lifecycle.
6. Profile current/pending images, pre-join avatars, room appearance, and message
   attachments have separate ownership and lifecycles. Only hard physical safety
   calculation and format/size validation are shared.
7. Provider-native transport owns transport facts only. The common adapter owns
   runtime/process custody and normalized turn lifecycle. The room owner alone
   authorizes actions and commits canonical messages/events.
8. A capability is advertised only when its executable product action exists at
   the bound server surface. Copied controls do not create backend authority.
9. Failure, unsupported state, and uncertain external effect remain distinct and
   visible. Empty values, silent catches, arbitrary model substitution, scraping,
   transcript inference, polling, or another provider do not replace them.
10. HTTP handles bounded request/response work such as bootstrap, CRUD, search,
    upload/download tickets, and OAuth handoff. Authenticated WebSocket handles
    live room snapshots, events, presence, commands, and provider requests. This is
    the Discord-style division selected for the product; neither transport is used
    dogmatically for every operation.
11. A defensive layer is retained only for a reachable use case, observed failure,
    or concrete in-scope threat. A security label does not justify duplicate
    credentials, proof state, crypto, polling, or recovery machinery when a smaller
    fail-closed owner preserves the same product and security contract.

## Authority map

| Product concept | Canonical owner | Derived consumers |
| --- | --- | --- |
| Local bootstrap and device identity | desktop-issued local authority plus Rust persistence | startup gate, private HTTP/WS ticket issuers |
| Central account and recovery | central identity/account service contract | desktop handoff and account settings UI |
| Room directory and lifecycle | room repository and room mutation owner | room rail, settings, archive/close/delete controls |
| Person profile | user-profile repository | left-bottom card, roster/timeline/search author projections |
| Room participation | room participant record and room permission policy | roster controls and per-viewer capability projection |
| Agent identity and runtime profile | Agent Session record | member details, provider controls, turn attribution |
| Human invite and session | human invite/admission/session owners | join page, direct route authorization, admitted WS ticket exchange |
| Room Connector admission | connector invite/session owner plus room MCP transport | external-AI invite UI and current AI session |
| External AgentBridge admission | attendee invite and bridge-session owner | `assemble room attend` client |
| Managed AgentBridge runtime | provider bridge process and report/turn owner | server-managed external provider sessions |
| Provider catalog and selection | provider registration plus bounded live catalog | Agent Add controls and exact creation validation |
| Provider credential/login/usage | provider-specific credential and operation owner | only controls advertised by that owner |
| Provider process and conversation | common adapter plus provider-native driver | durable Agent Session runtime projection |
| Ordered/ambient turn scheduling | room turn/floor persistence owner | provider adapter input and room events |
| General messages and mutations | lobby message/event persistence owner | timeline, history, edit/delete, search and pins |
| Custom text messages | custom-channel message owner | custom channel timeline, search, and pins |
| Side chat | side-chat repository and event owner | right panel and mobile room-info panel |
| Persona cards | persona library plus Agent Session selection | safe provider prompt construction |
| Asset hard ceiling | common physical-occupancy calculation | four independent lifecycle owners only |
| Public ingress | configured-manual/managed/stable-entry owners | invite presentation and public route admission |
| Product surface | concrete registered HTTP/WS actions | native/sidecar startup equality and frontend feature gating |

## Retained feature inventory and dependency order

The status below describes the audited baseline, not an approval. `implemented`
means code exists; it can still be reopened by Phase 0 findings.

| Area | Audited Rust state | Required remaining result | Phase |
| --- | --- | --- | --- |
| Core room snapshot/event transport | finite snapshot/catch-up/replay is implemented; redundant receipt and per-frame proof removed at `3ffb9eb`, `77cae0e`, and `0d24741` | preserve the synchronization contract through final real-flow verification | 0, 9 |
| Local bootstrap, room directory/create | implemented | reconcile stale spec statuses and final real-flow evidence | 0, 9 |
| Room settings/preferences/appearance | substantially implemented | verify one authority per setting and exact copied controls | 4, 9 |
| Human profile and avatar | implemented but Agent projection/UI ownership is inconsistent | restore human and Agent Session SSoTs; keep exact asset lifecycle | 3 |
| Human invite/admission/session/public ingress | substantially implemented; dead host challenge removed at `a7949bd` and redundant remote HTTP exchange removed through `9bfee34` | fix guide drift and remaining boundaries | 0, 4-5 |
| Participant role/mute/leave | implemented | add complete kick/re-add and room lifecycle actions before advertising them | 4 |
| General messages/history/edit/delete | implemented | retain canonical owner and final parity verification | 9 |
| General search/context/pins/attachments/votes | implemented | preserve distinct trust-boundary validation and remove only semantic duplication | 9 |
| Persona-card library and prompt use | implemented | final retained-flow verification after provider corrections | 8-9 |
| Ordered/ambient provider turns | implemented for four providers, reopened | remove transcript/fallback authority and repair cleanup/error ownership | 1-2 |
| Pause/resume/interrupt/stop | partly implemented | finish only against providers with exact native receipts; unsupported remains explicit | 2 |
| Friends | copied UI actively calls an absent Rust route | implement complete owner or hide until implemented | 5 |
| Custom text channels | settings/UI exist; message APIs are absent and poll | implement server message owner and event flow before enabling | 6 |
| Side chat | copied UI actively calls absent Rust routes | implement HTTP bootstrap plus WS events before enabling | 6 |
| Accounts/Google/recovery | local and central bootstrap are partial | implement exact account/handoff/recovery flows without bypass | 5 |
| Room Connector/current-session AI invite | copied control sends a payload the human-only Rust route rejects | implement distinct connector admission and MCP room-tool flow | 7 |
| External and managed AgentBridge | Rust enum/vocabulary exists, but no complete bridge admission/runtime owner exists | implement attendee and managed-process contracts separately | 7 |
| Provider login/usage/catalog refresh | UI/catalog claims exceed Rust operations | advertise and expose only implemented provider operations | 1, 8 |
| Runtime version/update/resources/release health | copied dormant/partly unreachable UI only | decide retained current product need, then implement or remove | 8 |
| Voice, Mafia, RimWorld | deferred by user | no active calls, polling, or core parity claim | deferred |

## Provider inventory

The original reachable provider catalog at `d504647` contained sixteen provider
entries. The older scripted-meeting adapter registry is not part of this list.
Neither is the separately labelled legacy one-shot API CLI/catalog. Stored-profile
compatibility branches for older Grok/Claude/transport shapes are evidence to omit,
not provider behavior to port. The Agent Session catalog below is the only parity
inventory.

| Provider | Verified original transport | Audited Rust state | Required target |
| --- | --- | --- | --- |
| Codex | persistent `app-server --stdio` | implemented; completion aliases/inference reopened | exact current app-server protocol, Codex-owned CLI config, no heuristic fallback |
| Antigravity | persistent PTY/ConPTY plus hooks | implemented with forbidden transcript polling | keep PTY/ConPTY and hooks; remove transcript and print completely; exact native signal or explicit incomplete |
| Grok | official ACP stdio | absent | add only when official installed client is available; preserve ACP contract |
| Claude | persistent Claude Code terminal/hook path | absent | use Claude Agent SDK as directed; no old transcript or print path |
| Cursor | persistent Cursor terminal/room portal | absent | reimplement the verified reachable current flow or record explicit unsupported evidence |
| Freebuff | persistent terminal runtime | absent | reimplement only its verified current reachable flow, without shared-terminal heuristics |
| OpenCode | owned loopback HTTP/SSE server | implemented; completion/cleanup authority reopened | Muse Spark default only when present; one explicit completion owner; exact child/peer custody |
| DeepSeek | official HTTPS OpenAI-compatible API | implemented | official Flash path, explicit credential owner, bounded complete turn, ordinary fixed-host TLS without a speculative DNS policy |
| Cerebras | HTTPS OpenAI-compatible API | absent | common remote API mechanism plus provider-owned catalog/header policy |
| OpenRouter | HTTPS OpenAI-compatible API | absent | common remote API mechanism plus provider-owned endpoint/catalog policy |
| Vercel AI Gateway | HTTPS OpenAI-compatible API | absent | common remote API mechanism plus provider-owned endpoint/catalog policy |
| LLM Gateway | HTTPS OpenAI-compatible API | absent | common remote API mechanism plus provider-owned model controls |
| TokenRouter | HTTPS OpenAI-compatible API | absent | common remote API mechanism plus provider-owned free-model/catalog policy |
| Custom API | caller-selected direct HTTPS OpenAI-compatible endpoint | absent | strict endpoint/credential policy and no local/private-network fallback |
| Ollama | local HTTP OpenAI-compatible endpoint | absent | explicit local owner and installed-model inventory |
| LM Studio | local HTTP OpenAI-compatible endpoint | absent | explicit local owner and installed-model inventory |

Provider modules share the smallest actual common mechanism: bounded catalog
projection, exact selection validation, normalized adapter lifecycle, RoomPortal
tool contract, credential redaction, and safe HTTP/process primitives. Provider
protocols, model controls, authentication, endpoint policy, completion signals,
and session identity remain provider-owned. No provider-name switch accumulates in
the frontend or a generic framework merely for possible future providers.

Alternate Codex/Claude/OpenCode/Pi harness installation over API/local models is a
separate deferred experiment. It does not justify an abstraction or compatibility
path in the core provider implementation now.

## Ordered implementation phases

### Phase 0 — freeze and correct the foundation

Audit-freeze substage:

- Classify the complete repository audit as `Fix`, `Consolidate`, `Keep`, or
  `Deferred/Unknown`, with one owner and exact evidence for each item.
- Correct current architecture/spec/exposure claims that contradict the audit.
- Cross-review the full previously unreviewed range, each commit, the cumulative
  diff, this master plan, and the finding register.
- Exit 0A: the inventory and finding map are approved as the implementation route;
  no product-code completion is claimed.

Phase 0A closed at reviewed content checkpoint `9711232`: critical-web Pro and
Daybreaker Blue High each returned `APPROVE — C0/H0/M0/L0` for the correction
chain and cumulative planning range. This approves the route, not any pending
product finding or Phase 0B implementation.

Correction substage:

- Remove the uncalled HTTP host-challenge/ticket bootstrap and its startup secret;
  keep desktop private-control issuance and authenticated remote-session issuance.
- Remove the remote proof because its key and frames cross the same HTTPS/WSS ingress
  authority. Treat both the local fresh-challenge receipt and local per-frame proof as
  unapproved until one controlled packaged reproduction, or equivalent concrete
  topology evidence, names the attacker capability and shows why native-owned child
  liveness/identity, an issuer-runtime-local one-use ticket, and separate exact ingress
  checks cannot fail closed.
  Separate private-control and loopback paths are a threat hypothesis, not that
  evidence. Without it, remove the proof key, receipt, and frame envelope and use the
  native-owned one-use ticket with ordinary bounded JSON. If evidence later justifies
  a receipt but not an active relay, stop at the one receipt; per-frame authentication
  requires its own evidence and, if retained, covers the Snapshot and every later
  bidirectional product frame as raw bounded UTF-8 bytes with one cached key and no
  base64, repeated derivation, or permissions digest. In every case preserve the finite
  snapshot/catch-up handshake, event sequence, request-ID deduplication, uncertain
  ACK recovery, product-surface equality, TLS/origin, ingress, and one-use socket
  ticket contracts.
- Let remote human HTTP routes authorize the session bearer once from the bounded
  Authorization header at the target boundary instead of exchanging a second purpose
  ticket for every request; never place it in a URL, body, log, event, or durable row.
  Keep desktop purpose tickets and socket-upgrade tickets because they cross
  distinct private-control and WebSocket boundaries.
- Remove or gate copied frontend promises that cannot reach a Rust authority.
- Keep the user-approved single room-search presentation and participant-ID avatar
  projection, but do not label lobby-only results as every readable channel. Until the
  custom-text owner exists, the visible scope must state the implemented lobby/current
  scope; the later custom-channel slice completes the true room-wide union.
- Remove false capabilities from the advertised surface until their actions exist.
- Replace the orphan frontend wire model and behavior-module CSS import chain with
  one generated protocol owner and one explicit stylesheet-order owner.
- Reconcile active frontend `maxLength` hints with domain limits. The server remains
  authoritative; only limits that represent the same product policy are generated,
  and an intentionally narrower UI limit is documented rather than duplicated as
  accidental policy.
- Consolidate only identical wire constants at their existing protocol owner.
  Issuance, parsing, canonical decoding, signature checking, authorization, and
  trust-boundary error handling remain independent.
- Remove the room-command request-ID compatibility fallback and use the existing
  secure fail-closed browser identity primitive.
- Close F-08 at the existing HTTP admission owner: controlled TCP evidence supports
  a 127-connection trusted-public partition inside the unchanged 128-connection
  total ceiling. Keep pre-header classification limits explicit; do not add a second
  listener, retry, timer, or fallback.
- Make closed provider-catalog authority terminal instead of busy-spinning each
  connected room socket.
- Prove a current finite consumer for every fixed human-invite token claim and remove
  unconsumed self-description; retain the room/server/lineage/scope/expiry and
  credential bindings that admission actually verifies.
- Measure the one-second unresolved-runtime reconciliation cadence before changing
  it; retain its recovery state machine and change only a cadence with observed
  idle cost and recovery-latency evidence.
- Exit Phase 0: every Fix, Consolidate, and defensive-complexity decision assigned to
  Phase 0—including D-07 Unknown—is implemented or explicitly deferred with evidence;
  its focused tests and affected repository gates pass; the master inventory,
  audit register, exposure map, architecture,
  active slice, and workboard agree on the resulting current behavior.

### Phase 1 — provider contract and process correctness

- Remove Antigravity transcript code from the production graph. If retained as the
  requested historical copy, keep it only in a non-build `deprecated/` boundary
  with no module/import/feature/test/runtime/fallback connection.
- Make observation abort, stop, portal teardown, hook cleanup, and process cleanup
  return typed outcomes to the common lifecycle owner.
- Verify Codex and OpenCode native completion/session contracts and remove
  unapproved inference/history fallback.
- Give the finite Agent Session state vocabulary one domain owner and repeated row
  encoding one entity-specific persistence owner without introducing a generic
  state-machine or repository framework. Lifecycle, turn, and reconciliation
  transitions remain with their distinct current authorities; provider observation,
  authorization, transactions, side effects, and boundary error mapping also stay
  local.
- Share only the pure Codex bundle identity/companion-name meaning; keep selection,
  commit-time revalidation, launch-time binding, staging, and open-file identity at
  their distinct TOCTOU boundaries.
- Correct catalog default selection, operation advertising, credential controls,
  tool descriptors, and provider-owned launch syntax.
- Restore one Rust-generated frontend wire contract for snapshot, live event,
  history, search, and provider-request decoding.
- Measure and bound the complete DeepSeek turn rather than multiplying only
  per-request timeouts across an unbounded user-visible wait.
- Remove the fixed DeepSeek host's custom public-IP DNS resolver; decide
  `.no_proxy()` only from an explicit credential/proxy policy and observed operating
  requirements. User-selected Custom API endpoints keep their separate SSRF owner.
- Remove the runtime handle's parsed-but-unused random UUID suffix while preserving
  platform, boot, lease generation, independently adoptable owner, and stale-CAS
  snapshots. Introduce a larger authority type only if it reduces actual state and
  glue after measurement.
- Exit: each enabled provider has exactly one completion/session authority and a
  visible failure/uncertainty contract, with no scraping, transcript, print, silent
  fallback, or ownerless cleanup.

### Phase 2 — finish exact Agent Session control

- Revalidate pause/resume/stop and the active busy-turn interrupt against the
  corrected provider contract.
- Keep unsupported interrupt unavailable until a correlated native quiescence
  receipt exists; never substitute Ctrl-C silence or process replacement.
- Implement re-add only with its complete participant/session transition and
  replay contract.
- Exit: copied controls advertise only exact implemented actions and pass durable,
  TCP/WebSocket, restart, and real-provider flows.

### Phase 3 — profile and asset SSoT correction

- Project human identity from the person profile everywhere, including search,
  timeline, custom channels, voice when later reopened, and the left-bottom card.
- Project Agent identity from the Agent Session and merge only room-owned
  membership fields from its participant.
- Give Agent avatar/profile mutation its own complete storage/ticket lifecycle or
  keep the editor unavailable until that boundary exists.
- Preserve current+pending profile replacement, pre-join transfer, room-owned
  appearance, message attachment lifetime, and exact-reference deletion.
- Exit: no profile, participant, Agent Session, or asset lifecycle overwrites
  another owner in source, restart state, or packaged UI.

### Phase 4 — room lifecycle and moderation

- Implement participant kick, room close/archive/delete, host claim where retained,
  and exact settings/lifecycle controls through the canonical command owner.
- Keep HTTP alternatives only where they serve a distinct bounded integration;
  browser live commands remain WebSocket-owned.
- Exit: advertised permissions intersect implemented actions and survive
  replay/reconnect/restart without client authority.

### Phase 5 — identity, accounts, friends, and human admission completion

- Finish account status, Google challenge/connect/delete, native handoff, recovery,
  friends, host, and operator-pairing flows that remain in the retained product.
- Preserve distinct local operator, admitted human, and central identity owners.
- Exit: every visible account/friend/invite control completes a real flow or is not
  exposed; no startup request targets an absent route.

### Phase 6 — custom text channels and side chat

- Implement one custom-channel message owner with history, events, search, and pins
  as separately completed slices; do not fake them with lobby data. The verified
  original composer is text-only, so custom-channel attachments are not added unless
  a separate reachable original flow is later proven.
- Implement side-chat bootstrap/mutation plus live WebSocket projection.
- Remove the current missing-route polling before either surface is enabled.
- Exit: each copied entry point has a complete backend owner and real packaged
  verification. Voice remains deferred and inactive.

### Phase 7 — external AI admission and bridges

- Implement Room Connector first as its own current-session MCP admission path:
  connector invite, agent participant identity, browser-like ordinary room-tool
  permissions, authenticated transport, reconnect, and leave.
- Bind the copied external-AI-session link to that Room Connector owner; it must not
  use or widen the human browser admission route.
- Implement external `assemble room attend` AgentBridge separately: attendee
  admission/principal, turn/report/provider-request protocol, reconnect, and
  cleanup.
- Bind both saved-AI-friend invitations and an admitted human's companion packet to
  this external AgentBridge owner. Their original
  `agent_attendee_entry_packet`/`assemble room attend` contract is not a human invite
  and is not the Room Connector path.
- Implement the server-managed AgentBridge process as a third custody boundary;
  process launch/stop is not authority for the external attendee or connector.
- Never widen the human invite payload or session to admit any of these agents.
- Exit: one real client for each retained path joins through its exact credential,
  receives only its authorized room view, uses permitted tools, publishes through
  the room owner, reconnects, and leaves/stops without leaking provider-private
  data or borrowing another path's authority.

### Phase 8 — remaining provider and operational surfaces

- Add Claude Agent SDK, then the rest of the original provider inventory in
  independently verified provider slices using the common mechanisms above.
- Implement provider login/usage/catalog refresh only where the actual provider
  supports it. Add runtime update/resources/release-health surfaces only after
  confirming they remain current reachable product behavior.
- Exit: the full retained provider matrix and all visible operational controls have
  source/static contract evidence. Installed and explicitly authorized providers
  additionally have exact real-client evidence and cleanup records; unavailable or
  unauthorized providers remain truthfully unavailable, never simulated.

### Phase 9 — final parity, performance, and repository cleanup

- Re-run the full exposure inventory and report backend-only, indirect/service-only,
  partial, and intentionally unavailable surfaces.
- Verify local and admitted-user packaged flows, restart/reconnect, all retained
  providers, responsive layout, right-panel/header/search behavior, and exact
  process/app cleanup.
- Measure CPU, memory, disk, task/process count, and latency at owning boundaries;
  optimize only observed costs or concrete threats and record before/after evidence.
- Remove stale regenerable build/verification artifacts after their active work
  ends, without touching user data or unrelated processes.
- Exit: every retained original user flow has authoritative Rust and real-flow
  evidence where its required external client/API is installed and explicitly
  authorized. Unavailable external dependencies have explicit static/contract
  evidence and a truthful unavailable state. Every excluded/deferred surface is
  inactive and explicit, both manual reviewers approve the exact pushed range, and
  no Python/legacy fallback remains.

At that exit, stop and report parity before beginning a repository-wide size-warning
cleanup. After the user explicitly resumes post-parity cleanup, review the existing
500+ LOC cohort for ownership drift and every 800+ LOC file as a strong split
candidate; production source above 1,000 LOC remains prohibited. LOC is only a
signal: keep a cohesive state/invariant owner when splitting would increase exposed
state, interfaces, dependencies, or glue, and split even a short file when domains,
authority, lifecycle, invariants, or change reasons differ.

## Per-slice execution gate

Before implementation, the slice owner records definition, current contract,
non-goals, authority/data ownership, failure and uncertainty states, concurrency,
storage/external protocol, acceptance, and verification. Each change is the
smallest independently buildable, verifiable, rollbackable commit under 1,000
changed lines. Do not wait for 500 LOC before separating responsibilities: a mixed
owner is split as soon as it appears, while a cohesive owner is not mechanically
cut to satisfy a count. Batch accumulation, push timing, and review timing are owned
only by the active `Standing project workflow` in `AGENTS.md`. This plan neither
restates nor reinterprets those thresholds.

When that workflow invokes review, the request covers individual commits and the
cumulative range. Requests
explicitly include structure, duplicated policy, overimplementation, ownership,
lifecycle, meaningless polling/heartbeat/timers, fallback, and swallowed failure.
For defensive code, reviewers must name the reachable use case, observed failure,
or in-scope threat and compare the smallest fail-closed alternative; future risk or
the word security alone is not approval evidence. Findings and final disposition
are recorded without copying full review prose into multiple product documents.
