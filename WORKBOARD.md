# WORKBOARD

Status: Phase 5 — live-session-bound typed exchanges and target routes are the active prerequisite before remote preference and room-appearance activation. Durable admission, grant custody, profile target revalidation, and the corrected asset-storage lifecycle are implemented and manually approved; their public exchange/target entry points remain explicitly unavailable until connected and verified.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust baseline `6bff15d`.
- Required order: connect the closed typed session-exchange routes and each exact-purpose target only after its durable post-consumption revalidation exists; then activate trusted public ingress, remote preference, and appearance in dependency order. Update the exposure map only after a surface is reachable and verified; inspect `git diff --stat`, then commit and push each independently buildable, verifiable, and rollbackable change before both manual reviews.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
