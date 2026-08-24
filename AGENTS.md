# AgentsAssemble

AgentsAssemble is being reimplemented as an asynchronous Rust runtime.

Preserve all currently reachable product behavior.
Reimplement the product, not the Python source tree.

Use mature maintained libraries for solved infrastructure.
Reuse mechanisms; implement AgentsAssemble product semantics.
Do not reinvent frameworks, protocols, cryptography, database drivers, WebSocket framing, routing, async runtimes, serialization, pooling, or generic concurrency primitives.

Prefer the smallest boring design that fully works.
Security takes priority over convenience.

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
