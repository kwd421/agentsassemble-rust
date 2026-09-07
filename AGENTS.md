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

## Implementation

Use mature maintained libraries for infrastructure; implement only product semantics.
Make the smallest complete change. Read `Rule.md` for implementation constraints and
`WORKBOARD.md` for the active task and relevant document sections. Follow `SDD.md`
for substantial design or reimplementation; read `WORKBOARD_GUIDE.md` only when
creating or restructuring the board. Do not load unrelated documents or history.

Discover behavior from reachable code and real flows, not old product markdown.
After cutover, the Rust contract and verified user flow are authoritative.
Completion preserves the reachable entry point, authority owner, state transition,
retry/failure semantics, and real user flow. Expand the implementation boundary
when necessary; placeholders, fake authority, disabled synchronization, authentication
bypasses, and client orchestration cannot replace a server-owned contract.
Continue through implementation, affected verification, and correction until the
active acceptance criteria are met or an actual authorization/dependency blocks work.

## Permissions and gates

- Security takes priority. New fallbacks and compatibility shims require explicit
  user approval; fix failures at their owner instead of hiding them.
- Architecture and structure gates remain mandatory. Do not weaken, bypass, raise,
  or add exceptions to pass. Changes to blocking gates require prior owner approval
  and deterministic actionable failures.
- Real providers, destructive migrations, deletion of user data, and termination of
  external processes require explicit approval unless already authorized for this task.
  Preserve user-owned uncommitted work outside the requested edits.
- Never expose credentials, tokens, secrets, or provider-private data in logs,
  events, prompts, fixtures, or committed files.

## Project workflow

Scoped reimplementation commits and pushes are pre-authorized; unrelated changes
are not. Keep feature commits independently buildable, verifiable, rollbackable,
and under 1,000 changed lines. Inspect the diff before committing. Push after three
feature commits or 2,000 aggregate changed lines; review corrections may be pushed
immediately. External review timing and reviewer settings have one owner:
`docs/PRODUCT_REIMPLEMENTATION_PLAN.md` → Per-slice execution gate.

Requests to the designated critical ChatGPT session are pre-authorized. Send one
complete request, wait for and read the completed answer; never use `Get answer now`.
If the session is exhausted or irrecoverably broken, transfer the user-authored
requirements, decisions, and review role to a replacement using the plan's settings.
New automated security scans require explicit approval; never use Deep Scan.

Computer Use is limited to packaged-frontend verification and the authorized web
review workflow. After packaged verification, normally quit only the exact app and
its owned children, reset Computer Use, and remove only that run's isolated data and
regenerable artifacts. Preserve unrelated apps, providers, and user data. Retain
build artifacts needed by active work; use the repository's artifact maintenance
owner for stale build caches, without racing active builds.
