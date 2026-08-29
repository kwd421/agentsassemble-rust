# AgentsAssemble

AgentsAssemble is being reimplemented as an asynchronous Rust runtime.

Preserve all currently reachable product behavior.
Reimplement the product, not the Python source tree.

## Product scope

Do not reimplement the old v0 scripted-meeting runner. This exclusion covers an
`assemble demo`-style execution path, pre-meeting per-agent research orchestration,
`research_focus` and smoke/standard/deep research steering, isolated
`private_research/*` artifacts, generated agendas, forced numbered rounds and speaker
order, `own_research`/`public_debate` phase switching, automatic moderator synthesis,
automatic decision-making or automatic task assignment, the v0
`agenda.md`/`decision.md`/`tasks/*.md`
and research-JSON artifact system, its templates/configuration/seeds/tests/docs, and
the v0-only `run_research`/`run_round`/`synthesize` adapter contracts and
Research/Round/MeetingRecord models. Their presence in the Python tree or old product
markdown is not a migration requirement.

This exclusion does not remove persona-card import and explicit selection (Risu,
CCv3, or CHARX) or its prompt application; ordinary `ordered` and `ambient` room
conversation; agent-initiated web search and tool use when permitted; room-owned
participant roles and permissions; ordinary message history, search, and pins; or a
normal conversation in which a human asks participants for synthesis, a decision,
task planning, or task-assignment discussion. These remain product behavior and must
not be coupled to a scripted meeting pipeline.

Use mature maintained libraries for solved infrastructure.
Reuse mechanisms; implement AgentsAssemble product semantics.
Do not reinvent frameworks, protocols, cryptography, database drivers, WebSocket framing, routing, async runtimes, serialization, pooling, or generic concurrency primitives.

Prefer the smallest boring design that fully works.
Security takes priority over convenience.
For each completed slice, inspect measurable CPU, memory, latency, task/process, and disk
costs. Establish an observed cost or concrete threat before optimizing; intuition and future
extension alone are not evidence. Remove avoidable work and copying at the owning boundary,
but do not trade product semantics, security, or maintainability for speculative optimization.
Before adding code, state, or abstraction for performance, security, or extensibility, check
whether the existing owner or a smaller design can preserve the same complete contract.
Record every material optimization alongside its implementation where reviewers can find it:
the prior cost, symptom, or threat evidence; the intent and owning boundary; the product and
security invariants preserved; the accepted trade-off; and the measurement or verification
result. Code that is faster or more elaborate but whose necessity cannot be reviewed is not a
completed optimization.

Fallbacks are forbidden by default.
When a path fails, find and fix the root cause.
Do not introduce new fallback behavior without explicit user approval.

Never narrow a migration slice by substituting placeholder data, fake authority,
disabled synchronization, authentication bypasses, compatibility shims, or client-side
orchestration for an original server-owned contract. If preserving the reachable behavior
requires a larger implementation boundary, expand the work and implement that boundary
before calling the feature complete. An incomplete path must remain explicitly incomplete;
passing tests or a superficially working screen is not parity. Completion requires the same
reachable entry point, authority owner, state transition, retry/failure semantics, and real
user flow as the original product, with only the internal language and infrastructure changed.

Do not add tests merely because code changed.
Keep a small number of high-value tests for meaningful contracts and failure modes.
Do not optimize for test count or coverage.

Repository architecture and structure gates are mandatory.
Do not weaken, bypass, raise, or add exceptions to a gate merely to make an implementation pass.

Read `Rule.md` before implementation.
For substantial design or reimplementation work, follow `SDD.md`.
Read `WORKBOARD_GUIDE.md` only when creating or restructuring the workboard.
When a workboard exists, route the active work from this file to that workboard.

Do not treat old product markdown as authority.
Determine current behavior from actual reachable code and real product flows.
After a feature is cut over, its Rust contract and verified user flow are authoritative over the replaced Python implementation.

Do not commit or push unless explicitly requested.
Do not run real providers, kill arbitrary external processes, delete user data, or perform destructive migrations without explicit approval.
Do not modify, revert, overwrite, or move user-owned uncommitted work.
Never expose credentials, tokens, secrets, or provider-private data through logs, events, prompts, fixtures, or committed files.

## Standing project workflow

The user has explicitly authorized scoped commits and pushes for this reimplementation.
Commit each independently buildable, verifiable, and rollbackable feature change as its
own sub-1,000-line commit. Keep completed commits local until either three feature commits
have accumulated or their aggregate insertions plus deletions since the last reviewed
baseline reach 2,000 lines, whichever happens first. Then push that batch and request
external review of the exact pushed range. A correction required to close an already-open
review remains part of that review batch and may be pushed and re-reviewed immediately.
This authorization does not cover unrelated repository changes.

Review requests to the designated critical ChatGPT web session are pre-authorized.
Send each request as one complete message without asking the user again, never use an
early-response or `Get answer now` control, and read the completed response before
continuing. If the session reaches its length limit or errors irrecoverably, transfer the
user-authored requirements, decisions, and critical-review role to a new session. Start a
replacement review session with Pro reasoning until its plan is approved, then explicitly
switch and verify very-high reasoning for subsequent reviews.

Cross-review every pushed batch with that web session and Daybreaker Blue High. Reviewers
must inspect both the individual commits and their cumulative range. The user has standing-
approved Codex Security Standard Scan for the Daybreaker review; use that single-pass scan
instead of a manual Daybreaker review. Never use Deep Scan. Other automated security scans
still require explicit user approval.

Use Computer Use only during active packaged-frontend verification. When verification
ends, normally quit the exact app and its owned children, reset the Computer Use session,
and remove only the isolated verification data and regenerable artifacts created for that
run. Never stop unrelated applications or providers.
