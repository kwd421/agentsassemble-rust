# Real external invitations — 2026-09-30

## Scope and environment

User requested actual normal-use verification of external human and external AI
invitations. Source baseline: `174ae645`, `codex/recovery-and-sonnet`. Built and
Developer-ID signed an isolated macOS Tauri package from that baseline; verified
the signature. Used a separate application identifier/profile, a disposable room,
and the actual managed Cloudflare public tunnel. Human participation used the
in-app browser. No existing rooms, credentials, frontend layout or provider
configuration were replaced.

External AI execution used the installed Antigravity 1.2.2 CLI through the current
Room Connector MCP and configured Grok 4.7 through `assemble room attend`.
The app/server stayed on the baseline; attendee corrections were rebuilt into the
actual `target/debug/assemble` CLI and tested against that running package.

## Observed results

| Flow | Actual result |
| --- | --- |
| Human invite | Host issued a one-use invitation; external browser selected/cropped a photo and joined. |
| Deferred avatar storage | Before Join, profile/prejoin asset counts were 0/0; after Join, 1/0 and invite use 1/1. |
| Human conversation | Guest received host history, sent a message and uploaded `attachment.txt`; host displayed both. |
| Browser reconnect | Reload preserved the admitted identity, room history and attachment. |
| Attachment download | Guest clicked Download; the resulting Downloads file SHA-256 matched the original: `fe14c2ad74dbc28690a49b94037d7f6aa0e02f243e8c1ac3db8ad7a6439261d9`. Browser automation's download-event wait timed out, but the actual saved file was verified. |
| Current AI conversation | Antigravity used actual MCP join/read/read_attachment/say calls; host and guest displayed `INVITE-ATTACHMENT-174ae645` and `17+19=36`. |
| Saved AI friend | Created a Grok external friend and issued its provider-bound invitation. Initial launch failed as described below; corrected CLI joined and published `42` in reply to 31+11. |
| Guest companion | Human issued a Grok companion invitation; corrected CLI joined and answered 23+19=42. |
| Historical attachment | Initial later-message read failed as described below. Corrected CLI rejoined, searched earlier history and published the exact attachment first line. |
| Human leave | Browser confirmed Leave and returned to the admission screen; host removed the human and companion. Companion CLI exited and durable session became detached/stopped without recovery required. |
| Consumed/revoked links | After Leave, consumed original invite was rejected. A separate unused invite was issued, revoked through host UI, then rejected in the browser. |
| AI cleanup | Friend CLIs handled SIGINT with confirmed room cleanup. Antigravity confirmed room_leave and retained-receipt release. All four attendee session records ended detached/stopped, with no error or recovery requirement. |

The companion CLI printed a reconnect rejection after owner departure; it still
performed cleanup and left no active durable runtime. This is not a clean-success
CLI exit claim. The guest room menu exposed Leave/read-state actions, not host
management. This UI observation is not an exhaustive authorization audit.

## Reproduced defects and correction

1. The external attendee CLI constructed its adapter without a private provider
   state root. Grok admission succeeded, then startup failed with
   `provider_state_unavailable`. The CLI now owns a separate private temporary
   provider-state directory through runtime stop and remote cleanup confirmation;
   uncertain cleanup retains it. Explicit user workspace selection remains intact.
   The attendee factory binds the exact helper executable on Unix and Windows.
   `assemble` also handles the existing private managed-worker entry before public
   CLI parsing, required by its Windows worker. Missing-custody invocation rejects
   through that entry rather than the public argument parser.
2. External room observations only connected attachment ingress when the new input
   itself included attachments. A follow-up asking about an earlier upload thus
   returned `The room attachment owner is unavailable.` The observation now keeps
   the existing attachment relay connected; the remote server still authorizes
   visibility using the exact turn and connection. No local filesystem authority,
   authentication bypass or fallback was added.

## Verification and limits

The existing attendee execution integration now uploads before admission, sends
a later attachment-free input, asserts empty input attachment IDs, and reads the
earlier file through actual native MCP/HTTP. It also retains socket replacement,
single-execution/report replay and random-result replay assertions. Its obsolete
JSON search expectation was corrected to the current compact text contract.
This integration and the exact-custody cleanup integration both pass. The latter
now executes the actual `assemble` helper with the attendee factory.

Affected provider/server all-target/all-feature Clippy, architecture/source gates,
19 policy/artifact-owner tests, formatting, diff and artifact checks pass.
No full provider suite or new external source review was claimed.

This verifies macOS with an external public URL on the same physical computer,
not a second physical machine or actual Windows execution. Other real providers,
mobile layouts, exhaustive permission attacks, long-idle recovery, and the two
previously documented runtime cause-loss defects are outside this result.
The CLI was executed from the build output; this machine has no `assemble` command
on PATH, so these results do not prove end-user CLI installation/distribution.

## Cleanup

Public ingress was closed through host UI. Test Antigravity exited and its temporary
MCP registration was removed; its original empty MCP configuration was restored.
The isolated app quit normally and its app/supervisor/server plus attendee PIDs
were absent. Computer Use was reset. Only this run's app bundle, isolated profile,
temporary invitation files and verified downloaded fixture were moved to Trash.
Sanitized public messages and final session states were captured under
`/tmp/aa-invites-20260930/evidence`. No invitations, bearer tokens, recovery codes,
provider transcripts or database copies are included in this document.
