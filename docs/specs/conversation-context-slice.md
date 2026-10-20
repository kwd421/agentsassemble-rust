# Conversation context and external attachments

Status: implemented and verified for the tested flows, 2026-09-22.
Feature commits and actual desktop/MCP/Codex execution are recorded below.

## Contract

- Optional replies identify one existing public message in the same room/channel.
  The room transaction validates the target and owns the new event. Store only the
  target ID, never a copied quote that could survive source editing/deletion.
  The UI offers reply selection/cancellation, source preview when loaded, and
  navigation through the existing authorized context reader. Ordinary messages,
  uncertain-send retry identity, and agent freedom to choose whether to reply remain.
- External Room Connector participants can read published lobby attachments and
  upload pending attachments for their own messages. Existing asset validation,
  size/count limits, room/session authorization, upload custody, deletion, and
  transactional message binding remain authoritative. MCP returns actual image/text
  content using the existing provider attachment representation. No arbitrary URL
  fetching, local file access by remote MCP, or authentication bypass is introduced.
- Optional `room_status` and native `read_room_status` tools expose bounded public activity and active poll state,
  including the caller's ballot. Existing UI typing/tool details are reused.
  Information does not force an agent to speak, wait, or terminate early.
  Twenty poll candidates are examined per page, with an exclusive sequence cursor
  across expired polls. Both native and attendee reads revalidate the exact active
  turn. Reads never create events or change scheduling.

## Ownership and failure semantics

- Domain owns wire shape; persistence owns reply target validation and attachment
  custody; server owns authenticated transport; provider owns MCP representation;
  frontend owns selection and rendering of canonical records.
- Missing, deleted, cross-channel and cross-room reply targets reject atomically.
  A previously accepted reply remains readable when its source is later deleted;
  the UI does not retain a copied source body. Reply metadata is removed when the
  reply itself is deleted. A retry of a committed send remains the same operation.
- Revoked, muted, or read-only participants cannot obtain new write effects.
  Upload alone is not publication; a send binds only that participant's pending
  assets. A failed read/upload is an explicit error. Uncertain writes retain their
  existing request identity. No extra polling or compatibility fallback is added.
- Reply metadata is additive. External pending uploads need participant custody:
  the prior table requires a human profile. Schema 70 → 71 adds an empty
  `room_connector_uploads` table after host authority verification and preserves
  existing rows and the original bootstrap receipt/digest. Sending moves an owned
  upload into the existing bound attachment table in the message transaction.
  Both pending stores share the one-hour expiry and absolute storage quota.
  Upload transport uncertainty remains explicit; uploads have no command receipt,
  matching the browser upload contract. No user-data reset is performed.
- Non-goals: nested thread channels, enforced speaking order, model behavior rules,
  new AI providers, redesigning existing panels, and public hosting changes.

## AI file parity (2026-10-13, required behavior)

An AI participant has the same current room per-file limit (10, 25 or 50 MiB)
and storage quota as a human, for uploads and reads. Managed portal, attendee
client and connector stdio gain a workspace-path upload producing pending IDs
publishable through their existing say tools. Hosted connector retains inline
base64 upload at 10 MiB and directs larger uploads to the CLI. Conversation-only
agents reject workspace operations explicitly. Resolve symlinks and `..` inside
the configured workspace; accept regular files only, check declared size before
reading bytes, and reserve quota at begin. Reuse `message_upload` begin/data/finish,
canonicalization, raster validation, previews, chunk storage and one-hour expiry.
Reuse the existing three-active-transfers-per-principal admission.

Reads at or below 10 MiB retain their inline representation. Larger reads up to
the current room limit stream through `message_attachment_reader` and
`download_budget` into an atomically published host-chosen workspace subfolder,
returning its path. Never overwrite outside that subfolder. Read-only agents and
hosted connector return `attachment_too_large` with actual and allowed sizes.
Authorization is revalidated per piece; cancellation/failure must not publish a
partial file. Transfer buffers stay bounded; existing raster validation is owned
by the human upload finish operation.

Existing managed read budgets remain ten distinct IDs per turn, two attempts
per ID, and 200 MiB successful aggregate bytes; these include saved-path reads.
Path uploads use the existing 32 successful-room-tool-results per-turn admission
and ten attachments per-message limit. Room-owned tracked file tasks release
the command loop during disk I/O and retire on turn response loss or shutdown.
Attendee and connector budgets were verified at their owners before
their surfaces were implemented. No schema or frontend change is intended.
Code evidence: the human reader admits already-bound files up to 50 MiB even
when upload policy was subsequently lowered. Saved AI reads follow that shared
reader policy; the current room setting is enforced by the upload owner.

Affected entry points: managed `upload_attachment` / `read_attachment` /
`publish_message`, provider command ingress/runtime and canonical turn completion;
attendee room tools and client transfer; connector MCP, stdio client and HTTP
transfer. Implement and verify managed first, then attendee, then connector.
Acceptance per surface: 30 MiB in a 50 MiB room uploads byte-exact, binds and
counts toward quota; 10 MiB-room rejection occurs before source reading; escapes,
symlink escapes, non-files, conversation-only permission and exhausted quota fail;
large saved reads are byte-exact and read-only reads fail with the specific code;
reader/writer seams demonstrate bounded transfer. Real-provider and packaged
verification remain separate from controlled test evidence.

Managed surface (a): implemented with controlled native MCP/server verification.
`cargo test -p agentsassemble-persistence -p agentsassemble-provider -p
agentsassemble-server` exercised the affected suites; final isolated `cargo test
-p agentsassemble-server` passed (529 persistence, 305 provider, 196 server
library, 5 server binary and 204 integration tests; four existing ignored tests).
The native fixture uploads, publishes and reads exact 30 MiB data. Source-reader
probes cover 64 KiB bounds and pre-read admission; cancellation/shutdown releases
staging. `cargo fmt --all -- --check`, `make architecture-check` (19 policy tests)
and `git diff --check` pass. Earlier overlapping runs hit shared fixture
lease/peer failures; the isolated server rerun passed. No real provider, packaged UI or
Windows verification. Existing human raster finish still buffers encoded images;
transfer buffering remains bounded. Attendee (b) budgets verified at their owners before implementation: path
uploads share the provider portal's existing 32 successful-room-tool-results
per-turn admission and ten-attachments-per-message limit; reads share ten
distinct IDs, two attempts each and 200 MiB successful aggregate bytes,
including saved-path reads; host transfer admission (three active transfers
per principal, one-hour pending expiry, quota reserved at begin) and the shared
download budget (encoded/stream leases) are the same owners as managed.
Affected entry points for (b): attendee `UploadAttachment` client tool and
attachment read, host `tool/upload/*` + `tool/attachment/download` transfer
endpoints, `AttendeeTurnOutcome::Message` attachment IDs, join capability flag.
Attendee (b) landed: client path uploads stream through the host transfer
owner with pre-read limit/quota admission and publish through report IDs;
large reads save byte-exact into `.agentsassemble-attachments` on the client
machine; read-only agents and older hosts keep the explicit refusal.
Connector (c) budgets verified at their owners before implementation: path
uploads share the host upload owner's three-active-transfers-per-principal
admission (per connector participant identity), one-hour pending expiry and
quota reserved at begin; the current room per-file limit (10/25/50 MiB) is
enforced at begin and finish on the new transfer path, while the inline
base64 path keeps its fixed 10 MiB bound and its existing exemption. Reads at
or below 10 MiB stay inline; larger reads stream through
`message_attachment_reader` (50 MiB shared reader policy, authority
revalidated per piece) under a stream lease from the shared download budget
(1 MiB fixed weight; per principal 16 concurrent encoded, 32 streams,
400 MiB weight; host-wide 512 MiB) reserved at open before any byte flows.
Read calls charge one history event against the existing socket-admission
window (per principal 10 requests/1,000 events per 10 s). Affected entry
points for (c): connector `room_upload_attachment_from_path` CLI tool and
`room_read_attachment` saved reads, `RoomConnectorClient` transfer methods,
host `/api/room-connector/tool/upload/*` +
`/api/room-connector/tool/attachment/download` transfer endpoints,
`MessageUploadAuthority::Connector` / connector reader authority, join
`attachment_transfers` capability flag, `room_say` attachment IDs. The
hosted/HTTP connector keeps inline base64 upload at 10 MiB and directs larger
uploads to the CLI; older hosts omit the capability flag and keep inline
behaviour.

## Verification matrix

| Flow | Required evidence |
| --- | --- |
| Reply to a loaded message | UI select/send/preview/source navigation; persisted ID |
| Reply to older history | Authorized context load and exact source navigation |
| Reply failure/retry | Invalid target rejected without event; committed retry deduplicates |
| Source edit/delete | Updated/tombstone source shown without copied old text |
| AI reply | Optional MCP parameter reaches the same canonical reply field |
| External attachment read | Joined MCP reads actual published image/text; foreign/unpublished/revoked rejected |
| External attachment upload | MCP upload/send renders in desktop and is readable by another authorized participant |
| Attachment lifecycle | Existing count/size/type limits and pending-owner checks still apply |
| Activity and polls | Running/finished and open/closed states agree with server; own ballot is accurate |
| Restart | Messages/replies/attachments persist; session admission remains required |

Evidence levels: code-evident until executed; execution-verified only for the exact
tested flow. Partial or unavailable real-provider/network coverage is stated as such.
Feature commits remain independently buildable and below the repository diff cap.
Affected tests, architecture/source-growth/format, build, Clippy and final real UI/MCP
checks are recorded here as they complete.

## Evidence

- Baseline: clean `6f7707e`, equal to `origin/codex/recovery-and-sonnet`.
- Feature commits: replies `56f9d74`, attachments `e8e2427`, status `1500619`.
- Actual Windows desktop/MCP/Codex and restart verification: see
  `../verification/2026-09-22-conversation-context.md` for the matrix, corrections,
  exact evidence and coverage limits.

## Authorized feedback follow-up (2026-09-30)

Implement the verified observation gaps without changing frontend layout or
stopped-target routing. Preserve bounded public MCP rejection text in API tool
results so the model can correct its call. Existing public-error owners sanitize
internal failures; do not forward raw transport exceptions or private data.
Resolve reply author and a 200-character excerpt from the current same-room lobby
source during assignment, within the transaction and observation character budget.
Missing/deleted sources remain explicit unavailable pointers, never stored quotes.
Search results add attachment IDs paired with their existing canonical filenames;
read authorization remains with the attachment owner. Update strict consumers
without changing presentation. Explain status reads, tabletop-only randomness and
managed upload/media limitations at the existing observation owner.

Acceptance: controlled MCP-to-API rejection survives to the next model request;
search IDs match canonical attachments; reply previews reflect edits/deletions and
remain bounded, including old sources outside incremental context. Verify affected
Rust and frontend contract tests plus mandatory gates. No provider call, new retry,
database migration or frontend redesign is required by these changes.

## Agent identity, roles and addressing (Task AB, 2026-10-12)

The server-built next-turn `read_discussion` view identifies the caller's canonical
room participant ID and current participant role, and shows roles beside other
agent handles. `read_room_status` projects current participant roles even during
an active turn. Promotion, role replacement and removal of director status use
the canonical participant record; assigned observations/retries remain immutable.
Director means facilitating conversation, inviting relevant participants and
summarising when useful with existing tools; it grants no operator authority or
scripted meeting orchestration. Role changes do not start provider turns.

Messages matching the existing routing mention parser are marked as mentioning
the caller independently of ordered-floor selection. Room identity is independent
of provider-session identity. Instructions distinguish content `<@ID>` addressing,
`reply_to_event_id` quoting (no floor handoff by itself), and `next_agent_id` floor
handoff in three plain sentences. Passing remains available. No ID migration,
new alias fallback, routing changes, capabilities changes or provider calls.

Owners: `room_turn_context.rs`, `room_turn_routing.rs`, `conversation_status.rs`
and its domain projection. All additions share the existing observation budget.
Acceptance: two regression invariants exercise promotion/demotion through next
API context and live status with unchanged capabilities, and picker self-mention
delivery with a different provider identity, including multiple mentions. Run
affected crate tests and mandatory architecture/structure/format gates.

Verification: `cargo test -p agentsassemble-domain -p agentsassemble-persistence`
passed (66 domain, 478 persistence; one existing ignored test). `cargo fmt --all`
and its check, `make architecture-check`, and scoped `git diff --check` passed.
Affected-crate all-target Clippy remains blocked only by existing `similar_names`
and `too_many_lines` violations in `restore_job_tests.rs` (273 and 61); unchanged.
No generated TypeScript changes, live provider calls or packaged UI verification.

## Participant identity and human roster in AI room views (2026-10-20)

Required behaviour: every rendered message author identifies both the current
display name and the canonical room participant ID, as `Name <@participant_id>`
(for example `#12 Nel Le <@p-7f3a> [event abc]: hi`). The rule applies to the
server-built room view, in-flight/pending lines, the reply preview, persona recent
messages, `read_message_context`, and search results. Two people who share a
display name remain distinguishable. The room view header keeps its existing
`Agent handles` line and adds a bounded `People` line listing currently joined
human participants with the same `Name <@participant_id>` form; left, kicked and
other non-joined participants are omitted. Display names remain untrusted room
content: the existing escaping/clipping stays, and CR/LF in a name is replaced
with a space so it cannot forge another line. Mention parsing and ordered-floor
routing are unchanged. Attendee room views use the same persistence observation;
connector and attendee search/context reads already expose the structured event
actor or `participant_id`, so no separate line format is introduced there.

Owners and affected entry points: `room_turn_context.rs` (`load_room_roster`,
`render_room_view`, `load_reply_preview`, `render_recent_messages`,
`bounded_pending_prefix`) and `room_portal_render.rs` (`search_page`,
`message_context`) with their existing tests. Acceptance: affected persistence,
provider and server library tests pass, including same-name humans, the People
line, and a newline-containing display name; `make architecture-check` passes.
No schema, routing, frontend or provider-call change.
