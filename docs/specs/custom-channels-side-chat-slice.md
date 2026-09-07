# Custom text channels and ephemeral side chat

Status: Phase 6 definition after Phase 5 approval at `1e24adf`.

## Definition and observed contract

Restore the retained custom-text and side-chat entry points from original product
`d504647` in `/Users/seinel/Projects/AgentsAssemble`, using its reachable code rather
than the legacy implementation layout. Discovery owners are `room/channels.py`,
`web/routes/room_media.py`, `room/settings_commands.py`, `features/jsonl_chat.py`,
`features/side_chat/{routes,service}.py`, `web/room_session.py`, and the original
`useRoomChannels`, `CustomChannelView`, `useRoomSideChat` and `SideChatDock` callers.

Custom channels are the ordered `channels` field of canonical room settings. The
existing WebSocket settings command, room-management permission, expected revision
and command receipt remain its sole writer. Names are bounded to 60 characters,
there are at most 50 channels, and IDs retain the canonical `c` plus 12 lowercase
hex format. Names are labels and can repeat; changing a label or order does not
change the message stream. The original frontend creates text channels; direct
settings changes also retain rename/remove/reorder semantics. Voice remains
deferred and inactive, including its copied presence poll and heartbeat.

Each custom text stream has durable room/channel-bound messages, readable history,
live events, search/context and pins. Its verified composer is text-only and bounded
to 2,000 characters. Current identity, membership, room state, read-only scope and
mute restrictions remain server-owned; caller-supplied names never supply identity.
These messages do not become lobby messages or scheduled agent-turn input. Existing
shared infrastructure for room sequencing, persistence, exact command receipts and
search indexing may be reused without merging the product owners.

Side chat is human-only, independent of the durable room transcript and invisible
to Agent Sessions and provider tools. The original UI explicitly promises that it
disappears on server restart and retains at most 200 messages from the last 24
hours. Its 2,000-character composer supports ordinary mention/emoji text but does
not trigger agents. Read-only humans may read and cannot post. Room deletion clears
its data. The original desktop right dock and mobile room-info entry are retained.

## Authority, lifetime and failure boundary

Use the existing Rust room settings, room membership/session, native purpose-ticket
and public bearer boundaries. Revalidate mutable permission in the owning operation,
including paired device authority, and bind every channel operation to the current
room incarnation and current registered text channel. Missing/deleted/wrong-room
channels fail explicitly; no lobby alias, missing-file empty success or compatibility
route supplies their data. Channel removal must retire visibility of its messages,
search and pins without permitting a new channel to inherit an old stream.

The channel owner commits the message and shared room event/receipt atomically.
History is bounded and channel-specific; the existing room event projection carries
live changes and supplies reconnect/resync instead of polling. Search uses one
canonical index/query policy for the actual readable lobby/custom-channel union;
channel context and pin targets resolve the exact channel owner. No custom-channel
attachment, new scheduled meeting pipeline or agent runtime is introduced.

Side-chat content and retry custody remain in server memory, separate from durable
room events, command results and provider context. Its repository owns retention,
sequence and restart generation, and derives its live WebSocket projection from
that state. Bootstrap/resubscribe must not lose an append between snapshot and live
subscription. Uncertain retries cannot silently duplicate a retained message or
cross a restart/retired room; an expired retry boundary stays explicit. Retention
is evaluated on access and lifecycle cleanup, without a new periodic task. Reads,
posts, projection and cleanup share current human authority and room identity.

## Dependency order and acceptance

Define both owners before polishing either surface. Implement the channel message,
history/event path and side-chat bootstrap/mutation/live path, then complete shared
room-wide search/context and channel pins. Restore the copied entry points only as
their real owners become available. Keep the existing design system, margins,
focused dialogs and Discord-like channel interactions; remove the obsolete missing
route polling from the text path before enabling it.

Acceptance covers native, paired and ordinary/read-only human boundaries; different
rooms/channels, stale settings, removal/recreation, concurrent append and replay,
permission/mute/revoke changes, restart, socket gaps and explicit failed reads/writes.
Verify side-chat retention with a controlled clock and prove its absence from the
durable database, room search/history and provider-visible views. Reuse existing
boundary tests where they already own the invariant and add only concrete missing
cases. Run affected builds/tests and unchanged structure/security/CSS gates.

Directly manipulate the packaged desktop and 390px flows: add/select channels,
post/read/search/navigate/pin, settings changes, side-chat send/failure/reconnect,
and human read-only/leave transitions. Measure CPU, memory, latency, task/process
and disk costs at the completed phase. Clean only that verification's owned app,
children, browser tabs and isolated data. Obtain Daybreak approval of every phase
commit, cumulative range, exact HEAD and whole local phase before Phase 7. Real
AI providers and new Pro review remain at authorized full closeout.
