# WORKBOARD

Status: Phase 5 — B1a/B1b/B2 and C1a/C1b are complete. C2 and its packaged matrix are verified; room appearance persistence, authenticated local/remote HTTP, typed desktop issuance, copied-frontend activation, restart recovery, and verification cleanup are complete through the current manual-review corrections pending final re-review.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; verified public Rust implementation baseline `fdb4e49`; pushed, fully verified, and manually approved B2 baseline `2b97a7c`.
- Active gate: the appearance persistence and local/remote HTTP foundation remains manually approved through `d82b8e2`; frontend activation and its first correction range are pushed through `53033b6`. The next review corrections close asset-ID suffix validation and the authority-loss object-URL window through `4794ca0`. Final web review found one remaining SQLite `NULL`/embedded-NUL grammar mismatch; clean schema 43 closes it in `cc2aebd`. The complete correction range passes `make verify` and is pushed, but both reviewers must approve the exact final range before the next product slice.
- Review cadence: commit each independent change below 1,000 changed lines; push and cross-review at three completed product features or 2,000 aggregate insertions plus deletions, whichever comes first.
- Required order: close both exact-diff re-reviews for the final schema correction and record only their final findings and verdicts. After approval, choose the next currently reachable missing surface from the exposure map; do not prebuild its authority or state in this slice.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
