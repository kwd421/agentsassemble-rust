# WORKBOARD

Status: Phase 5 — the admitted-human WebSocket is the active authority boundary. Durable admission, grant custody, corrected asset lifecycles, and the live-session profile exchange/target are reachable, real-client verified, and manually approved; WebSocket activation and remaining typed exchanges stay explicitly incomplete.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust baseline `644b1d5`.
- Required order: activate the admitted-human WebSocket only with subscribe-before-consume revocation custody, durable session revalidation, command-UOW provenance, and final outbound checks; then connect each remaining typed exchange after its exact target revalidation, followed by trusted public ingress, remote preference, and appearance. Update the exposure map only after a surface is reachable and verified; inspect `git diff --stat`, then commit and push each independently buildable, verifiable, and rollbackable change before both manual reviews.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
