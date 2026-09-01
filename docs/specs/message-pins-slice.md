# Lobby message pins

Status: implementation and flow evidence retained; remote-human HTTP authorization
reopened by repository audit D-03

## Definition

Reconnect the copied lobby pin list, pin, unpin, and pinned-message navigation to
canonical Rust room events through bounded-purpose HTTP authority.

## Approved target contract after Phase 0B

- `room_events` remains the only message authority. A pin is a durable pointer to one
  existing public `message_final`; it never copies or rewrites message content.
- This slice owns only the reachable lobby flow. Custom-channel pins remain unavailable
  until the custom-channel runtime owns its messages. Search, context lookup, history
  paging, message edit/delete, and attachments are separate product slices.
- Pin records store only the room, event identity/sequence, and pin timestamp. The
  original `pinned_by` field is not exposed by the reachable product and does not affect
  authorization, ordering, projection, or cleanup, so Rust does not preserve that
  otherwise ownerless state.
- A list read requires a currently joined human with `room.history`. A mutation requires
  a currently joined human with `message.modify`; read-only sessions cannot mutate.
  Room status, membership, invite scope, and current profile remain owned by their
  existing authorities.
- Local desktop HTTP access uses a fresh one-use room- and operation-purpose ticket
  received through a typed host command. A remote human presents its durable session
  bearer in the bounded Authorization header directly to the pin route, which resolves the exact room principal and operation;
  the audited preliminary session-to-purpose-ticket exchange is a Phase 0B removal target.
  Raw issuer credentials, reusable room socket tickets, and cross-scope credentials are
  not accepted.
- Authentication happens before a mutation body is read. The persistence unit
  reauthorizes the exact room participant and validates the target event before changing
  the pin and reading the returned list. Wrong-room, wrong-scope, replayed local tickets, expired,
  revoked, missing, non-message, malformed, and read-only requests fail without state
  change.
- `GET /api/room-pins?room_id=...&channel_id=lobby` returns the copied projection:
  `event_id`, `channel_id`, `pinned_at`, `seq`, `author`, `content`, `created_at`, and
  `attachment_filenames`. The current message-attachment owner supplies canonical
  filenames for attached messages. Results are newest pin first with event identity
  as the stable tie-break.
- `POST /api/room-pins` accepts exactly `room_id`, `channel_id`, `event_id`, and
  `pinned`. Re-pinning refreshes the existing pointer's timestamp; unpinning an absent
  pointer to a valid message remains an idempotent no-op. Missing, non-message, deleted,
  or corrupt targets fail before either branch mutates state. The response returns the
  complete current list as the copied UI expects.
- A room owns at most 64 lobby pins. This absolute response-safety bound keeps the
  complete-list contract finite; a new pin at capacity fails without writing, while an
  existing pin may be refreshed and any valid target may be unpinned. Listing reads at
  most one sentinel row beyond the bound and rejects excess durable state instead of
  allocating an unbounded projection.
- HTTP owns this bounded request/response state operation; the room WebSocket continues
  to own live room events. No WebSocket pin event or client-side pin authority is added.

## Non-goals

- No custom-channel implementation, generic message-record framework, derived search
  index, pin notification event, audit subsystem, pagination redesign, compatibility
  path, fallback transport, or configurable quota layer beyond the absolute response
  safety bound.
- No recreation of the excluded scripted-meeting runner or its artifacts/models.

## Acceptance criteria

1. The packaged local copied frontend pins and unpins a real lobby message, shows the
   canonical projection, navigates to it, and retains the pin after an exact restart.
2. A writable admitted human can list and mutate pins; a read-only admitted human can
   list but receives a stable denial on mutation.
3. Wrong room/scope, replayed or expired local ticket, expired or revoked remote
   session, malformed or oversized body, absent target, and non-message target leave
   durable state unchanged.
4. Re-pinning one event produces one row at the newest position; unpin is exact and
   deletion of a room removes only that room's pins through room ownership. A 65th
   distinct pin fails without changing the complete 64-item list; re-pin and unpin remain
   available at the bound.
5. No legacy host-token, session bearer outside the bounded Authorization header,
   placeholder, fake-authority, migration, or fallback path becomes reachable.
   Existing architecture, source-growth, and 800-line gates pass.

## Verification path

- Persistence behavior and schema invariants, including exact target validation,
  transactional rollback, ordering, re-pin, unpin, and room-cascade cleanup.
- Local ticket/store, private-control, desktop registry/capability, and remote bearer
  real-TCP HTTP tests for scope separation, auth-before-body, room/session revalidation,
  disclosure rejection, and read-only denial.
- Focused copied-frontend API/controller tests, full frontend suite/build, `make verify`,
  and a fresh isolated packaged Computer Use flow with exact process/data cleanup.
- Each independent commit remains below 1,000 changed lines. Batch timing is owned by
  `docs/PRODUCT_REIMPLEMENTATION_PLAN.md`.
