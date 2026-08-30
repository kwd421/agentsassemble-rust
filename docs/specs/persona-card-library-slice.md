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

The SQLite library repository owns one row per canonical persona ID: normalized private card
JSON and, for cards only, one optional canonical PNG. A single upsert replaces both values, so
validation or statement failure retains the prior exact row and a thumbnail cannot outlive the
card that selected it. Reimport without a thumbnail removes the prior thumbnail in that same
statement. No raw upload, source path, timestamp, pending state, quota ledger, cleanup task, or
generic repository layer was added. This is the smallest durable owner for the reachable picker
contract and keeps persona custody separate from profile, pre-join, room-appearance, and message
attachments.

The concrete avoidable cost was loading a thumbnail BLOB of up to 10 MiB while listing summaries.
The list query projects only the ID, normalized card JSON, and a SQLite presence bit; card reads and
thumbnail reads are separate indexed operations. Sorting computes the original default-casefolded
name key once per item. This preserves fail-closed corrupt-row handling, card-before-module ordering,
private-body exclusion, and exact replacement semantics. The accepted trade-off is decoding each
bounded card JSON on a library list instead of introducing a second summary copy or cache that could
drift from the private owner. A warm real-SQLite replacement/list/read/reopen regression completed its
test body in 0.04 seconds on the development host; that is harness evidence, not a production latency
claim.

Agent Session creation and stopped-session configuration resolve the selected canonical ID to its
current card and safe summary inside the same SQLite transaction that writes the session. Empty IDs
clear both fields; missing, corrupt, mismatched, or thumbnail-inconsistent assets return the single
`persona_not_found` product error and preserve the prior session. Durable deserialization also rejects
an ID/summary mismatch, so neither restart nor a direct corrupt row can expose split selection state.
The lookup projects card JSON and one thumbnail-presence bit and never reads the thumbnail BLOB. The
runtime profile key is versioned over the selected ID, so provider catalog revalidation and the final
persistence write bind the same selection without retaining a second catalog-group field.

Inlining the safe summary enlarged measured async futures to 17,264--17,936 bytes. Boxing the selected
summary and its ID at the public Agent Session owner brought three affected paths below the 16-KiB
architecture gate; the remaining 16,576-byte participant-mute interrupt future is boxed only at that
rare dispatch boundary. The common unselected session therefore retains pointer-sized option state,
while a selected session pays one summary allocation. A proposed removal of the create-result session
clone was rejected after the existing create-and-start regression changed the committed result from
`stopped` to `starting`: that clone deliberately freezes the pre-effect public result and is part of
the replay contract, not avoidable copying. Provider tests completed 133 cases, persistence completed
211, the real WebSocket stopped-configuration boundary completed its focused case, and full workspace
Clippy plus structure/source gates passed. These are verification results, not production performance
claims; packaged-picker and authorized real-provider ordinary-turn verification remain pending.

Ordinary ordered and ambient assignments now load the selected private card only while constructing
the existing durable provider-turn envelope. Lore scans the same at-most-50-message canonical prefix
used for the bounded room observation; room labels, Agent handles, attachment metadata, and later
events cannot activate it. The rendered provider input, room view, delivery kind, attachments, and
room-Agent IDs are committed together in the existing `provider_turn_executions.assignment_json`.
Recovery therefore does not reread the persona library: replacing the same library ID after assignment
cannot alter an in-flight turn. No persona cache, second turn model, provider-specific prompt branch,
or new durable state was added.

The selected path adds one indexed card-JSON read and one bounded provider-neutral render per assigned
turn; an unselected session returns before querying the library. Input work is bounded by the existing
50-message/20,000-character room prefix and 8,000-character persona result. The accepted disk cost is
the rendered private persona text inside the already required exact assignment input, still under the
existing 20,000-character provider-input ceiling; the complete card and thumbnail are not copied into
turn custody. Private card bodies remain absent from public room views, events, logs, and snapshots.
Missing or corrupt selected rows fail the same message/assignment transaction rather than clearing the
selection or emitting a partial turn. One real-SQLite regression exercised both ordered and ambient
schedulers, literal-lore activation, same-ID library replacement, and byte-identical recovery in 0.06
seconds on the development host. The complete workspace test suite, Clippy, formatting, and structure/
growth gates passed. These bounds and harness results establish custody and regression behavior, not a
production latency improvement; packaged-client and authorized real-provider verification remain
pending.

The local-operator HTTP owner consumes one exact server-operator ticket before reading any request
body. List, import, and thumbnail responses are private/no-store; thumbnails are fixed canonical PNG
responses rather than arbitrary imported content. Import accepts only the original five filename
suffixes, bounds the JSON/base64 envelope from the shared 10-MiB byte limit, and sends only decoded
bytes to the format owners. The shared server transport owner now computes that base64 envelope once
for both persona and existing attachment JSON uploads.

The concrete scheduler threat is that one accepted CHARX may validate up to 80 MiB of expanded entry
metadata and data. Performing that work on a Tokio executor thread would occupy it and can delay
unrelated room and HTTP tasks, while many cancelled imports could otherwise release admission before
their blocking work stopped. Parsing, decompression, normalization, and base64 decoding now run on
Tokio's maintained blocking pool. The process admits two complete imports at a time, and a detached import task retains
its permit through the atomic SQLite replacement even if the requesting connection disappears. This
bounds concurrent expanded custody to the two accepted imports without adding a queue framework,
retry path, cache, timer, or background cleanup. The accepted trade-off is backpressure on a third
local import. The real TCP suite verifies purpose separation, authorization-before-body, one-use
tickets, CORS, private caching, canonical PNG delivery, missing-thumbnail failure, and an untouched
library after rejected requests; its two tests completed in 0.07 seconds during full verification.

The copied picker now obtains list, import, and thumbnail access through fresh native
local-operator exchanges. It treats the summary thumbnail URL only as a presence signal and
constructs the fixed persona-ID route itself, so private server output cannot select a different
authority target. One shared frontend safe-raster owner checks exact private/no-store PNG responses
for both persona thumbnails and room appearance rather than maintaining two signature and size
policies.

Thumbnail Blob URLs have an explicit React lifecycle owner. It requests only the selected item and
the at most eight visible library rows, aborts reads that leave that set, and revokes each Blob URL
only after React no longer renders it. Reimporting the same canonical ID explicitly invalidates the
old request and rendered Blob instead of retaining a stale thumbnail across the server's atomic
replacement. An advertised thumbnail read failure remains visible; it is not silently converted to
a successful icon-only read. StrictMode, failure, exact-ID replacement, and unmount regressions prove
request and Blob custody. The focused frontend boundary completed 16 tests in 0.69 seconds and the
complete frontend suite completed 599 tests in 10.78 seconds on the development host. These are
harness results, not production latency claims. Packaged-picker verification remains pending and is
not inferred from the source test.

Before adding a real API or local-model persona path, a repository-wide provider-identity search
found the same current provider ID, runtime kind, transport, discovery entry point, and launch entry
point independently selected by the loading catalog, creation selection, and runtime factory. Commit
`edbf83d` first moved the existing provider-neutral driver and turn contract out of the runtime
lifecycle owner. Commit `50e9b96` then made one fixed three-entry registration the owner used by
catalog discovery, creation transport selection, and runtime launch. Provider-specific discovery,
protocol, custody, and prompt behavior remain in their concrete owners; Antigravity's initial native
session promotion and transport-specific room-tool instruction, and Codex's executable-bundle
integrity, were deliberately not generalized into registration metadata.

The change adds no provider, product state, cache, retry, fallback, background worker, or generic
plugin framework. Catalog discovery still owns one cancellable task and exactly the current three
concurrent bounded probes. The registration path allocates one boxed future per probe and one bounded
provider vector, matching the allocations already present at those boundaries; no production CPU,
memory, disk, or latency improvement is claimed. The accepted benefit is removal of three divergent
identity branches before the already-required API/local provider work. The final tree passed the
complete workspace tests, warning-denied workspace Clippy, formatting, whitespace, and architecture/
source-growth gates. The independently staged driver-contract commit additionally passed all 133
provider tests, warning-denied provider Clippy, and the architecture gates. No real provider,
Computer Use resource, Deep Scan, or other automated security scan ran for this structural change.

The first remote-API prerequisite owns only the currently required `DeepSeek` credential rather
than introducing a generic credential/provider framework. The verified original status contract
prefers the `AgentsAssemble`/`deepseek` platform-keyring item, reports only
`keyring | environment | missing`, permits `DEEPSEEK_API_KEY` only when no secure item exists, and
never falls back to the environment after an installed secure store fails. Set validates a trimmed
8--8,192-character secret, and delete removes only the secure item so an environment credential may
become visible again. The Rust boundary exposes no secret read or serialization API yet; copied UI,
runtime injection, and remote-host authority remain explicitly incomplete.

The implementation uses maintained `keyring` 4.1.6 with its v1 platform stores instead of owning
Keychain, Credential Manager, Secret Service, encryption, or persistence code. The macOS status
path uses `security-framework` 3.7.0 item search without requesting data, attributes, or references.
Inspection of the
dependency showed that `Entry::new` collapses every store-initialization error into
`NoDefaultStore`; treating that result as an absent backend would silently turn a locked or failed
installed store into an environment fallback. The owner therefore checks `Entry::store_status`
first and accepts only its documented `Invalid("platform", ...)` result as platform absence; every
other initialization or access error is the stable `secure_store_unavailable` failure. On macOS,
the exact search is restricted to the same User/login keychain selected by the v1 store.

The metadata-only query still had a concrete interaction threat: Security.framework's default item
search policy permits authentication UI, so a protected matching item could display a prompt during
status. Skipping protected items was rejected because that would misclassify a present secure item as
absent and activate the environment fallback. Process-global interaction disabling was also rejected
because concurrent keychain users could observe the temporary global state. The query now requests
fail-on-authentication-UI through a safe `security-framework` method pinned at exact public fork
commit `85407d113b978b27728e162c8485c11e233c3e3e`, based on release 3.7.0. Only
`errSecItemNotFound` means absent; interaction-required, authentication, keychain, and all other
errors fail closed. Apple's underlying value is deprecated in favor of an `LAContext` with
`interactionNotAllowed`, but the maintained crate has no safe compatible context bridge; the pinned
release patch is the smaller auditable boundary until one exists.

The direct pin leaves the registry copy used by `keyring` and TLS alongside the patched direct copy.
That duplication increased the measured debug server binary from 118,258,392 to 118,263,160 bytes
(4,768 bytes) and added about 6.5 MiB of debug intermediate artifacts; it added no product state,
runtime task, network request, or disk custody. This build cost was accepted over raw local FFI or a
process-global policy. The fork's complete library suite passed 113 tests with one ignored, and its
dictionary-boundary regression proves the default, fail, and skip policies remain mutually
exclusive. The application regression verifies a real missing-item lookup without secret material
and proves that only not-found maps to absence. All 139 provider tests, warning-denied provider
Clippy, formatting, whitespace, architecture, and source-growth gates passed. During that full run,
an unrelated arbitrary PID-file readiness poll was exposed as timing-dependent; commit `e74815f`
replaced it with a child-published Unix-datagram readiness event in the process-tree test owner. The
three affected process lifecycle tests passed without changing production runtime behavior.

The full workspace run then exposed a separate pre-existing server-boundary harness deadline. On
the development macOS host, launching the large debug helper took about 4.3 seconds to complete the
guardian lifetime handoff and about 2.3 additional seconds to publish provider readiness. Both
product-owned helper stages completed inside their separate 5-second fail-closed limits, but the
shared test WebSocket imposed one unrelated 5-second deadline across the complete create/start
command and rejected the later valid public result; the same failure reproduced at baseline
`ec19b4d`. Commit `f902201` removes that wall-clock assertion from ordinary receives and waits for
the authenticated public ACK/events instead. Tests that explicitly assert no-frame or deadline
behavior retain their scoped timeout. The exact create/start regression completed in 7.5 seconds,
and all nine serialized Agent Session TCP/WebSocket boundary scenarios completed in 107.93 seconds;
no production timeout, retry, provider behavior, or fallback changed.

Platform keyring operations are potentially blocking OS calls. One store-owned semaphore and
Tokio's maintained blocking pool serialize them; the permit moves into the blocking closure so
caller cancellation cannot admit an overlapping operation while the first OS call still runs. The
accepted cost is one blocking task and one semaphore acquisition per status/set/delete operation,
plus target-specific lockfile dependencies from the maintained cross-platform crate. No production
CPU, memory, disk, or latency improvement is claimed. Fake-backend tests prove precedence, deletion,
unsupported-store behavior, validation bounds, and fail-closed installed-store errors without
reading or writing a real credential. All 136 provider tests, warning-denied provider Clippy,
formatting, whitespace, architecture, and source-growth gates passed.

The private DeepSeek credential HTTP owner now exposes the reachable metadata-only status,
secure-store set, and secure-store delete operations through one route. Each request consumes a
fresh exact local server-operator ticket before reading its body; responses are private/no-store and
serialize only `configured` plus `source`. The route owns only transport admission and stable HTTP
error projection. Secret length, trimming, secure-store precedence, deletion, and fail-closed
backend behavior remain in `ProviderCredentialStore`, with no second provider registry, credential
trait, cache, retry, or fallback.

The 128-KiB POST ceiling is a transport bound rather than a duplicate secret policy: it admits the
provider owner's 8,192-Unicode-scalar maximum even when every scalar uses a JSON surrogate-pair
escape. A real TCP regression completed in 0.04 seconds and proved purpose separation,
authorization-before-body, one-use consumption, secret-free errors, private CORS/cache headers, and
registered DELETE preflight. Focused provider tests covered the secure-store owner through its fake
backend. No real Keychain item was written or deleted because that would mutate user-owned
credential data; copied-UI and authorized real-provider verification remain pending.

## Manual-review findings

- Daybreaker Blue High found one Medium in the first pushed registration/credential range:
  `keyring`'s macOS `get_credential()` materialized password bytes despite its name, so the status
  path could request secret ACL access and the metadata-only documentation was false. Commit
  `0906e4d` replaces it with a `security-framework` generic-password existence query that requests
  no data, attributes, or references, keeps every non-not-found error fail-closed, and verifies a
  real missing-item metadata lookup without creating or deleting a Keychain item.
- A later manual source review found that this metadata query still inherited Security.framework's
  UI-allowed default. Commit `2492ccc` makes noninteraction explicit without treating a protected
  item as absent or changing the environment fallback contract. External re-review of this
  correction is pending.
- The critical web session and Daybreaker Blue High manually reviewed pushed range
  `087ba1a..45c8302`. Both found no Critical, High, Medium, or Low findings and
  returned `APPROVE`. Neither reviewer ran Deep Scan or another automated security
  scan.
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
  lines and tokenizes decorator arguments with the exact set. It also bounds prompt
  normalization while scanning instead of first allocating every word from an input that can
  reach the 5 MiB card limit. Reachable import, partial-match, probability-decorator, and
  visible prompt behavior are covered without new durable state.
- The critical web review found one Medium in CHARX embedded-Risu lore projection: normalizing
  the module as a standalone `.risum` before projection leaked standalone-only aliases and
  priority/case state across the narrower CHARX boundary. It also changed which large lore
  entry won the ordinary 3,600-character budget. Daybreaker found one Medium in the correction:
  CHARX and the ordinary matcher separately owned the same keyword grammar. Commit `f5fc9f9`
  now decodes the bounded raw module once, projects only the original CHARX field set, preserves
  standalone normalization unchanged, and shares one domain keyword iterator. Opposing CHARX
  and standalone regressions pin both contracts through the ordinary renderer.
- Daybreaker also found that the first casefold dependency commit omitted the desktop's
  nested lockfile and therefore was not independently gate-clean. Commit `557f3fd` repaired
  the published HEAD. The already-public history was not force-rewritten; future dependency
  changes must update both lockfiles in their owning commit.
- The critical web session and Daybreaker Blue High both approved exact
  `296f298..f5fc9f9` and cumulative `4fa167c..f5fc9f9` with C0/H0/M0/L0. No automated
  security scan was used.
- The critical web session and Daybreaker Blue High both approved the pushed atomic
  library, local-operator HTTP, and copied-picker range `f5fc9f9..087ba1a` with
  C0/H0/M0/L0. No automated security scan was used.

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
