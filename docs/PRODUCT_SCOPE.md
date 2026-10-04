# Product scope

Moved from `AGENTS.md` (2026-10-04). Owned by the user; change only with approval.

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

User-approved provider scope (2026-09-07): exclude Freebuff from this reimplementation.
Antigravity participates only through an externally launched CLI and Room Connector
invite link; do not implement or expose an app-managed Antigravity Agent Session.
Its external admission remains required under the product plan's Phase 7 contract.
Real-provider verification runs after reimplementation: only the already configured
DeepSeek API, Codex, OpenCode, external Antigravity, Grok, and Cursor `auto` are
authorized. Implement other providers from their reachable original contracts without
real execution. Local tests, mandatory gates, and phase code reviews still apply.
