# WORKBOARD

Status: Phase 5 — canonical lobby mutations are implementation/package-verified; external review is pending.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/lobby-message-mutations-slice.md`](docs/specs/lobby-message-mutations-slice.md)
- Completed prerequisite: [`docs/specs/lobby-votes-slice.md`](docs/specs/lobby-votes-slice.md)
- Exposure inventory: [`docs/FRONTEND_BACKEND_GAPS.md`](docs/FRONTEND_BACKEND_GAPS.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; pushed, fully verified, and manually approved Rust persona baseline `f6f8636`.
- Active gate: Daybreaker manually approved pushed HEAD `d168354` and cumulative
  `a958bab..d168354` at C0/H0/M0/L0. Close critical-web manual review of canonical lobby message edit
  and message/poll deletion, including every correction, the cumulative implementation, copied
  controls, and packaged lifecycle evidence.
- Review cadence: keep every independent change below 1,000 changed lines. Push and cross-review when the unreviewed aggregate first reaches at least 1,000 changed lines; feature count alone does not trigger review, but a three-feature batch must not grow beyond roughly 2,000 changed lines.
- Source structure: LOC only signals possible ownership drift. Review at 500 lines; treat 800 lines as a strong split candidate; reject over 1,000 by default, with concrete generated-code, fixture, or declarative-data exceptions considered only when they exist. Split at differing state/invariant, domain, authority, lifecycle, or change-reason owners regardless of size; reconsider a split if it increases state transfer, public interfaces, inter-module dependency count, or obscuring glue.
- Required order: run complete gates on the review corrections; commit them as independent
  production/test/documentation units; push them into the open review; then re-review each commit
  and cumulative range for ownership, duplication, overengineering, lifecycle, meaningless
  polling/heartbeat/timers/retries, fallback, and swallowed failure before accepting findings or
  advancing the workboard.
- Exit: critical-web and Daybreaker both approve the exact pushed range after any corrections; the
  recorded local and admitted flows already preserve exact edited/tombstoned state across live
  delivery, reload, and restart, with search, pins, votes, and attachments agreeing without leaked
  deleted data or background cleanup.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, HTTP authority, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
