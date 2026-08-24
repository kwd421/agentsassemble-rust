# WORKBOARD

Status: Phase 4 — canonical current room settings, ordered/ambient scheduling, and tabletop are the active reimplementation slice; the uncommitted legacy-contaminated prototype is not completion evidence.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/room-settings-preferences-appearance-slice.md`](docs/specs/room-settings-preferences-appearance-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Stage A baseline `d111c29`. The active review correction is tracked by the current `main` candidate and is not completion evidence until both critical reviews approve it.
- Required order:
  1. remove the uncommitted compatibility migration and legacy continuous-relay implementation, then critically review the corrected current-product design;
  2. implement and publish Stage A settings, typed durable input, ordered/ambient schedulers, and human/provider tabletop as one complete authority boundary;
  3. verify Stage A through deterministic contracts, mandatory gates, packaged UI, and the three approved persistent real providers, then obtain critical web and manual-security review;
  4. implement, publish, verify, and review Stage B user preferences plus room appearance on the Stage A transaction boundary;
  5. route the next incomplete original product contract from the exposure map.
- Exit: copied current settings controls mutate only completed Rust behavior; ordered and ambient scheduling plus tabletop survive restart with exact authority; legacy/compatibility state is neither imported nor executed; the authenticated user retains isolated preferences and safe room appearance; every public feature commit is reviewed; and no verification-owned process or UI resource remains running.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
