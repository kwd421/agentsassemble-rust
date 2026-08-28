# WORKBOARD

Status: Phase 5 — the admitted-human WebSocket exchange, exact session lifetime, bounded command provenance, copied frontend transport, reachable one-use/reusable normal/read-only browser matrix, remote human preferences, exact `participant.leave`, startup-configured manual public trust, the complete managed quick-tunnel/stable-entry lifecycle, B2 frontend ingress controls, backend manager invite create/revoke controls, C1a exact authority binding, and the strict C1b frontend manager-invite API contract are implemented and test-verified. C2 retained controller/UI custody and packaged activation stay explicitly incomplete and unverified.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; verified public Rust implementation baseline `fdb4e49`; pushed, fully verified, and manually approved B2 baseline `2b97a7c`.
- Active gate: B1a/B1b, their admission-custody corrections, B2, and C1a are approved. C1b now has one strict create/revoke dispatch and acceptance owner without activating copied controller state. C2 retained custody, UI cutover, and packaged activation are next. The reviewed B1a/B1b/B2/C1a/C1b/C2 order is recorded in the active owner above.
- Required order: connect the copied frontend through the implemented exact desktop manager-invite grants to the create/revoke routes, then complete packaged frontend activation before continuing with appearance. Update the exposure map only after a surface is reachable and verified.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
