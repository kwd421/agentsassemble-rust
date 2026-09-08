# Operational surfaces

Status: Phase 8 contract established after Phase 7 approval at `d70224b`.
Implementation and local acceptance are pending. Real-provider execution and both
final reviewers remain governed by the product plan's final closeout gate.

## Definition and current reachable contracts

Preserve supported provider login, account usage and explicit catalog refresh.
Preserve confirmed runtime diagnostics and update/restart operations. Operational
metadata is not permission to start a model turn or expose host credentials.

The original checkout `d504647` supplies these reachable entry points:

- `web/routes/providers.py` mounts local-operator provider login and catalog refresh,
  plus credential-authorized account-usage reads. `providers/login.py` runs only
  provider-defined login commands: Codex, Claude, Grok and Cursor use browser OAuth;
  OpenCode opens its interactive authentication UI and reports started, not authenticated.
- Agent creation and member usage are ordinary frontend controls. Original
  `provider_usage.py` dispatches provider-specific readers. Codex reads app-server
  rate limits; DeepSeek reads its authenticated balance endpoint. Claude's original
  terminal reader can be replaced by the already-pinned SDK's structured usage API.
- Grok terminal-screen extraction and OpenCode Go HTML extraction are original
  implementations, not permission to introduce scraping under the current Rust
  contract. Their operational capabilities require a supported current native/API
  owner; absent evidence must remain explicitly unsupported, never fabricated usage.
- `web/routes/observability.py` mounts resource and release-health projections.
  `assemble release-health list/run` selects fixed local checks, bounds their
  execution and persists a latest report. Retain this CLI behavior using current
  Rust/frontend verification owners instead of old Python check implementations.
- `assemble frontend-info` inspects the actual served build. The mounted original
  `FrontendUpdateNotice` compares the current served build and protocol identity.
  `assemble rolling-restart` and `web/routes/runtime.py` expose status/request/wait;
  the original POSIX owner hands off the listener after quiescence and reconstructs
  eligible Agent Sessions. It does not transfer live provider process handles.
- Copied `AdminPanel` has no opener in either original or current production graph.
  It must be connected to confirmed diagnostic owners or removed in this phase;
  its lazy import, dead state and pollers are not a retained user flow by themselves.

The fourteen managed providers remain canonical. Freebuff and managed Antigravity
are excluded. External Antigravity remains a Room Connector participant and has no
app-managed login/usage/runtime operation. Voice, Mafia, RimWorld and the scripted
meeting runner are outside this phase. No new account, purchase, usage reset,
credential import, browser-cookie extraction or provider simulation is introduced.

## Authority, state and lifecycle

Provider registration owns advertised login and usage capabilities. Runtime
availability and operation support are separate facts. Existing catalog discovery
owns explicit refresh and publication; concurrent refreshes join one operation,
and shutdown cancels and waits for discovery. A failed refresh is visible rather
than silently publishing an old catalog as newly discovered. Selected attendee
discovery must remain scoped to its one explicit provider.

Local operator HTTP tickets and existing request-origin/body bounds protect
host operations. Remote admitted users cannot initiate host login, refresh or
account usage. Credentials remain in their existing provider store/environment;
only normalized quota/balance results reach the local operator. Provider-native
account IDs, tokens, raw diagnostics and authentication URLs containing secrets
must not enter room events or logs. Room participants retain ordinary room authority.

Browser OAuth login has one bounded process owner, explicit cancellation and
cleanup, and success only after the provider command completes successfully.
Interactive authentication reports launch separately from completion. Catalog
refresh follows successful authentication or an explicit operator action. Failure,
unsupported operation, missing executable, unavailable account and cancellation
remain distinct. No command is selected from caller-provided shell text.

Usage reads run on demand without a model turn. Native transport parsing and
cleanup reuse their existing provider owners where possible. Bound execution,
output and cancellation; coalesce concurrent identical reads rather than spawning
unowned work. Any retained cache must expose its observation time and stale/error
state. Missing or null quota values never mean zero usage or unlimited remaining.
Codex's multiple limit buckets and Claude's non-subscription case remain distinct;
DeepSeek monetary decimal strings retain their exact meaning and currency.

Runtime build identity and update state belong to the serving runtime, not a
frontend timer or a client-created release flag. Preserve assets used by admitted
clients while a replacement build becomes current. Restart requires quiescence,
preserves durable room/account/session configuration and bound address, and waits
for positive old-provider cleanup before recovery. Desktop parent/sidecar custody
must survive the transition; listener handoff cannot create an unowned replacement.
Busy, unsupported platform, missing build, readiness failure and uncertain cleanup
must be observable. Runtime stop is not a claimed successful rolling restart.

Resource reads use the existing maintained process library and bounded typed
projections, with observation time and unavailable metrics. Do not expose process
arguments, environment or user paths. Release health selects only fixed current
verification commands at their owning repository; no arbitrary remote shell or
new automated security scan. A report stores actual outcomes, including timeout,
failed and not-run, and failures to save are not reported as durable success.

## Phase dependency order and acceptance

1. Add operation capabilities to provider registration and explicit refresh to the
   catalog lifecycle. Wire one real local operator refresh path through the API and
   agent editor. Verify single-owner concurrency, cancellation and revision updates.
2. Connect supported login and normalized usage through the same authority boundary
   and visible creation/member controls. Cover every retained provider with static
   capability evidence; unavailable runtimes remain truthfully unavailable.
3. Complete runtime build/update/restart and resources/release-health owners and
   their retained CLI/HTTP/frontend entries. Resolve the copied Admin surface using
   these actual owners. Do not harden one provider while these targets are absent.
4. Verify the phase as a whole through direct packaged desktop/mobile manipulation
   with explicitly labeled local protocol fixtures. Exercise denied remote access,
   operation failure/cancellation, catalog refresh, usage rendering, diagnostics,
   restart/reconnect and update notice. Confirm exact app/child cleanup and remove
   only run-owned data/regenerable artifacts after work ends.
5. Run affected tests, builds and mandatory unchanged gates. Measure CPU, memory,
   process/task count, disk and latency at affected owners; optimize only concrete
   costs. Obtain Daybreak Blue `xhigh` approval for each commit, cumulative Phase 8,
   exact final HEAD and complete resulting local contract before Phase 9.

Final real runs remain configured DeepSeek, Codex, OpenCode, external Antigravity,
Grok and Cursor `auto` only. Claude and other providers receive static/native
contract and local fixture verification without actual account/provider execution.
Local Phase 8 acceptance does not claim the final authorized real-client matrix.
