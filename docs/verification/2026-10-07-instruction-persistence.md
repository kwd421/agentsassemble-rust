# Real-provider instruction persistence — 2026-10-07

Base: `21f344b9`, including `0bd36155`, `299f2edd` and `abbda15e`.
User explicitly authorized Codex/OpenCode compaction and Claude SDK Sonnet at
minimum effort, with strict turn/run budgets. This is managed-host room execution,
not a companion compaction claim. The post-update correction below changes only
error presentation and update completion reporting.

| Provider | Actual compaction / resume | Room rule evidence | Result |
| --- | --- | --- | --- |
| Codex 0.154.0, `gpt-5.6-luna`, low | Seven native `context_compacted` events across two room turns | Each turn's first actual MCP call is `read_discussion`, followed by successful `publish_message`; two canonical `room_portal` publications, no runtime error | Pass after actual compaction |
| Claude Agent SDK, `claude-sonnet-5`, low | Continuation: SDK `/compact`, manual `compact_boundary`, 13,398 → 1,007 tokens in the same native session | Read/publish at native lines 12/15 before and 39/42 after compaction; two canonical publications | Pass after actual compaction |
| OpenCode 1.18.34, MiMo 2.6 Flash free (latest retry) | First requests rejected; no compaction | HTTP **403**, `APIError`, `isRetryable=false`, free-tier client rejection; Korean host guidance verified after fix | Blocked: 2 additional attempted turns, 5/6 historical total; zero publications |

## Initial run (d062326b): entry point and evidence

Added ignored test:
`agent_session_boundary::real_instruction_persistence::real_managed_instruction_persistence`.
It reuses the integration harness's local authority, catalog, managed adapter,
real room socket, `agent.create`, scheduler and room portal. Each run creates an
isolated empty workspace and in-memory host database and permits at most two
room turns. Messages ask for a short marker sentence without mentioning room
rules, tool names or reading. Host instructions are supplied by the production
assignment owner. Successful publication requires the portal's fresh same-turn
read receipt. Native transcripts separately establish first-call ordering and
compaction; a passing test alone does **not** certify compaction.

[Sanitized native evidence](2026-10-07-instruction-persistence.json) contains only
model/effort, event positions, tool names, error classes/statuses, token counters
and transcript hashes. No credentials, prompts, response bodies, private session
handles or raw provider transcripts are committed.

Codex native line order:

- Turn 1 starts at line 3; compaction at 23; `read_discussion` at 25;
  compaction at 35; `publish_message` at 37.
- Turn 2 starts at 55; compaction at 60 and 80; `read_discussion` at 82;
  compaction at 92; `publish_message` at 94.
- The remaining compactions at 47 and 104 precede native task completion.
  All four actual MCP calls succeed. Code-mode tool discovery is not counted as
  a room tool call.

Claude's one native transcript contains both ordered observations and the four
tool calls at lines 12/15 and 25/28. The SDK runtime's native Stop receipt checks
the requested `low` effort. In that initial run, no Opus invocation or compaction attempt was made.

OpenCode's two isolated native sessions each contain one user text part and an
assistant `APIError` with HTTP 403, no tool or compaction parts, and zero reported
input/output tokens. Response headers/body were inspected only for safe error
classification and were not printed or exported. No authentication/configuration
bypass or retry fallback was added.

## Configuration and reproduction

The existing Codex launch honors `CODEX_HOME`. This run used a private temporary
home with a private copy of the existing authentication/configuration and set
`model_auto_compact_token_limit = 12000` only there. The global files were not
edited. The [official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
defines this automatic-compaction threshold. No large room-history padding was
needed. The threshold was below the approximately 13k-token tool-bearing prompt,
so it triggered repeated compaction; this proves persistence but is not an
efficient recommended setting for later verification.

OpenCode's existing launch creates its own temporary config and overwrites
`OPENCODE_CONFIG_CONTENT`; it exposes no per-run auto-compaction override. Its
provider HTTP 403 also blocks a native summarization or padded-history test.
No global config was changed to force compaction. An initial discovery-only test
stopped before creation because the preferred 1.2 model was no longer listed;
subsequent tests explicitly selected currently listed models. That initial
selection failure consumed zero model turns and is not a provider error code.

Build the single integration harness, then invoke only the ignored test in a
separate process with `AA_VERIFY_PROVIDER=codex|claude|opencode` and optional
`AA_VERIFY_MODEL`. For Codex, supply an already prepared private `CODEX_HOME`.
For OpenCode, select a listed authorized model explicitly if no default exists.
Never run the whole ignored suite to reproduce this check. Environment selection
does not configure extra compaction support in the production adapter.

## Budget and limitations

- Codex: one test invocation, **2/6 room turns**, 7 compaction events, 104.52 s.
  Native cumulative usage reports 105,377 input tokens (67,840 cached), 603 output
  tokens, including 233 reasoning tokens. These counters are not a billing receipt;
  separate compaction billing is not established.
- Claude: one test invocation, two model-bearing session openings (initial and
  resume), **2 room turns**, 36.34 s. No additional real retry. Native assistant
  usage sums: 12 input, 13,757 cache creation, 66,756 cache read, 256 output tokens.
  Monetary cost was not measured.
- OpenCode: two model-bearing test invocations, **2/6 attempted room turns**, both
  first requests rejected (9.16 s and 9.25 s). One extra discovery-only invocation
  made no model request. Native reported cost/input/output are zero, not independent
  billing confirmation. Further attempts stopped at the external dependency.

Companion policy is covered separately by the existing deterministic
`attendee_client_execution::external_execution_reconnects_without_reentry_and_recovers_committed_report`
test: privileged instructions equal the fixed local policy, host instructions
stay in untrusted turn input, and reconnect retains exact execution custody.
No real companion compaction run, API provider, other model, or platform parity is
claimed. No product defect requiring a runtime patch was reproduced.

Local checks passed: the companion policy/reconnect regression (1/1), managed
room publication regression (1/1), integration-target Clippy with warnings denied,
architecture/source-growth checks and their 19 policy/artifact unit tests,
workspace formatting, diff checks and read-only artifact maintenance check.
Existing ts-rs and source-size advisory warnings remain. The new real-provider
test remains ignored in the normal suite. No full phase-closeout or push check
is claimed.

All real tests awaited managed server shutdown, including OpenCode failures. Only
this run's copied Codex home/configuration/authentication and private scratch data
are removed after sanitization. Native Claude/OpenCode test histories remain under
their existing provider-owned storage. User data, global config, `.agents/` and
`scripts/__pycache__/` are untouched. No push or external review request.

## Authorized continuation from d062326b

OpenCode 1.17.18 `opencode models opencode` lists the free model as
`opencode/mimo-v2.6-flash-free` (not an inferred model ID). One discovery-only
invocation failed with `model_discovery_timeout` / absent models in 10.30 s;
no agent or model turn started. The next invocation selected the exact listed
model, started idle, then failed its first room turn in 9.93 s. Native SQLite
message evidence is `APIError`, HTTP 403, `isRetryable=false`, zero reported
tokens, and one text part without tools or compaction. This is the same error
class/status/retryability as the previous Muse run, so Muse was **not retried**.
This continuation used **1 attempted room turn**, **3/6 cumulatively**.

The [OpenCode server API](https://opencode.ai/docs/server/#sessions) documents
`POST /session/:id/summarize` with `providerID` and `modelID`; native summarization
exists. The provider rejection prevented a successful baseline conversation, so
no extra summary request or low-threshold configuration was applied. Compaction
remains unverified, rather than being classified as unsupported.

Claude used **one additional run**, **2/3 runs cumulatively**, Sonnet 5 at `low`.
The test-only SDK decorator is staged through the existing
`AGENTSASSEMBLE_PROVIDER_RUNTIME` override. It submits `/compact` on the same
live SDK input queue after the first successful room result, waits for a native
manual compact boundary and successful command result, then forwards the original
room result unchanged to the production bridge. The host then submits its second
ordinary room turn; there is no stop/resume, new session, instruction replacement,
room-rule reminder, or product feature. Production receipt/effort/tool authority
validation remains active. This follows the
[SDK command contract](https://code.claude.com/docs/en/agent-sdk/slash-commands#compact-history-with-compact).

The run passed in **36.77 s**: **2 room turns plus 1 `/compact` input**. Native
history has a manual compact boundary at line 25 (13,398 pre-tokens, 1,007
post-tokens), and read/publish at 12/15 before and 39/42 after that boundary.
The SDK stream independently returned `compact_boundary` and a successful
command result in the same session. The native transcript records the command
input at line 28, after its compact boundary; use SDK stream order for command
submission, not the rewritten native line order. Both canonical `room_portal`
publications succeeded and the host reported no runtime error. Only Sonnet 5
appears in the native assistant model records. The bridge's unchanged Stop hook
verified applied `low` effort for the room turns; the summarizer's separate
internal effort is not reported. Room assistant token counters total 12 input,
8,504 cache creation, 74,641 cache read, and 132 output; these exclude separately
unreported summarization usage and are not a billing receipt.

Reproduce only with explicit real-provider authorization:

```sh
cargo test -p agentsassemble-server --test integration --no-run
python3 -B crates/agentsassemble-server/tests/agent_session_boundary/run_compact_sdk_fixture.py \
  <built-integration-executable> <sanitized-evidence.jsonl>
```

The helper creates/removes a private runtime bundle, keeps native SDK and bridge
behavior, and exports only allowlisted event/model/token fields. The test remains
`#[ignore]`; the ordinary invocation retains its original resume check. Evidence
is appended under `continuation` in the JSON, preserving the initial results.
No secrets, prompts, response bodies or session IDs are exported. Native provider
histories remain provider-owned; only the helper's temporary bundle is removed.
Global provider configuration, `.agents/` and `scripts/__pycache__/` are untouched.

Continuation local checks: integration target builds; targeted Clippy with warnings
denied, workspace formatting, JS syntax, architecture/source-growth checks and
19 policy/artifact tests and read-only artifact maintenance pass. Scoped diff
checks pass; a whole-workspace diff check reports whitespace in concurrently
modified frontend files outside this task, which are left untouched. Existing
source-size and ts-rs advisories remain.
No product runtime changes, new security scan, push, or phase-closeout claim.

## Post-update diagnosis and correction (2026-10-07)

Installed CLI is **1.18.34**. Read-only correlation of UI Check's `OpenCode MiMo`
record to its native OpenCode session confirms that the failed app session was
also created by **1.18.34**. Its error is free-tier client rejection, not the
minimum-version error seen in the direct older CLI. No existing app/provider
process was terminated and no global provider configuration was edited.

The resident lifecycle is per managed session: `OpenCodeDriver::spawn_inner`
launches a byte-bound `serve --pure` child; `reuse_owned_runtime` attaches to that
owned child. `ProviderUpdateService` refreshes the catalog after confirmed install
and does not restart existing runtimes. That is a lifecycle limitation, but it is
**not the cause of this failed session**. No speculative restart/invalidation or
mid-turn process interruption was introduced for this disproved hypothesis.

The failed execution uses a dedicated `agentsassemble_room` primary agent in
`opencode.rs` to retain privileged room instructions. Native rejection of this
managed request is established; the narrower custom-agent trigger is corroborated
by [upstream issue 53347](https://github.com/anomalyco/opencode/issues/53347), which
reports the same 1.18.34 behavior with custom primary agents. This remains an
upstream diagnosis, not an independently controlled A/B proof in this task.
We did not remove persistent instructions, switch agents/models, spoof headers or
bypass the free-tier check. A successful managed free-model turn remains blocked.

| OpenCode 1.18.34 / `opencode/mimo-v2.6-flash-free` | Turns attempted / completed | Compaction evidence | Result |
| --- | --- | --- | --- |
| Ignored test before correction, 8.97 s | 1 / 0 | 0 native compaction or tool parts | HTTP 403 free-tier client rejection |
| Same ignored test after correction, 13.45 s | 1 / 0 | 0 native compaction or tool parts | Same rejection; persisted host error has Korean guidance |

The two invocations stopped at their first turn, **2 turns in this task and 5/6
cumulative test turns** including the earlier three. The pre-existing packaged
failure is separate historical user activity. Native summarize is available as
previously documented, but no baseline request was admitted; spending another
model request on compaction would not establish persistence. Compaction is
blocked/unverified, not unsupported or passed. Both test processes awaited the
managed server's shutdown. No Muse retry or paid model was run.

Corrections: HTTP assistant-error envelopes and SSE session/message errors now
preserve only a bounded, redacted structured message. Version and free-tier
rejections have explicit Korean guidance; other messages follow Korean context.
Raw bodies/headers are never included. Old persisted English-only OpenCode errors
also receive Korean UI context. The internal transport label renders as OpenCode.
The update view retains `업데이트했어요 · <version>` instead of hiding it after
2.6 seconds; a fresh version check retains a prior confirmed completion receipt
only for the same installation with no newer offer. No polling or new processes.

Validation: provider library **293/293** (including OpenCode and owned updater
cases); affected frontend **32/32**, including retained completion beyond 35 s;
frontend TypeScript/Vite build; provider/server all-target/all-feature Clippy with
warnings denied; architecture/source-growth gates and **19/19** policy/artifact
unit tests; workspace formatting, diff check and read-only artifact maintenance.
Existing ts-rs, source-size and frontend chunk-size advisories remain.
The two ignored real tests failed at provider admission as recorded above; they
are not counted as passing tests. Packaged visual acceptance of these source
changes has not been performed; no rebuilt app,
real updater rerun, deployment, external review or push is claimed.

## Free-tier regression fix (2026-10-07)

This newly authorized regression run is separate from the five historical failed
room turns above. Installed `opencode --version` and the installed `opencode-ai`
package both report **1.18.34**. The adapter now explicitly selects native `build`
and writes fixed instructions to `room-instructions.md` in its existing private
per-session config root. `opencode.json` contains an absolute `instructions` path;
no custom agent definition, agent prompt override or per-message `system` fallback.
Changed or cleared instructions still rewrite files, dispose the same instance and
re-register the room portal before advancing the cached instruction value.
Unchanged instructions do not dispose. Existing session permissions remain intact.

Version-matched upstream evidence:

- [Config schema](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/core/src/v1/config/config.ts#L124)
  accepts `instructions` as an array of strings.
- [Instruction loader](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/session/instruction.ts#L122)
  resolves absolute paths even with project config disabled; `system()` reads
  their contents and returns system instruction text.
- [Prompt loop](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/session/prompt.ts#L1257)
  calls `instruction.system()` and passes it as `system` to the processor each
  iteration, including the next turn after compaction.
- [Built-in agents](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/agent/agent.ts#L141)
  mark `build` native and primary, with configured tool permissions. `plan` adds
  planning/edit restrictions, so `build` preserves the ordinary participant flow.
- [Summarize handler](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts#L273)
  creates native compaction with the current agent/model and executes its loop.
  `auto=false` avoids an extra automatic room continuation.

Actual results (new authorization; separate from historical runs):

| Path / model | Attempted / completed turns | Compaction evidence | Result |
| --- | --- | --- | --- |
| Managed ignored test / MiMo 2.6 Flash free, native `build` | 1 / 0 | No native compaction or tools | HTTP 403 `APIError`, non-retryable, free-tier client rejection; 8.47 s |
| Managed ignored test / Muse Spark 1.3 contributor free, native `build` | 1 / 0 | No native compaction or tools | Same rejection; 9.01 s |
| Isolated official CLI / MiMo, native `plan` | 1 / 1 | Not requested | Native step/text/step-finish, exit 0 |
| Isolated official CLI / MiMo, native `build` | 1 / 1 | Not requested | Native step/text/step-finish, exit 0 |

Total **4/6 attempted model turns**: two failed managed room turns and two
single-turn diagnostic CLI controls. The controls use no room instructions or MCP
portal, so they prove basic built-in admission, not managed instruction persistence.
Both managed native records explicitly identify `build`, installed version
1.18.34, the requested free model and zero reported input/output tokens. No paid
model was used. Counts are not billing confirmation.

The first managed turn failed in both runs, before the prepared summarize hook
could run. **Zero summarize calls; no post-compaction read/publish proof.** The
unexercised test-only hook was removed from the final minimal diff. Reproduce the
existing ignored test with `AA_VERIFY_PROVIDER=opencode` and `AA_VERIFY_MODEL`
set to an authorized free model. Once baseline admission works, native
`POST /session/:id/summarize` with `auto=false` is the documented cheap path.

The custom-agent regression is removed at its instruction owner, but this is
**not an accepted end-to-end free-tier fix**: built-in selection alone did not
resolve managed admission here. The remaining rejection trigger is unknown;
source inspection and CLI controls do not establish whether instructions, tool
shape, permissions or another managed-request property causes it. No header
spoof, authentication bypass, permission relaxation, global config change or
per-message fallback was added. Config `instructions` is supported by the source;
its failure to obtain model admission does not establish a loader failure.

Validation: provider library **293/293**; integration target build; provider/server
all-target/all-feature Clippy with warnings denied; architecture/source-growth
gates and **19/19** policy/artifact tests; workspace formatting, diff check and
read-only artifact maintenance. Existing ts-rs and source-size advisories remain.
Both failed managed runs awaited server shutdown. Native histories stay with the
provider; isolated diagnostic directories were removed. `.agents/` and
`scripts/__pycache__/` are untouched. Commit only, no push or external review.

## Managed launch bisect: bash tool admission boundary (2026-10-07)

Base `7c0439e3`; installed native OpenCode **1.18.34**. **No safe adapter fix
established.** A single session rule `{permission:"bash", pattern:"*",
action:"deny"}` changes the otherwise successful request to HTTP 403 with the
exact free-tier client rejection. Independently, message `tools:{bash:false}`
produces the same rejection. Denying only edits or only task delegation succeeds.
The minimal demonstrated trigger is removal of the native bash tool, not a custom
agent, isolated config, instructions file or the caller's HTTP client identity.

### Controlled requests and results

Each case launches its own `opencode serve --hostname 127.0.0.1 --port <reserved>
--log-level ERROR` in a new temporary directory, waits for `/global/health`, creates
one session with `POST /session`, then posts one message. All use `agent:build`,
model `{providerID:opencode, modelID:mimo-v2.6-flash-free}`, and one text part:
“Reply only OK. Do not use any tools.” Returned message parts contained no tool
calls (full native histories were not audited). Successful cases returned text; failures returned native `APIError`, status 403 and the
free-tier-client message. Response bodies and credential/header values are omitted.

| Case | Cumulative addition unless indicated | Result | Message seconds |
| --- | --- | --- | ---: |
| 1 | Plain serve / default config | PASS | 5.60 |
| 2 | XDG_CONFIG_HOME = isolated root | PASS | 35.08 |
| 3 | OPENCODE_CONFIG_DIR = same root | PASS | 4.81 |
| 4 | OPENCODE_CONFIG_CONTENT = empty plugin list and MCP map | PASS | 3.49 |
| 5 | OPENCODE_DISABLE_PROJECT_CONFIG=1 | PASS | 5.14 |
| 6 | OPENCODE_DISABLE_DEFAULT_PLUGINS=1 | PASS | 4.61 |
| 7 | OPENCODE_DISABLE_EXTERNAL_SKILLS=1 | PASS | 4.12 |
| 8 | OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1 | PASS | 4.10 |
| 9 | OPENCODE_PURE=1 and --pure | PASS | 5.50 |
| 10 | Config instructions array pointing to private instruction file | PASS | 3.01 |
| 11 | Exact managed meeting_read_only session permissions | 403 client rejection | 1.37 |
| 12 | Case 10 + only bash wildcard deny | 403 client rejection | 2.36 |
| 13 | Case 10 + only edit wildcard deny | PASS | 30.75 |
| 14 | Case 10 + only task wildcard deny | PASS | 14.39 |
| 15 | Case 10 + message tools.bash=false (no session deny override) | 403 client rejection | 1.28 |

The instruction control says “Reply briefly. Do not use tools for this diagnostic.”
The exact managed policy is wildcard deny, allow read/glob/grep/list and
agentsassemble_room_*, deny external_directory. The adapter itself does not override
HOME, model/provider config or other XDG roots. It preserves allowed environment
variables, sets server credentials and sends model selection in HTTP bodies.
The hand probes use inherited environment and no server authentication. They do
not reproduce the guardian, MCP registration or all host instructions. Those
remaining differences are covered only by the failed managed test below; the
single-field paired controls already reproduce the rejection without them.
No upstream header identity was forged or varied. Broad system/parts/agent/model
permutations were unnecessary after the one-field trigger reproduced twice.
Some isolated instance initialization was slow; timings above cover only the
message request, not process startup or config initialization.

### Version-matched source evidence and its limit

- [permission/index.ts:204–212](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/permission/index.ts#L204)
  marks tools disabled for a matching global deny; edit/write/apply_patch share
  edit permission.
- [session/llm/request.ts:210–215](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/session/llm/request.ts#L210)
  removes both permission-disabled tools and message tools set to false before
  constructing the actual model tool set. These are the two observed failing paths.
- [request.ts:187–205](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/session/llm/request.ts#L187)
  generates native OpenCode session/request/client and User-Agent headers itself;
  [runtime-flags.ts:56](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/opencode/src/effect/runtime-flags.ts#L56)
  defaults the client flag to cli. The same native header path runs for passing
  and failing permission variants; caller HTTP headers are not forwarded here.
- [Console inference-proxy.ts:90–93](https://github.com/anomalyco/opencode/blob/v1.18.34/packages/console/app/src/lib/inference-proxy.ts#L90)
  forwards inference and identifies destination ownership of authentication.
  The exact free-tier error string/admission predicate was not found in the
  downloaded v1.18.34 source. **The bash tool admission requirement is observed
  behavior; the proprietary Console classifier remains unknown.**

### Security stop and managed persistence result

Re-enabling bash, changing deny to ask/allow, or introducing a fake tool would
change the required security contract. None was applied. Per the user's explicit
stop condition, the adapter remains unchanged; this is a documented dependency
block, not a successful fix. An upstream admission change that accepts disabled
bash would be needed to establish compatibility without relaxing current policy.
No new fallback, provider identity override or global configuration change.

The ignored integration test was rerun with `AA_VERIFY_PROVIDER=opencode` and
`AA_VERIFY_MODEL=opencode/mimo-v2.6-flash-free`:

```sh
cargo test -p agentsassemble-server --test integration \
  agent_session_boundary::real_instruction_persistence::real_managed_instruction_persistence \
  -- --ignored --exact --nocapture
```

Discovery and managed start succeeded. Turn 1 failed with `provider_turn_failed`;
Korean error guidance assertion passed. Test exit **101**, duration **9.09 s**;
**1/6** authorized room turns attempted, zero completed. No summarize request was
sent because the baseline failed. Post-compaction read_discussion/publish_message
remains unverified. This run is separate from all earlier failed runs.

Diagnostic budget: **15/approximately 15 explicit message requests**, 12 passing
and 3 rejected; not a count of OpenCode's internal inference/title/summary calls
or a billing assertion. Only the authorized free MiMo model was explicitly
selected. Every diagnostic owned server was stopped and its temporary working
/config directory removed; provider-native histories were preserved. No unrelated
processes, global config, `.agents/` or `scripts/__pycache__/` were changed.

Documentation-only change: JSON parsing and diff check passed; mandatory
architecture/source-growth gates, 19/19 policy/artifact tests and read-only
artifact maintenance passed (existing size advisories remain). The 642-line JSON
retains one cohesive dated verification record; no source boundary was added.
No product test pass or compaction acceptance is claimed from this blocked run. Commit only, no push.
