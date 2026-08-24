# WORKBOARD

Status: Phase 3 — server-owned room directory and canonical creation are the active migration slice.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/room-directory-creation-slice.md`](docs/specs/room-directory-creation-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust `6624e51edbd71c450497c41812eab23bb0e74770`. Uncommitted work is not public completion evidence.
- Required order:
  1. publish the locally verified canonical room-directory/creation slice;
  2. record its exact public comparison and packaged-app evidence;
  3. obtain critical web and manual-security review and fix validated findings;
  4. route the next incomplete original product contract from the exposure map.
- Exit: the copied room rail is hydrated from durable Rust authority, creates and enters a real room, retains stable server/room identity and message history across restart, and leaves no verification-owned process or UI resource running.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
