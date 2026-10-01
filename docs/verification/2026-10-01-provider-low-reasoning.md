# Actual providers with minimum reasoning — 2026-10-01

User requested Cursor, Grok, Codex Terra, external Antigravity and configured
DeepSeek execution with minimum reasoning, plus Gemini web MCP availability.
Source: `c1d4615e`, branch `codex/recovery-and-sonnet`. No product code or provider
installation was changed. Used the existing final signed macOS verification
package, a fresh isolated guest/profile, and new room `room-20261001T101635`.
This is macOS and same-computer external CLI evidence.

## Actual results

| Provider | Actual selection | Result |
| --- | --- | --- |
| Cursor | Intended Auto; installed CLI `2026.08.11-e8db854` | Blocked before selection/start: the app's real catalog lookup reports `provider returned malformed model data`. No successful Cursor generation is claimed. |
| Grok | Installed `1.0.34`, `grok-4.7`, Low, room-read-only permission | Real room read and correct publication, then idle, Stop, same native session Resume, idle and Stop. |
| Codex | Installed `0.154.0`, `gpt-5.6-terra`, Low, room-read-only permission | Real room read and correct publication, then idle, Stop, same native session Resume, idle and Stop. |
| DeepSeek | Configured API, `deepseek-flash`, low, Thinking disabled | Real room read and correct publication, then idle, Stop, Resume, idle and Stop. |
| Antigravity | External CLI `1.2.14`, `gemini-3.8-flash-low`, `--effort low` | Ordinary interactive CLI, with individual MCP approvals: two-stage join, read, correct publication, committed leave and receipt release. |

Canonical stored replies independently agree with the host UI:

- seq 12: `PROVIDER-LOW-OK grok-4.7 nonce=94c7a1 sum=73`.
- seq 30: `PROVIDER-LOW-OK Codex Terra nonce=94c7a1 sum=73`.
- seq 49: `PROVIDER-LOW-OK DeepSeek nonce=62b48e sum=83`.
- seq 61: `PROVIDER-LOW-OK Antigravity nonce=cf58b2 sum=91`.
- seq 62: Antigravity `participant_left`.

The request prompts supplied marker names, not nonce values. DeepSeek and
Antigravity each received a new host-only nonce. Grok and Codex used the same
initial host probe, so their shared nonce does not independently prove isolation
from one another. No alternate room or long-idle recovery claim is made.
All three managed sessions finish detached/stopped, with one completed turn,
no last error and no recovery requirement. Grok and Codex retain native-session
reuse; DeepSeek's API does not have a reused native CLI session.

## Cursor diagnostic limits

Installed CLI `status --format json` returns authenticated with both access and
refresh-token presence. A separate native ACP diagnostic initializes successfully
but `cursor/list_available_models` returns `-32000: Authentication required`.
It performs no authentication RPC and is not an instrumented trace of the app's
staged executable. It narrows the failure to the ACP/catalog path but does not
prove the exact owner-level repair. The diagnostic-owned process exits.
Neither a provider update nor an authentication/catalog bypass was applied.
The September 15 successful Cursor record is historical and does not certify
today's installed configuration.

## Antigravity user flow

Initial noninteractive `--print` calls report outer `SUCCESS` with an empty
response and a denied MCP action. There is no corresponding room admission.
The current CLI explains that headless mode cannot ask for MCP permission.
This is a permission refusal, not evidence that ordinary CLI participation fails.

The subsequent ordinary CLI trusts only the empty test workspace and presents
native MCP approval cards. Each of the two `room_join` calls, `room_read`,
`room_say` and two `room_leave` calls is approved once. No global auto-approval,
persistent tool grant, app-managed Antigravity session or public tunnel is used.
The app-issued local invitation and app-owned HTTP `/mcp` endpoint are used.
The CLI also reads its own tool-definition/cache files, without accessing user
workspace files. A temporary server permission rule tried during headless
diagnosis was ineffective and removed before the ordinary CLI test.
The actual native tool-result cache independently contains `connection_prepared`,
`joined`, publication/leave `committed` and final `receipt_released`; only status
terms are exported. The ordinary CLI exits 0. Its test MCP registration is removed
and global CLI settings are restored byte-for-byte, including removal of the
test-only trusted workspace. The native CLI's own test conversations are retained.

## Gemini web

Google's current official documentation supports linking a custom MCP URL in
the Gemini web app, for use in chats and tasks. The published requirements are
US availability, age 18+, a personal Google Account, English and Keep Activity
enabled. Dynamic Client Registration or advanced client credentials are described
for servers that require registration.

- [Connect and manage custom apps](https://support.google.com/gemini/answer/17209137).
- [Use and manage Connected Apps](https://support.google.com/gemini/answer/13695044?co=GENIE.Platform%3DDesktop&hl=en).

In the actual signed-in in-app-browser account, Settings → Personal Intelligence
→ Connected Apps opens normally. The full current page has no Custom apps / Add
a custom app / MCP URL controls. No account privacy/region setting is changed and
no MCP registration or model generation is attempted. Therefore Gemini web
supports this class of integration in eligible accounts, but this account's real
connection to AgentsAssemble remains unverified. API/CLI support is a separate
contract and is not substituted for web evidence.

## Evidence and scope

Sanitized evidence: `/tmp/aa-providers-20261001/evidence/` includes native success
screenshots, Cursor failure/authentication/catalog-shape evidence, public canonical
events/session states and Gemini availability/screenshot evidence. Invitations,
recovery credentials, provider transcripts, private handles and database copies
are excluded from this document and the public evidence export.

One real generation each is verified for Grok, Terra, DeepSeek and interactive
Antigravity. This does not verify Windows/Linux, multiple sequential generation,
long idle, crash recovery, provider updating or remote-machine/public ingress.
Verification-only documentation changes require no product build/test rerun.

## Cleanup

Managed sessions are stopped and Antigravity has committed departure and released
its receipt. The exact desktop/supervisor/server and ordinary test CLI PIDs are
absent. Public ingress was never opened. The Gemini tab is closed and Computer
Use reset. Only this run's bundle, fresh isolated profile/cache/WebKit and private
verification files are moved to `~/.Trash/aa-providers-low-20261001`.
The earlier verification profile in Trash and ordinary user data are unchanged.
Sanitized evidence remains in `/tmp/aa-providers-20261001/evidence/`.

## Cursor and Gemini follow-up

User subsequently authorized fixing Cursor and researching remaining Gemini web
routes. Base `66d370e8`; installed provider version is unchanged.

### Cursor diagnosis and correction

The native status command reported authenticated/token presence, but real ACP
startup and `cursor/list_available_models` returned authentication required.
Explicit `authenticate(cursor_login)` then waited for browser login: native logs
confirm its saved-login check was false and it entered the browser-login flow.
The CLI status result therefore was insufficient evidence of usable ACP auth.

Completed the official `cursor-agent login` flow with the existing account. Native
login exits 0, its web page says All set, and fresh ACP returns 43 real models,
including `default` / Auto with no configurable model parameters. That fresh
startup succeeds before an explicit authenticate request; adding unconditional
ACP authentication/browser login is not necessary to repair this incident.
The documentation's normal authenticate step alone was not the proven root cause.

Corrected the response owner: ACP `AuthRequired` becomes a fixed, sanitized
`authentication_required` driver failure, and Cursor discovery preserves it as
login required. Other extension failures and malformed catalogs retain existing
failure behavior. Provider error message/data are not forwarded. No synthesized
catalog, compatibility fallback, credential extraction, auto-login, provider
update, extra process, polling or retry was added; existing deadlines remain.
A native-wire regression distinguishes auth rejection from internal failure and
checks that private provider error text is not exposed.

### Actual corrected packaged flow

Built a fresh debug package with identifier `app.agentsassemble.cursorfix20261001`
and Developer ID signature; strict/deep signature verification passes. This is
macOS/same-computer evidence, without notarization or distribution claims. A fresh
isolated guest creates `room-20261001T110430`; ordinary profiles are untouched.

- Actual app discovery shows Auto normally. Native Auto has no reasoning control;
  the UI correctly disables the control at Default. No Low override is invented.
- Select Auto, room-read-only permission and an empty test workspace; Start reaches
  idle. No provider installation update is accepted.
- Host writes the test instruction at seq 6. Cursor requests `read_discussion` and
  `publish_message`; each receives Allow once through the app's normal request UI.
  No persistent/global grant is used.
- Canonical seq 16 and the host UI agree on exactly
  `CURSOR-AUTO-OK nonce=5f6c82 sum=97`; seq 17 completes the turn and returns idle.
  The host message itself triggers this turn, so no undisclosed-input isolation
  claim is made for this follow-up.
- Stop at seq 19, Resume at seq 22 using the same native session, return idle,
  then final Stop at seq 24. Final state: detached/stopped, one completed turn,
  native session reused, no last error and no recovery requirement.

Provider all-feature tests pass 290/290; the final wire regression is rechecked
after lint corrections. Workspace all-target/all-feature warning-denied Clippy,
architecture/source growth, 19 policy cases, format, diff and artifact gates pass.
Frontend production compilation and the signed desktop package build pass; no
unrelated frontend/full-server suite claim is made. The process snapshot at resumed
idle is instantaneous evidence, not a performance benchmark. All ten app/provider
processes in that snapshot, including native helpers, are absent after normal
Stop and Quit; the native login process is also absent.

Sanitized evidence: `/tmp/aa-cursor-fix-20261001/evidence/`, including
`native-auth-catalog.json`, `cursor-auto-settings.png`, `cursor-auto-success.png`,
`cursor-final-stopped.png`, `canonical-results.json`, process and cleanup receipts.
The official login refresh is intentionally retained. Only this run's isolated
bundle/profile/cache/WebKit/private files/empty test workspace are moved to
`~/.Trash/aa-cursor-fix-20261001`. Native provider test conversations are retained.
Public ingress is never opened, test browser tabs are closed, Computer Use reset.

### Gemini web remaining routes

In the actual account, `https://gemini.google.com/apps?hl=en` displays English but
still has no custom-app/MCP URL field. Expanded sidebar has no Switch to Spark
control. Keep Activity is already On; it is not the observed missing prerequisite.
No account privacy, region, subscription or connected-app setting is changed.

Current [custom-app help](https://support.google.com/gemini/answer/17209137) still
requires age 18+, US, a personal account, Keep Activity and English. Registration
is Settings -> Connected Apps -> Custom apps -> MCP URL. Current canonical help
permits chats/tasks; older localized/search-cached wording says Spark tasks only.
No documented alternate add-link or Gems configuration enables an arbitrary
personal MCP endpoint for this current account. A URL pasted into a chat is not
actual authenticated tool participation.

[Current Spark help](https://support.google.com/gemini/answer/17094507) and
[July rollout notes](https://support.google.com/gemini/answer/17171264?hl=en)
describe broader country availability and Pro/Ultra access. That does not establish
removal of the explicit US/English custom-MCP conditions. An upgrade is not a
verified remedy for this account. Spark browser interaction is a different
possible contract and is not exercised here; API/CLI support is also not web proof.
Actual AgentsAssemble interoperability for a future eligible Gemini account
remains unverified. Evidence: `gemini-check.json` and
`gemini-english-connected-apps.png`; no Gemini model generation is performed.
