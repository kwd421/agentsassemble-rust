# WORKBOARD

Status: Phase 3 — human user-profile SSoT is the active migration slice.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/user-profile-slice.md`](docs/specs/user-profile-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust `377d2af`. Uncommitted work is not public completion evidence.
- Required order:
  1. establish the server-wide human profile and its authenticated HTTP owner;
  2. project only display name and avatar into current human room memberships;
  3. connect the copied UserPanel and safe profile-avatar flow without client authority;
  4. pass persistence, HTTP, reconnect/restart, copied-UI, and cleanup verification;
  5. review the published slice in the web and manual-security sessions and fix validated findings before closure.
- Exit: one durable human profile drives the left-bottom card and every current room projection across save, reconnect, and runtime restart without changing room-owned membership authority or any Agent Session profile; all verification-owned processes and UI resources are shut down.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
