# Conversation context and external attachments

Status: implementation in progress, 2026-09-22. User requests feature commits and
actual application/MCP verification after implementation.

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
- No implementation or new-flow verification claimed yet.
