# Persona-card library and Agent Session selection

## Definition

Reconnect the copied persona picker to one Rust-owned import library and apply the
selected normalized persona to ordinary Agent Session turns. This is the Risu/CCv3/
CHARX product flow, not the excluded scripted-meeting pipeline.

## Verified original behavior

The authority baseline is reachable code at original commit
`d5046473010d1353a81ee38337360e6d98f7bd6f`, not old product markdown:

- the local operator can list, import, and read thumbnails for `.json`, `.png`,
  `.apng`, `.charx`, and `.risum` assets through the copied picker;
- one upload is at most 10 MiB and resolves to a normalized card or Risu module;
- CCv3 and CHARX card fields, literal lore entries, and Risu module fields are
  normalized before selection; remote/file/unknown asset URIs are not fetched;
- scripts, triggers, custom regex execution, CJS, MCP declarations, and low-level
  module access are preserved only as ignored-feature metadata and never executed;
- list and Agent Session projections expose a safe summary, not private prompt bodies;
- direct Agent Session creation and stopped-session configuration select or clear one
  library asset; API and Local catalog groups expose this control;
- an ordinary turn renders the selected card against canonical recent room messages,
  applies bounded literal lore, and includes the result in the server-owned provider
  input. The reachable call does not persist a second sticky/cooldown lore state.

## Authority and invariants

- The persona library owns normalized private card content and optional safe thumbnail
  bytes. Its public summary has one schema and is derived from that owner. Imported raw
  scripts or private bodies never enter events, logs, catalog data, or public summaries.
- The Agent Session owns only its selected persona ID and the matching safe summary.
  It is not merged with the owner's left-bottom human profile, participant role, room
  mute, join state, or room permissions.
- Selection is accepted only when the ID resolves in the current library and the
  provider's current catalog group supports personas. Missing, corrupt, mismatched, or
  unsupported selections fail; they do not silently clear or fall back to a model
  default.
- Creation persists the selected ID atomically with the real Agent Session. A stopped
  session may atomically replace or clear it. A running session cannot be reconfigured,
  matching the existing runtime-profile authority.
- Each durable turn stores the exact provider input produced from the selected card and
  the canonical bounded event prefix. Restart/retry reuses that durable turn envelope;
  it does not reread a changed library asset and alter an in-flight turn.
- Persona prompt construction is one provider-neutral room-context operation. Provider
  adapters receive the resulting ordinary turn input and do not implement separate
  Risu/CCv3/CHARX or provider-specific prompt branches.

## Import and storage boundary

- Authenticate the exact local-operator purpose before accepting the bounded import
  body. The copied frontend must use the current typed local authority rather than an
  original host token, client-only state, or an authentication bypass.
- Decode base64 and archives with explicit encoded, decoded, entry-count,
  per-entry, total-expanded, and compression-ratio ceilings derived from the reachable
  10-MiB upload boundary. Use a maintained ZIP implementation; do not implement archive
  framing or compression.
- Reject unsafe or duplicate archive paths, encrypted entries, malformed length fields,
  invalid UTF-8/JSON, invalid IDs, duplicate normalized IDs with inconsistent state, and
  decompression-bound violations. Never resolve archive paths onto the host filesystem
  and never fetch remote or `file:` URIs.
- Persist only product-reachable normalized content, ignored-feature counts needed for
  the safety notice, and an optional verified image thumbnail. Do not retain a second
  raw-source copy or arbitrary unused embedded assets merely because the Python tree did.
  This is a disk/custody simplification, not a compatibility migration.
- Commit an import atomically. A failure leaves neither a selectable partial card nor a
  partial thumbnail. Reimporting the same normalized ID replaces that exact library
  entry atomically; it does not evict unrelated entries.
- Persona assets have their own library custody and limits. They are not profile,
  pre-join, room-appearance, or message attachments and do not consume or weaken those
  owners' absolute accounting.

## Prompt boundary

- Build character name, system/persona instruction, description, personality,
  scenario, literal lore, example dialogue, first-message style, recent room context,
  and post-history instruction from normalized fields with explicit per-field and total
  bounds.
- Replace only the reachable literal variables (`{{char}}`, `<char>`, `<bot>`,
  `{{user}}`, `{{persona}}`, and supplied literal slots). Unresolved slots remain text;
  they are not code or template execution.
- Literal lore honors enabled/always-active/selective, primary and secondary keys,
  case and whole-word settings, insertion order, priority, scan depth, recursion bound,
  deterministic probability, and the current bounded recent-message input. Regex lore
  remains ignored. No background scan, cache, generic rules engine, or persistent lore
  state is added.
- Persona context remains lower priority than room rules and provider safety contracts.
  It cannot broaden Room Portal tools, filesystem access, network access, permissions,
  or participant authority.

## Non-goals

- No v0 research, agenda, forced rounds, moderator synthesis, decisions, tasks,
  meeting artifacts, work/artifact persona surface, or meeting-specific persona mode.
- No execution of imported scripts, regex replacements, triggers, CJS, MCP, HTML,
  low-level access, or remote asset retrieval.
- No persona deletion/editor/marketplace, operating quota configuration, generic import
  framework, provider plugin framework, or PostgreSQL migration in this slice.
- No compatibility read of the Python persona directory and no legacy fallback.

## Acceptance criteria

- A fresh local operator imports representative CCv3 JSON/PNG, CHARX, and Risu module
  fixtures through the real copied picker, sees safe summaries/thumbnails, and can
  explicitly select, replace, and clear them.
- Unsupported, malformed, oversized, encrypted, traversal, duplicate-entry,
  compression-bomb, remote/file-URI, and executable-feature cases fail or remain
  inert at the exact owner without partial durable state.
- Selection is durable across restart, rejected for unsupported providers, available at
  creation and stopped-session configuration, and never changes the human profile or
  room-owned role/mute/permission state.
- Ordered and ambient ordinary turns prove that the provider receives bounded persona
  fields and matching literal lore from canonical recent messages, while imported
  executable features do not run. In-flight retry uses the exact durable input.
- The copied frontend, HTTP/TCP authority, persistence, provider-neutral turn context,
  packaged local flow, and at least the authorized real provider matrix are verified.
  Computer Use owns only that packaged verification run and is fully cleaned afterward.
- Every independent commit remains below 1,000 changed lines. Push and cross-review occur
  at three completed product features or 2,000 aggregate changed lines, with structure,
  duplicate policy, overimplementation, lifecycle, performance evidence, and security
  included in both critical-web and Daybreaker Blue High review requests.
