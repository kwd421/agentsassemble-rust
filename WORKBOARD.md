# WORKBOARD

Status: Phase 5 — B1a/B1b/B2 and C1a/C1b are complete. C2 is locally complete: retained invite custody and controller/UI activation are manually approved through `10c63b4`, and the packaged matrix is verified locally; its batched manual review is queued while appearance is active.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; verified public Rust implementation baseline `fdb4e49`; pushed, fully verified, and manually approved B2 baseline `2b97a7c`.
- Active gate: C2's authority and controller/UI cutover are approved through `10c63b4`; local commits `6a8b5f1` and `1aca717` pass the complete packaged invite matrix. Appearance is now the active implementation owner, while C2's local completion waits for the next batched review.
- Review batch: reviewed baseline `10c63b4`; queue two local feature commits and 33 changed lines (`6a8b5f1`, `1aca717`). Commit each feature independently below 1,000 changed lines, then push and cross-review at three feature commits or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: implement and verify the smallest complete appearance lifecycle surface next. Update the exposure map only after that surface is reachable and verified.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
