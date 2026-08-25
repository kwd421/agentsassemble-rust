# WORKBOARD

Status: Phase 5 — room settings, per-user preferences, and appearance are the active cutover boundary. Phase 4 local authority, product surfaces, strict admission/subscription, canonical role/mute, packaged provider matrix, cross-review, and cleanup completed on 2026-08-26.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust baseline `4c7b2a0`.
- Required order: follow the active owner's Stage A then Stage B authority and verification boundaries; update the exposure map only after a surface is reachable and verified; commit and push each independently reviewable change before both manual reviews.
- Exit: every currently reachable settings, preference, and appearance control uses its complete Rust owner and real asset lifecycle; incomplete adjacent surfaces remain visibly unavailable; mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
