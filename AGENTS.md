# AgentsAssemble

AgentsAssemble is being reimplemented as an asynchronous Rust runtime.

Preserve all currently reachable product behavior.
Reimplement the product, not the Python source tree.

## Product scope

The old v0 scripted-meeting runner (generated agendas, forced rounds, automatic
synthesis/decisions/task assignment and its research artifacts) is not reimplemented.
Ordinary room conversation, persona cards, permitted tool use, roles, history, search
and pins stay. Freebuff is excluded; Antigravity joins only as an external CLI.
`docs/PRODUCT_SCOPE.md` has the exact lists; read it before adding or removing meeting,
research, persona or provider behavior.

## Implementation

Use mature maintained libraries for infrastructure; implement only product semantics.
Make the smallest complete change. Read only what the task needs: `Rule.md` for design
and review constraints, the top entries of `WORKBOARD.md` (newest first) for active
work, `SDD.md` for substantial design, and a feature's contract in `docs/specs/` (start
from the files it lists) instead of surveying the repository.

Discover behavior from reachable code and real flows, not old product markdown.
After cutover, the Rust contract and verified user flow are authoritative.
Completion preserves the reachable entry point, authority owner, state transition,
retry/failure semantics, and real user flow. Expand the implementation boundary
when necessary; placeholders, fake authority, disabled synchronization, authentication
bypasses, and client orchestration cannot replace a server-owned contract.
Continue through implementation, affected verification, and correction until the
active acceptance criteria are met or an actual authorization/dependency blocks work.

Before implementation, record the user's required behavior and affected entry points
in the existing contract; do not narrow them to what the chosen implementation supports.
When one defect exposes a shared cause, inspect and correct its affected flows within
the authorized scope without waiting for the user to enumerate them. App and web share
product UI and semantics; isolate platform transport/capabilities instead of creating
separate product flows. Unapproved behavior differences remain defects, not acceptance.

## Permissions and gates

- Security takes priority. New fallbacks and compatibility shims require explicit
  user approval; fix failures at their owner instead of hiding them.
- Architecture and structure gates remain mandatory. Do not weaken, bypass, raise,
  or add exceptions to pass. Changes to blocking gates require prior owner approval
  and deterministic actionable failures.
- Real providers, destructive migrations, deletion of user data, and termination of
  external processes require explicit approval unless already authorized for this task.
  Authorized real-provider verification after reimplementation: the configured
  DeepSeek API, Codex, OpenCode, external Antigravity, Grok and Cursor `auto` only.
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

Manual review requests under the plan's per-slice execution gate are pre-authorized.
Send a complete request, wait for and read the completed answer. Reviewer selection
and phase-closure requirements are owned by that gate.
New automated security scans require explicit approval; never use Deep Scan.

Computer Use is limited to packaged-frontend verification and the plan's web review.
Afterwards quit only the exact app and its children, reset Computer Use, remove only
that run's isolated data, and leave other apps, providers, user data and active build
artifacts alone; stale caches go through the artifact maintenance owner without
racing active builds.
