# WORKBOARD

Status: Phase 5 — B1a/B1b/B2 and C1a/C1b are complete. C2 is active: retained invite custody and controller/UI activation are implemented, fully verified, and manually approved through `10c63b4`; packaged verification remains incomplete.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; verified public Rust implementation baseline `fdb4e49`; pushed, fully verified, and manually approved B2 baseline `2b97a7c`.
- Active gate: C2's post-ticket dispatch fences, monotonic publication/continuity owner, strict directory association, retained invite custody, and controller/UI cutover are approved by the web session and Daybreaker with C/H/M 0/0/0 through `10c63b4`. The packaged managed-ingress normal invite/message, one-use rejection, retained-record replacement, and revoke rejection flows pass after local commit `6a8b5f1`; the remaining packaged matrix is incomplete. The reviewed order is recorded in the active owner above.
- Review batch: reviewed baseline `10c63b4`; queue one local feature commit and 12 changed lines (`6a8b5f1`). Commit each feature independently below 1,000 changed lines, then push and cross-review at three feature commits or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: complete packaged frontend activation before continuing with appearance. Update the exposure map only after a surface is reachable and verified.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
