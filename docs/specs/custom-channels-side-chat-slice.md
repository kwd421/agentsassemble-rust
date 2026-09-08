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

## Side-chat bootstrap transport

`GET /api/side-chat?room_id=...` reads the bounded memory snapshot with private,
no-store responses. Public human/paired bearer authority uses the existing origin,
device and current-session resolver. The bundled desktop uses a new exact-purpose,
one-use read ticket, resolved to its current local identity inside the same
transaction as the memory read. Wrong room, wrong purpose, consumed ticket and
revoked session remain errors. The native control response must match both its
purpose and request; search and side-chat response decoding share that mechanism
without accepting one another's grants. Only the bundled main window receives the
new native permission, and the advertised host surface is its actual registry/
capability intersection.

Actual HTTP acceptance passes (0.17 s), including all 200 maximum-size Unicode
messages, a body larger than the unchanged WebSocket frame limit, no-store headers,
local ticket reuse/wrong-purpose rejection, and read-only human read/wrong-room/
leave rejection. Desktop Clippy and all 28 native tests pass; the surface check
caught the initially missing bundled-window permission, which was then registered.
Fifteen affected frontend native-bridge cases and the production build/CSS gate
pass. Snapshot/message bindings are generated from Rust. Affected Rust Clippy and
unchanged structure, 19 policy, formatting, diff and artifact gates pass.

The full bootstrap travels over HTTP because 200 Unicode text messages can exceed
256 KiB. Live delivery will send one bounded update per WebSocket frame after
subscription-before-bootstrap; no enlarged frame gate or polling was introduced.
The dock and live mutation/subscription remain subsequent Phase 6 work.

## Side-chat live transport

Human room sockets may explicitly request `side_chat` beside `room_events`. The
memory subscription is installed before the subscription receipt and room bootstrap
finish, so a subsequent HTTP side-chat bootstrap overlaps the live stream safely.
Both subscription and each private delivery revalidate human read authority and
the exact room UID. Bootstrap now includes its authoritative `room_uid`; the client
parser requires the expected UID, distinguishing a recreated room from a restarted
process. An old room subscription cannot attach to a newly created namesake.

`side_chat.send` uses the existing principal mutation/inflight budget and the memory
append owner. It never enters the durable room queue/result or provider publication
owner. Its ACK contains one private update; live frames carry one update and never
advance the room event cursor. A lagged private receiver emits an explicit private
resync request and closes for bootstrap/reconnect. No timer or independent task is
added. Shared direct socket dispatch handles history, vote summaries and ephemeral
sends while their distinct owners retain their contracts.

The frontend negotiates only declared streams, validates private updates and ACK
generation/author scope, and rejects unsolicited private frames. Sequence checking
is shared; side-chat snapshot validation additionally requires the complete retained
prefix. Rust generates stream lists, side-chat types and bounds. The dock has not
yet been mounted, and bootstrap merging/composer/packaged acceptance remain open.

Actual TCP tests pass for bootstrap overlap, exact replay, reconnect, non-subscribed
observers, absence from durable history, read-only delivery/post denial and leave
closure (0.06 s). HTTP bootstrap still passes (0.19 s), as does the affected vote
route (0.05 s). Four memory-owner cases pass (0.10 s), including rejection of a
retired UID after same-name room recreation. Existing room-history transport cases
and six protocol cases also pass.

Affected Rust all-target Clippy, 44 frontend transport/surface cases (3.48 s),
the production frontend build and CSS check pass. Unchanged structure, 19 policy,
formatting, diff and artifact gates pass. Packaged UI acceptance remains pending.

## Side-chat frontend connection

The human dock is mounted below the active channel and can be expanded without
covering its conversation. It restores the original human-only notice, plain-text
mention/emoji insertion and scoped drafts. Current canonical membership, mute and
message capability control its composer; failure retains the draft and success
clears it only after the server receipt. No author name is supplied by the client.
Drafts remain private memory scoped to HTTP authority, room ID and room UID.

The existing canonical room socket forwards its accepted UID, private updates and
closure to the side-chat hook. That hook owns one abortable HTTP bootstrap per
connection and at most 200 buffered live updates. It merges the HTTP cut with live
sequence/generation/floor checks, immediately hides old history when room or login
authority changes, and exposes failed bootstrap or gaps with an explicit reconnect
action. There is no added poll, timer, socket or local persistence. ACKs can precede
earlier queued live frames, so the subscribed live stream owns transcript order;
ACKs confirm sends without inventing a gap or duplicating messages.

Affected frontend acceptance covers stale native grants, private/no-store HTTP,
overlapping bootstrap/live retention, ACK-before-live ordering, generation and
sequence failure, authority/reconnect isolation, room-incarnation drafts, and the
composer's rejection, success, focus and read-only state. Canonical room lifecycle,
creation, controls and isolation regressions also pass. Packaged desktop/mobile
acceptance, custom-channel presentation and channel search/pins remain open.

All 37 affected frontend cases pass (3.64 s); the production build and unchanged
CSS cascade pass. The dock adds about 3 KiB of compressed frontend code, one
bounded private projection and one in-flight bootstrap. Structure, 19 policy,
formatting, diff and artifact checks pass; visible packaged behavior is not yet proven.

## Channel client contract and retained window

The channel client uses generated text bounds and the 80-event default page size.
Its ACK validator binds channel history and send responses to the requested room,
channel, cursor, current author and channel-message schema. Channel room sequences
may be separated by other room events; they are strictly ordered but need not be
consecutive. These records remain excluded from the lobby transcript projection.

A selected-channel hook owns one bounded 200-message window and buffers live events
while the initial page is pending. It adds only events after the page's room cut,
keeps older navigation stable, indicates newly arrived messages and reloads latest
history on request. Room, UID, channel and canonical connection changes invalidate
late reads and send receipts. It adds no poll, timer, second socket or per-channel cache.
The canonical hook exposes accepted room events to this owner before its own
bounded React event window can discard older delivery.

An actual client regression reproduced an existing retry defect: a pending channel
send was replayed after a native reconnect resolved a recreated namesake room. The
shared socket now binds its accepted room UID, rejects old pending outcomes as
unknown and reconnects from cursor zero before accepting that new room's history.
The correction prevents both old intent replay and old/new transcript mixing;
ordinary reconnect retry semantics are unchanged. The failing and corrected cases
are captured by `roomSocketClient.channel.test.ts`.

Affected socket/canonical/hook cases pass (106 unchanged cases plus the corrected
incarnation case); the production frontend build/CSS and protocol Clippy pass.
The added window remains bounded at 200 events; the 300-message burst test confirms
retention. Unchanged structure, 19 policy, formatting and artifact gates pass.
Channel mounting, search/pins and packaged acceptance remain subsequent work.

Retired channel tombstones remain valid room-history sequence records after the
server erases their text, but are rejected as current channel history or send ACKs
and never enter the selected-channel feed. Six affected client cases, the
production build/CSS and unchanged mandatory gates pass for this correction.

## Shared channel search and context

The canonical message index now includes current custom text messages alongside
lobby messages. Existing normalization, Unicode/short-query/attachment matching,
30-result cursor pagination and 20-neighbor context remain one owner. Human/native
reads select an exact registered text channel or the readable `all` union; context
requires one concrete channel. Results carry their canonical channel ID. Channel
retirement removes index entries atomically with its event tombstones, so an explicit
read fails and union search cannot expose retired content. Provider room tools remain
bound to lobby history and cannot resolve custom-channel context.

The clean database version is 67 to identify the expanded index contract; earlier
versions fail through the existing version owner without an implicit reindex or
migration. No new table, poll, task or parallel search cache is added. Custom messages
add the same bounded record/FTS entry already used for lobby messages, and reads
retain the existing page/context limits. Shared HTTP responses now serialize domain
projections directly instead of rebuilding them with a hardcoded lobby ID.

Five actual HTTP cases pass (0.11 s), including native one-use tickets, read-only
human channel/union searches, 30+1 pagination, exact context and retirement. Seven
persistence search/mutation/provider cases pass (0.12 s), four channel cases pass
(0.05 s) with search after reopening the file database, and the actual MCP search/
context receipt case passes (0.02 s). Affected Rust all-target/all-feature Clippy,
unchanged structure/19 policy cases, format, diff and artifact checks pass. The
search owner exceeds 500 lines because it cohesively owns the bounded query and
context paths; the transport projection was reduced by 81 lines. Frontend channel
search parsing, pins, mounting and packaged acceptance remain open.

## Channel-scoped pins

The existing pin pointer transaction now takes one concrete message channel. Current
registration, exact event/channel identity, read/write session authority and the
64-pin limit are checked at their existing owners; each channel has its own capacity.
Read and mutation responses serialize the canonical pin projection directly with its
channel ID. Search/context and pins share the persisted event-channel selector,
while custom history retains its dedicated indexed query. There is no copied pin
table, additional cache, poll or subscription. Removal uses the channel retirement
transaction's existing pin cleanup; reopening the database preserves surviving pins.

Four actual HTTP pin cases pass (0.07 s), including independent channels, wrong-channel
unpin rejection, lobby isolation and retirement. The five search HTTP cases remain
passing (0.10 s). Eleven affected persistence cases pass (0.21 s), including canonical
attachment projection, mutation rollback, read-only/revoked authority and a custom
pin when the lobby already holds all 64 pins. Four channel cases pass (0.05 s), with
history, search and pins checked after a file database reopen. Generated pin bounds,
15 frontend pin regressions (1.03 s), production build/CSS, affected Rust all-target/
all-feature Clippy and unchanged structure/19 policy/format/diff/artifact gates pass.
Pin reads remain at most 64 projections; the room-owned pointer scan can cover the
bounded set of 50 custom channels plus lobby. The public client parser and custom
channel mounting remain the next slice; packaged acceptance is still pending.

## Channel search and pin clients

The existing search and pin clients now accept an explicit lobby/custom channel.
A shared selector validates the outbound identity before native grant consumption.
Search pages bind every result to the requested channel, or permit the canonical
union when `all` was requested. Context binds the enclosing response and every
custom event to the same room/channel and reuses the canonical public event parser;
retired tombstones and private/extra fields fail. Pin lists and mutation receipts
bind every pointer to the requested concrete channel. The lobby callers now pass
`lobby` explicitly; there are no compatibility wrappers or alternate routes.

All 28 affected client/view/window cases pass (1.11 s), including mixed union results,
wrong-channel/context rejection, retired/private records and pin receipt scope.
Production frontend build and unchanged CSS, structure/19 policy and diff gates pass.
React review found no added state, effects or subscription; this extends existing
bounded projections and adds approximately 0.14 KiB compressed code. The custom
channel screen and packaged verification remain the next work.

## Shared pin lifecycle and explicit context selection

Lobby and custom channels share one on-demand pin hook scoped to room UID, channel
and HTTP authority. It owns the bounded visible list, operation/error state and
pre-dispatch/late-response fences. A retired request cannot block or overwrite the
new channel's request; the lobby's duplicated pin plumbing is removed. No automatic
refresh, cache, timer or subscription is introduced.

Context reads now take the selected result's concrete channel separately from the
search scope. This fixes the reachable `all` search path which previously sent
`all` to the concrete-context endpoint. Room UID changes retire pending context and
search state. The selected-channel window can display that bounded server context
and return to latest history; it does not invent a context paging flag. An explicit
context selection invalidates a pending history page's publication, while preserving
send ownership and keeping later live arrivals indicated outside the older window.

All 39 affected hook/view cases pass (1.72 s), including late context/history ordering,
room-recreation/login/channel pin custody and independent new-scope reads. Production
build/CSS and unchanged structure/19 policy/diff gates pass. React review confirms
that operation state belongs to the scoped hook, with cleanup and no added periodic
work. Mounting and direct packaged acceptance remain open.

## Canonical custom-channel view

The copied custom-channel view now consumes the selected-channel socket window and
shared pin/search owners. Its text composer sends only channel/content, retains
failed drafts, clears after the receipt and returns focus. Read-only or unready
channels cannot send. History supports explicit older/latest navigation; exact
search and older-pin context reuse the bounded window and preserve scroll/focus.
Late context cannot replace a different connection's view. A temporary unavailable
channel projection shows a connecting state while preserving the same identity's
draft; different room/channel/login identity never displays that draft.

The obsolete custom HTTP polling and voice presence/join/heartbeat body are removed
from this directly replaced component. Voice remains deferred and is not mounted.
The view reuses the original CSS with 24px interior spacing and 44px controls, and
adds no poll, timer, socket or cache. Twelve affected view/pin/window cases pass
(1.09 s); the final four composer/navigation cases also pass after correcting the
mocked pin receipt to match its target. Production frontend build/CSS and unchanged
structure/19 policy/diff gates pass. The view is not mounted in the app shell yet;
that wiring, create-channel dialog and direct packaged acceptance remain open.

## Mounted channel flow

The app now derives its text-channel list and labels from accepted room settings,
opens the canonical channel view, and feeds its single bounded window from accepted
room events. Removing the selected channel returns to lobby after authoritative
settings arrive; temporary disconnection keeps the same channel view and draft.
Channel search uses the selected channel, room-wide results route to their concrete
channel, and lobby/custom pin and context lifetimes receive the room UID.

The create entry uses the existing settings command and its revision/receipt checks.
A native modal owns focus, keeps failed drafts, blocks duplicate submission and
dismissal while saving, and navigates only after the returned settings contain the
created text channel. Room/login changes retire the modal and its navigation. The
retired voice selector and unused custom pin stylesheet are removed; common styles
and local layout preserve the unchanged approved CSS cascade.

Twenty-five affected channel/view/search/model cases pass (the combined run took
1.88 s); two new modal cases pass separately (0.54 s), covering failure/receipt,
busy dismissal, composing Enter and late completion after unmount. Production
build, unchanged CSS cascade, structure/19 policy, formatting, diff and artifact
checks pass. React review confirms one channel window under the existing socket,
without polling, per-channel cache or another subscription. Mounted code adds
about 4.15 KiB compressed to the main bundle. Packaged desktop/mobile acceptance
and complete-phase resource measurement remain open.
