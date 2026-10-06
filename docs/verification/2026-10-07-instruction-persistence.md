# Real-provider instruction persistence — 2026-10-07

Base: `21f344b9`, including `0bd36155`, `299f2edd` and `abbda15e`.
User explicitly authorized Codex/OpenCode compaction and Claude SDK Sonnet at
minimum effort, with strict turn/run budgets. This is managed-host room execution,
not a companion compaction claim. No production behavior changed.

| Provider | Actual compaction / resume | Room rule evidence | Result |
| --- | --- | --- | --- |
| Codex 0.154.0, `gpt-5.6-luna`, low | Seven native `context_compacted` events across two room turns | Each turn's first actual MCP call is `read_discussion`, followed by successful `publish_message`; two canonical `room_portal` publications, no runtime error | Pass after actual compaction |
| Claude Agent SDK, `claude-sonnet-5`, low | Continuation: SDK `/compact`, manual `compact_boundary`, 13,398 → 1,007 tokens in the same native session | Read/publish at native lines 12/15 before and 39/42 after compaction; two canonical publications | Pass after actual compaction |
| OpenCode, Muse Spark 1.3 contributor, Big Pickle, and continuation MiMo 2.6 Flash free | All first model requests rejected; no compaction | Native `APIError`, HTTP **403**, `isRetryable=false`; host `provider_turn_failed`. No tool calls/publications | Blocked; same error means no further Muse retry |

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
