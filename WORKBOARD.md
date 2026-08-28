# WORKBOARD

Status: Phase 5 — the admitted-human WebSocket exchange, exact session lifetime, bounded command provenance, copied frontend transport, reachable one-use/reusable normal/read-only browser matrix, remote human preferences, exact `participant.leave`, startup-configured manual public trust, the complete managed quick-tunnel/stable-entry lifecycle, B2 frontend ingress controls, backend manager invite create/revoke controls, and C1a exact manager-room authority transport, server ticket issuance, and transaction revalidation are implemented and test-verified. C1b frontend activation, C2 retained invite custody, and packaged activation stay explicitly incomplete and unverified.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/human-invite-admission-session-slice.md`](docs/specs/human-invite-admission-session-slice.md)
- Completed prerequisite: [`docs/specs/asset-custody-lifecycle-slice.md`](docs/specs/asset-custody-lifecycle-slice.md)
- Downstream owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; verified public Rust implementation baseline `fdb4e49`; pushed, fully verified, and manually approved B2 baseline `2b97a7c`.
- Active gate: B1a/B1b, their admission-custody corrections, and B2 are approved. C1a now carries one exact manager-room authority tuple through private control, verifies it before issuing a purpose-specific ticket, and revalidates it in the invite mutation transaction without activating the UI. C1b response/revoke dispatch is next; C2 retained custody and packaged activation remain incomplete. The reviewed B1a/B1b/B2/C1a/C1b/C2 order is recorded in the active owner above.
- Required order: connect the copied frontend through the implemented exact desktop manager-invite grants to the create/revoke routes, then complete packaged frontend activation before continuing with appearance. Update the exposure map only after a surface is reachable and verified.
- Exit: local and remote humans reach preferences through their real authority owner, appearance uses its complete asset lifecycle, incomplete adjacent surfaces remain visibly unavailable, and mandatory gates, packaged frontend flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
