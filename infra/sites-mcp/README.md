# AgentsAssemble MCP Sites registration

Registered on 2026-10-02 at the user's request.

- Project: `appgprj_6abe7c7c2ba88191a5e241ef59dbef8a`
- Slug: `agentsassemble-mcp`
- Access: owner only; no groups or external viewers.
- Saved versions: 0. No source upload or deployment has occurred.
- MCP connection lookup requires a published MCP-ready Site.

The existing MCP endpoint is implemented in the native Rust room server at
`crates/agentsassemble-server/src/connector_mcp_web.rs`. Sites publication expects
a supported Worker entrypoint or static artifact, so registration does not make
this native server runnable on Sites. Deployment needs an explicit compatible
design that preserves room authority, admission, private connection custody and
shutdown semantics. No proxy, rewritten server or authentication bypass was added.

Keep the returned project ID in `.openai/hosting.json` and reuse this registration.
Do not create another Site to retry deployment. No credentials belong in this file.
