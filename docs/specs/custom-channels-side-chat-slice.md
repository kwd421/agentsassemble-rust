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

## Channel persistence owner

The first channel slice uses `channel_message_final` records in the existing durable
room event store; that distinct event type never enters the lobby message/turn
owner. The channel append transaction revalidates room/session membership and the
current text-channel registration before an exact command replay or new write.
It applies the existing participant write policy, commits one event and receipt,
and never creates provider input. Channel history selects only that room/channel
through a partial SQLite expression index and returns bounded messages with the
same transaction's room high-water mark.

Channel changes remain canonical settings updates. Renaming/reordering preserves
identity; type/creation identity changes fail. Removal atomically tombstones the
channel's visible event content and removes its search/pin pointers. One retired ID
row prevents a later channel from inheriting its stream or notification identity.
The sequence remains intact for existing room subscriptions. Registered-channel
validation precedes replay, so the stored old append result cannot reopen a removed
channel. The clean schema is version 66; older databases fail without conversion.

Local acceptance: four persistence cases cover channel/room isolation, paging,
replay/conflict, restart, rename/reorder, retirement/reuse rejection, read-only/mute,
and injected final receipt failure rollback (0.06 s). Sixty domain cases (0.08 s)
and 23 affected schema cases (0.24 s) pass. The index exposed a prior pin fixture
which manually interpolated an unescaped NUL into JSON; using the existing JSON
serializer preserves that fixture's event-ID rejection test. No validation was
weakened. The message/history transport and copied UI are not enabled by this
persistence-only slice; search/pins and side chat remain later Phase 6 obligations.
Domain/persistence all-target/all-feature Clippy and unchanged architecture/source,
19 policy cases, formatting/diff and artifact checks pass. Storage adds one partial
index entry per custom message and one small row per retired channel; existing
event/receipt size and write budgets still apply. No timer or provider task is added.

## Side-chat memory owner

Each current room incarnation has one store-owned memory repository with a restart
generation, monotonically increasing sequence, and at most 200 retained messages
and exact retry receipts. Reads and sends prune entries at the 24-hour boundary;
physical room deletion removes the repository and closes its live subscription.
No side-chat content or receipt enters SQLite, room events, search or turn input.
Shared text sanitization owns the original 2,000-character composer limit.

The existing single-connection authority transaction serializes current human
membership, read/write permission and mute checks with the memory append. Identity
comes from the canonical participant. An exact retained retry returns the original
message without another broadcast; a changed payload conflicts. A send includes
the observed generation and sequence, so a restart or expired receipt horizon
returns an explicit unresolved result instead of duplicating an uncertain send.
A read-transaction completion failure after append also stays unresolved, with its
memory receipt available for retry. Subscribe-before-bootstrap supports overlap
deduplication by generation/sequence without losing an append between the two.

Four actual-store cases pass (0.08 s): bootstrap/live overlap and retry, current
human/read-only/mute authority, count/controlled-clock retention, restart with no
durable sentinel or command receipt, and room isolation/deletion/recreation. The
read-only fixture derives capabilities using the production scope constructor.
This foundation adds no timer, task or provider execution. Retained message lookup
is bounded by 200 entries; bootstrap clones at most 200 text messages. Transport
and packaged UI acceptance remain open until subsequent Phase 6 slices.

## Channel transport

`channel.message.send` enters the existing bounded room command/admission and durable
publication owner, using the channel transaction for native, paired and human
authority. It creates no provider assignment. `channel.history` uses the existing
socket history budget, current session authority and shared cursor parser, with
the original 80-message default. Both are declared by the canonical product surface
and generated action types. Channel result/message types are generated from Rust.

History frame fitting now takes the bounded page encoder as its sole variable:
the existing exact-size binary search keeps the newest events and updates the
oldest cursor/more flag for either page shape. Channel identity and shared room
high water survive fitting, including noncontiguous channel sequences. The 256 KiB
frame gate is unchanged; no estimate or oversized frame bypass was added.

Actual TCP/WebSocket acceptance passes for native append/replay/live events,
read-write and read-only human history/send, and channel removal followed by denied
history/replay (0.08 s). All three existing room-history boundaries still pass
(0.37 s). Three frame-fitting cases pass, including an 80-message Unicode channel
page (0.13 s); shared channel parsing cases pass. Frontend production build and
CSS gate pass with generated actions; mounted channel UI remains a later slice.
Forty affected socket/directory frontend cases pass after recomputing both fixed
surface digests (ordinary and intentionally downgraded) for the new action registry.
Digest and bootstrap-authority rejection assertions are preserved. All affected
Rust targets/features pass Clippy; unchanged structure, 19 policy, formatting,
diff and artifact gates pass. The dispatch groups atomic updates without runtime
follow-up under one match, preserving profile/role command behavior and line gates.
