# Core Room Production Slice

Status: active implementation owner

## Definition

A desktop host opens an existing agent-free room, sends one plain-text message through the Rust runtime, sees the canonical message in the existing React UI, and recovers it exactly across reconnect and runtime restart.

## Current contract

The reachable frontend obtains an opaque ticket, connects to `/ws?ticket=...`, subscribes with `resume_from_seq`, validates a canonical snapshot, sends a correlated `message.send`, and accepts an ACK only when its durable `message_final` event and `event_seq` agree. It rejects gaps and invalid snapshots.

The Rust slice owns this complete boundary: local principal resolution, ticket consumption, room mutation ordering, SQLite transaction, event delivery, snapshot/resume, and sidecar lifecycle. Python is not a runtime dependency of the accepted flow.

## Non-goals

- provider or Agent Session processes;
- attachments, votes, moderation, invites, custom channels, or room creation;
- PostgreSQL;
- redesigning the existing React room UI;
- runtime fallback to Python.

## Acceptance criteria

1. Tauri or the equivalent desktop lifecycle harness starts the Rust sidecar on loopback and receives structured readiness.
2. The existing React socket client accepts the Rust initial snapshot for an existing room.
3. A host `message.send` commits one positive, contiguous `message_final` sequence and a matching durable ACK.
4. Retrying the identical principal/request/action/payload returns the committed result without another event; changing action or payload returns a conflict.
5. A non-member, read-only principal, muted participant, reused/expired ticket, invalid snapshot cursor, or failed SQLite transaction fails closed.
6. Reconnect resumes without duplication or a gap; runtime restart retains the message and sequence.
7. The browser visibly renders the sent message using the existing React projection.

## Verification path

- `make architecture-check`
- `cargo test --workspace --all-features`
- `npm --prefix frontend run build`
- a real-browser flow against the Rust sidecar covering send, reconnect, and restart
- `make verify`

Any Computer Use resources created by the real-browser flow are closed under
`docs/VERIFICATION.md`. This agent-free slice does not start a provider; later
provider-visible frontend slices use the exact real Agent matrix owned there.
