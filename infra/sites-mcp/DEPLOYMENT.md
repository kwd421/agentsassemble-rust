# Private Sites MCP deployment candidate

Status: gateway verified locally; private website published; hosted MCP blocked.
Project: `appgprj_6abe7c7c2ba88191a5e241ef59dbef8a`.

## Runtime boundary and approval

The existing MCP implementation runs inside the native Rust room server.
Sites requires a Worker-compatible artifact. Uploading the native application
does not produce a supported Sites deployment.

The deployment is a private Worker gateway to one configured app-owned `/mcp`
endpoint. The user clarified on 2026-10-02 that this deployment adapter is within
the requested work: it addresses the known execution-environment difference and
does not conceal a failure with a fallback. Proceed under that clarification.

This candidate hosts the gateway on Sites. It does not move the Rust MCP runtime,
room authority, history, provider execution or client-session registry to Sites.
A standalone Sites MCP runtime would require a separate implementation slice for
durable connection custody, retry resolution and concurrent wait observation.

## Gateway contract

- Keep the existing registered Site owner-private; use private publication only.
- Route `/mcp` POST, GET and DELETE to one exact operator-configured HTTPS endpoint.
  Never derive an upstream from tool input, invitation URLs or request headers.
- Refuse redirects and configuration containing embedded credentials, fragments
  or query parameters. Do not enable an arbitrary network proxy.
- Let Sites enforce caller authentication. Do not send Sites OAuth credentials,
  authenticated-user headers, cookies or bypass tokens to the Rust upstream.
- Derive upstream Host from the configured URL and apply the native ingress
  contract deliberately. Never bypass its current-generation ingress validation.
- Preserve MCP content, status and streaming behavior, bounded request sizes,
  private/no-store responses, and disconnect cancellation. Avoid payload logging.
- Let the existing Rust hub own the 17 tools, connection preparation, admission,
  request identities, unresolved commands, wait cursors and leave receipts.
- Report upstream unavailability as an error. Do not substitute a local demo,
  fabricated tool results or an alternate server.
- Configure `MCP_UPSTREAM_URL` through Sites runtime environment management. No
  runtime credentials or invitation capabilities belong in source or artifacts.
- On a host restart or changed public ingress, require a verified upstream update;
  a fixed Sites URL alone does not guarantee continuity of room connections.

## User flow and limitations

After deployment and verification, a client connects to the stable Sites MCP
endpoint once, and the user supplies an ordinary room-connector invitation.
The user does not install a separate MCP process. The room-owning AgentsAssemble
app and its reachable ingress remain necessary while participating.

The initial owner-private deployment is not a publicly installable integration
for other users. Any audience expansion is a separate explicit user action.
Automatic client installation and automatic upstream updates are not established
by this design and must not be promised.

## Acceptance before reporting success

1. Check actual native ingress compatibility and verify a reachable upstream.
2. Build a supported artifact from the exact pushed Sites source commit.
3. Verify authenticated MCP initialization and discovery of all 17 native tools.
4. Reject unauthenticated access, arbitrary destinations and forwarded credentials.
5. Exercise prepare, confirm admission, read, say, leave and receipt release against
   an isolated actual Rust room; verify the resulting room events independently.
6. Exercise transport failure and cancellation without changing native retry or
   wait semantics. Run affected checks and mandatory repository gates.
7. Save and privately deploy the candidate, then verify the production endpoint.
   A saved version, deployment URL or tools/list response alone is not completion.

## Verification and remaining dependency (2026-10-02)

- Node contract tests: 4 pass. Actual Rust upstream in an isolated room: initialize,
  17 tools, prepare, join, read, say, 32-second silent wait cancellation, leave and
  receipt release pass. Native SQLite independently records exactly one message.
- Actual workerd runtime: initialize and discovery of 17 tools pass. Architecture,
  formatting, diff and artifact repository gates pass without exceptions.
- Private Sites version 1 deployed successfully at
  `https://agentsassemble-mcp.kwd421.chatgpt.site`; authenticated root GET returned
  the gateway HTML. Unauthenticated MCP access was rejected. Authenticated MCP
  initialization and GET `/mcp` returned HTTP 404, so production acceptance failed.
- Deployment reported `has_mcp: false`. Connection provisioning explicitly refused
  with "The published Site does not declare an MCP server. Enable MCP and republish it."
- Neither the exposed Sites tools, the official Sites documentation nor the
  public `openai/sites` starter/plugin source supplied the required MCP declaration
  contract. Trial `mcp` and `mcp_server` hosting-manifest fields were both rejected
  before saving with "Extra inputs are not permitted"; those fields are removed.
  No alternate route, authentication bypass or invented provisioning ID is used.
- The test upstream configuration is removed and its isolated host stopped. The
  published website is explicitly marked as not ready. No user room is attached.

Completion requires the supported Sites MCP declaration/activation contract,
private redeployment and the production acceptance checks above. The current
private website is not a usable hosted MCP server or an installed client plugin.
