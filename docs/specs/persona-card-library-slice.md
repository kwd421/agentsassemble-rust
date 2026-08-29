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

The provider-neutral scanner uses maintained Unicode default case folding on the bounded
search context once per recursive round. The earlier `lowercase` implementation could not
preserve the reachable Python `casefold()` contract (`Straße` did not match `STRASSE`).
Whole-word matching checks only the adjacent folded characters against the original Python
word set (Unicode letter/number or ASCII underscore), avoiding incompatible engine-specific
`\w` definitions and per-keyword regex compilation. Folding at the scan owner avoids
traversing the whole context once per case-insensitive key. Domain tests verify partial,
whole-word, and combining-mark-adjacent Unicode matches alongside literal, recursive,
substitution, and inert-regex behavior. No wall-time or CPU reduction is claimed without a
real-turn measurement.

The `CCv3` import boundary now parses PNG text chunks through the maintained PNG decoder
instead of owning chunk framing or CRC handling. The reachable upload is bounded to 10 MiB,
card JSON to 5 MiB, and decoder metadata allocation to 20 MiB. Only the one thumbnail needed
by the copied picker crosses the existing shared raster admission and is re-encoded as PNG;
the prior Python path copied the source plus every resolved asset into permanent custody.
This removes the duplicate source copy and arbitrary unused asset disk cost while preserving
the normalized card, safe asset count, preferred icon/avatar/portrait choice, and inert
remote/file/executable-feature contract. Tests exercise a real PNG card and prove that its
regex and imported runtime declarations remain counted but unexecuted. A malformed optional
thumbnail is counted and omitted rather than turning the otherwise valid card into an
executable or unverified image; malformed card/container data still rejects the import.

Standalone `.risum` parsing owns the RPack v0 byte permutation as fixed wire-format data,
verified against the official 512-byte encode/decode map hash, instead of searching operator
home folders, `/tmp`, or an environment-selected RisuAI checkout as the Python path did. The
10-MiB upload, 5-MiB decoded main body, 8-MiB aggregate asset-record body, 256-record, exact
length, marker, and terminal-EOF bounds are enforced before normalization. Since the current
product uses module text and lore but does not expose its arbitrary media bundle, asset record
bodies are range-checked without a second decoded copy or durable write; only the larger of
declared and present asset counts remains in the safe summary. This removes hidden runtime
configuration and unused memory/disk custody while preserving module selection, literal lore,
and ignored-feature counts. A real encoded module fixture verifies the format boundary and
that regex, trigger, CJS, MCP, and regex-lore declarations remain inert.

`CHARX` uses the maintained ZIP reader with only the stored/deflate/Bzip2/LZMA methods
reachable in the original standard-library reader. Before any selected member is expanded,
the archive owner rejects more than 512 entries, unsafe or duplicate normalized paths,
encryption, overlapping compressed ranges, entries above 10 MiB, aggregate expansion above
80 MiB, and ratios above 200:1. Only card-declared embedded assets are read and no archive
path is ever resolved onto the host filesystem; the shared card owner still selects and
canonicalizes one preferred thumbnail while continuing to validate and count every later
resolved asset without retaining it. A readable `module.risum` replaces the card lore only
when that module actually carries a lorebook array, as the original flow does; its
non-executable feature counts are merged once rather than double-counting regex lore at both
container and module boundaries. An unreadable optional module is counted and remains inert
without discarding an otherwise valid card. Real ZIP,
PNG, and RPack fixtures verify the combined path, and a traversal archive verifies rejection
before `card.json` is read.

## Manual-review findings

- Daybreaker Blue High returned five Medium findings for the first pushed importer range:
  absent module lore erased card lore; thumbnail selection stopped validating/counting later
  assets; Unicode case-insensitive matching used lowercase rather than casefold; the CCv3
  JSON size/root policy had two owners; and the persona-ID bound was duplicated in provider
  input validation. Commits `f57ec60`, `6e12ee8`, `6cc4980`, `0d859f3`, and `937492f`
  correct those owners without adding compatibility or fallback paths.
- The critical web review returned one Medium candidate requesting a durable recent-message
  count. It was not accepted: the actual ordinary-room caller at the fixed original baseline
  passes one rendered string, and the original `_recent_message_count(str)` contract is
  exactly empty/non-empty (0/1). Adding a count would change reachable behavior rather than
  preserve it.
- Both manual reviewers found one further Medium: Rust regex `\w` includes combining marks
  that original Python whole-word matching excludes. The correction now owns the original
  Unicode letter/number-or-underscore predicate directly and tests a combining-mark-adjacent
  match rather than depending on an engine-specific word class. Both reviewers then found
  that non-overlapping substring iteration could skip a later overlapping candidate; the
  same owner now advances one Unicode scalar after a failed candidate and tests that path.
  The critical web review then found the original full-word path's second
  `re.IGNORECASE` step: after full casefold, CPython's remaining unequal equivalence is
  ASCII `i` with dotless `ı`. The full-word comparison preserves that pair while partial
  matching intentionally remains plain casefolded substring search.
- The critical web cumulative review found one further Medium in the same prompt owner:
  Rust budgeted raw lore text while the original first removed leading `@@` decorator
  lines, allowing decorator length to displace higher-priority visible lore. Budgeting now
  owns the visible body used by rendering, and the sole oversized fallback is truncated
  before variable replacement, matching the original observable ordering and bound.
- Both reviewers then found that Rust `str::lines()` recognizes fewer boundaries than the
  original Python `splitlines()`, so a reachable CR-only, NEL, or Unicode-separated card
  could fail to parse a decorator and erase its visible body. One allocation-free iterator
  now owns Python-compatible line boundaries for both parsing and display extraction. This
  also avoids the prior temporary `Vec<&str>` whose memory grew with attacker-controlled
  line count; the bounded content string remains the only output allocation. A regression
  exercises every Python boundary, including CRLF as one separator.
- A post-approval local audit found the adjacent source-whitespace difference: Python card
  normalization treats U+001C through U+001F as whitespace while Rust's standard predicate
  does not. The same card-text owner now trims imported fields, lore keywords, and decorator
  lines with the exact set and bounds prompt normalization while scanning instead of first
  allocating every word from an input that can reach the 5 MiB card limit. Reachable import,
  partial-match, decorator, and visible prompt behavior are covered without new durable state.
- Daybreaker also found that the first casefold dependency commit omitted the desktop's
  nested lockfile and therefore was not independently gate-clean. Commit `557f3fd` repaired
  the published HEAD. The already-public history was not force-rewritten; future dependency
  changes must update both lockfiles in their owning commit.
- Final cross-review approval of the correction range is pending. No automated security
  scan was used.

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
