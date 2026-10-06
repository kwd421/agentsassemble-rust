# Real-provider instruction persistence — 2026-10-07

Base: `21f344b9`, including `0bd36155`, `299f2edd` and `abbda15e`.
User explicitly authorized Codex/OpenCode compaction and Claude SDK Sonnet at
minimum effort, with strict turn/run budgets. This is managed-host room execution,
not a companion compaction claim. No production behavior changed.

| Provider | Actual compaction / resume | Room rule evidence | Result |
| --- | --- | --- | --- |
| Codex 0.154.0, `gpt-5.6-luna`, low | Seven native `context_compacted` events across two room turns | Each turn's first actual MCP call is `read_discussion`, followed by successful `publish_message`; two canonical `room_portal` publications, no runtime error | Pass after actual compaction |
| Claude Agent SDK, `claude-sonnet-5`, low | No `compact_boundary`; `agent.stop` then `agent.resume` between two observations in the same native session | Native calls are read, publish, read, publish; both canonical publications succeed, no runtime error | Pass for resume only; compaction unverified |
| OpenCode, `opencode/muse-spark-1.3-contributor-free` and `opencode/big-pickle` | Neither reached a successful model turn; no compaction | Each first request ends in native `APIError`, HTTP **403**, `isRetryable=false`; host reports `provider_turn_failed`. No tool calls/publications | Blocked by provider response; persistence unverified |

## Entry point and evidence

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
the requested `low` effort. No Opus invocation or compaction attempt was made.

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
