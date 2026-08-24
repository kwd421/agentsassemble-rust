# WORKBOARD

Status: Phase 4 — local authority, product surfaces, strict admission/subscription, and canonical role/mute are the active cutover boundary. Pro critical design review approved the complete boundary on 2026-08-25; implementation is not yet completion evidence.

Purpose: route the asynchronous Rust reimplementation without duplicating its contracts.

## Active work

- Owner: [`docs/specs/local-authority-surface-admission-moderation-slice.md`](docs/specs/local-authority-surface-admission-moderation-slice.md)
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)
- Real-client verification: [`docs/VERIFICATION.md`](docs/VERIFICATION.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`; public Rust baseline `92e6bb44e3154f2c8d4d2f7b50761ebcf8e92bb7`.
- Required order:
  1. replace schema-coupled seed/bootstrap with the reviewed immutable-lineage local authority and real zero-room directory/create/join flow;
  2. derive actual server/host product surfaces, enforce strict typed WebSocket subscription and process-wide pre-parse admission, and complete proof-bound finite catch-up;
  3. complete canonical roster and role/mute authority, including durable provider-start serialization, interrupt quiescence, and restart custody reconciliation;
  4. connect only complete surfaces in the copied frontend, run deterministic and mandatory repository verification, then perform packaged Computer Use with the approved persistent provider matrix;
  5. commit and push each complete vertical slice before same-session critical diff and Daybreaker Blue High manual-security review; after this boundary exits, return to the remaining settings/preferences/appearance and exposure-map contracts.
- Exit: a fresh local authority reaches a real zero-room product without seed data; every composed frontend path is backed by its actual authority and advertised surface; subscription readiness is proof-bearing and gap-free; admission cannot be bypassed by room sharding; role/mute survives races and restart without stopping persistent runtimes or publishing stale success; all gates, packaged flows, cross-reviews, and verification cleanup pass.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → active owner above.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read `docs/ARCHITECTURE.md`.
- Frontend, Computer Use, profile avatar, or real-client verification: also read `docs/VERIFICATION.md`.
- Workboard changes: also read `WORKBOARD_GUIDE.md`.
