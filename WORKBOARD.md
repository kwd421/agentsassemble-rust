# WORKBOARD

Status: The Phase 0A source/duplication/defensive-complexity audit and planning
review are closed at reviewed content checkpoint `9711232`. Phase 0B foundation
correction is active from public baseline `4ab5ee1`.

Purpose: route the asynchronous Rust reimplementation without duplicating product
contracts, findings, or verification journals.

## Active work

- Phase: 0B foundation correction.
- Completed: D-01 at `a7949bd`; the uncalled HTTP challenge/ticket bootstrap and
  startup secret are absent, while private-control and admitted-human socket ticket
  issuance remain.
- Completed: D-02 at `3ffb9eb`, `77cae0e`, `0d24741`, and `57fd6ec`; the
  evidence-free receipt, digests, per-frame HMAC/base64/counter envelope, proof-key
  ticket state, and obsolete test vocabulary are absent. One-use ticket authority,
  strict bounded JSON, finite snapshot/catch-up, replay, and failure contracts remain.
- Completed: D-03 through `5693e13`. Profile, preferences, message pins,
  message search, message attachments, and bound room-appearance reads authorize
  reusable remote sessions at the target; the obsolete socket-to-profile authority
  interpretation and public HTTP-purpose exchange state are absent. Desktop purpose
  tickets and one-use WebSocket upgrade tickets remain because they cross distinct
  authority boundaries. Critical ChatGPT Pro and Daybreaker Blue High each manually
  approved cumulative `ac905de..5693e13` and HEAD at `C0/H0/M0/L0`.
- Completed: F-04 through `8903445`. Four non-executable capability fields,
  copied room-delete/participant-kick/provider-response/agent-readd controls, and the
  producerless provider-request snapshot, kicked-event projection, and room-delete
  callback are absent. `bridge.publish` remains because the current vote path consumes
  it; the distinct server `participant_kicked` start-denial code and OpenCode's
  interactive-request fail-closed test remain current contracts. Critical ChatGPT Pro
  and Daybreaker Blue High each approved exact `f4bc3d9..8903445`, cumulative
  `dd1e99d..8903445`, and HEAD `8903445` at `C0/H0/M0/L0` after the stale re-add
  guidance and audit-state corrections.
- Active task: F-05 frontend exposure correction — gate copied requests,
  polling, and heartbeats whose complete Rust owner does not yet exist. Do not add
  dummy routes, fallback data, timers, or a generic feature framework. The first
  independently committed batch through `87d3d0d` removes the active Friends,
  side-chat, custom-channel, and deferred voice entry paths; a fresh `make verify`
  passed before review. Initial cross-review found three Low dead-state remnants;
  `a2b2f41` removes them and a fresh complete `make verify` passes. Follow-up
  re-review found two Low producerless/dead projections plus one Low documentation
  overclaim; `87d3d0d` and the current documentation correction address them, with
  a fresh complete `make verify` passing. Critical ChatGPT Pro and Daybreaker Blue
  High each approved exact `778d761..f74af57`, cumulative `8903445..f74af57`, and
  HEAD `f74af57` at `C0/H0/M0/L0`.
  The next three independent commits, `762ba40`, `11e167b`, and `7159c2d`, remove
  the copied Room Connector invite, operator-pairing issuer, and guest companion
  admission controls without changing human invitation, incoming pairing redemption,
  or room membership. A fresh complete `make verify` passes. Both manual reviewers
  found the Low omission of a JavaScript chunk from the emitted total; Daybreaker then
  found that first correction `95951d9` described the aggregate rounding incorrectly.
  Corrections through `7f2e878` distinguish the raw-byte aggregate from displayed
  per-chunk gzip figures. Critical ChatGPT Pro and Daybreaker Blue High each approved
  the corrected original batch, correction `9759d73..7f2e878`, cumulative
  `8903445..7f2e878`, and
  HEAD `7f2e878` at `C0/H0/M0/L0`.
  The dormant AI-friend invite branch, public Google controls, and evidence-backed
  dormant-source cleanup remain in F-05.
- Sequence/exit owner: [`docs/PRODUCT_REIMPLEMENTATION_PLAN.md`](docs/PRODUCT_REIMPLEMENTATION_PLAN.md)
- Finding/evidence owner: [`docs/architecture/REPOSITORY_AUDIT_2026-09-01.md`](docs/architecture/REPOSITORY_AUDIT_2026-09-01.md)
- Comparison baseline: original `d5046473010d1353a81ee38337360e6d98f7bd6f`;
  audited Rust baseline `8a5f75a`.
- Exit 0A: satisfied. The complete planning range, master plan, finding register,
  and aligned current contracts received critical-web Pro and Daybreaker Blue High
  manual approval at `C0/H0/M0/L0`. No product-code completion is claimed by this
  phase.

## Read routes

- Any implementation: `AGENTS.md` → `Rule.md` → this board → active phase owner.
- Architecture, protocol, persistence, auth, lifecycle, or cutover: also read
  [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and the exact file under
  [`docs/specs/`](docs/specs/).
- Frontend or real-client verification: also read `docs/FRONTEND_BACKEND_GAPS.md`
  and `docs/VERIFICATION.md`.
- Workboard restructuring: also read `WORKBOARD_GUIDE.md`.
