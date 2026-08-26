# WORKBOARD

Status: Phase 5 — durable human admission and remote-session preference authority are the active prerequisite before room-appearance activation. Local room settings and local-operator preferences are verified; remote preferences are explicitly unavailable rather than falsely connected.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust baseline `7905a0b`.
- Required order: obtain line-by-line approval of the active SDD, then implement the complete human invite/admission/session owner and live-session-bound one-use exchanges before trusted public ingress, remote preference, or appearance activation; update the exposure map only after a surface is reachable and verified; inspect `git diff --stat`, then commit and push each independently buildable, verifiable, and rollbackable change before both manual reviews.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
