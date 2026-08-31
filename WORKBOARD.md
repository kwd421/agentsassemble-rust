# WORKBOARD

Status: Phase 5 — Agent Session idle pause/resume is implemented and packaged-verified; cleanup-proof follow-up and review are active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/agent-session-slice.md`](docs/specs/agent-session-slice.md#active-idle-pauseresume-extension)
- Completed prerequisite: [`docs/specs/lobby-message-mutations-slice.md`](docs/specs/lobby-message-mutations-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: resolve the packaged macOS Codex cleanup-receipt uncertainty without weakening the
  fail-closed custody contract, then cross-review the threshold batch. The pause/resume behavior and
  exact three-provider matrix are otherwise implemented and locally verified.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Source structure: LOC only signals possible ownership drift. Review at 500 lines; treat 800 lines as a strong split candidate; reject over 1,000 by default, with concrete generated-code, fixture, or declarative-data exceptions considered only when they exist. Split at differing state/invariant, domain, authority, lifecycle, or change-reason owners regardless of size; reconsider a split if it increases state transfer, public interfaces, inter-module dependency count, or obscuring glue.
- Required order: keep the verified pause/resume commits intact, determine the exact Codex receipt
  failure at its custody owner, preserve uncertainty unless a stronger proof exists, then rerun the
  complete gates and packaged boundary before both manual reviews.
- Exit: pause leaves each exact resident provider process and conversation intact with no new turn;
  paused input stays durably queued; resume assigns it through the existing floor owner; reload,
  restart, and Codex Terra, Antigravity Flash, and OpenCode Muse Spark packaged flows pass; both
  reviewers approve the pushed range.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
