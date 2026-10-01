# AgentsAssemble private Sites MCP gateway

Registered on 2026-10-02 at the user's request.

- Project: `appgprj_6abe7c7c2ba88191a5e241ef59dbef8a`
- Slug: `agentsassemble-mcp`
- Access: owner only; no groups or external viewers.
- Build: `npm run build`; output: `dist/server/index.js` and Worker configuration.
- Verification: `npm test`. No package install or third-party runtime dependency.
- Runtime configuration: `MCP_UPSTREAM_URL`, one exact native HTTPS `/mcp` address.

Sites runs `src/worker.mjs`; the existing native MCP implementation at
`crates/agentsassemble-server/src/connector_mcp_web.rs` retains its 17 tools,
room authority, admission, private connection custody and shutdown semantics.
The Worker streams protocol requests and responses to that one upstream, blocks
redirects and strips Sites/user authentication headers at the upstream boundary.
It introduces no room database, provider process or fabricated tool results.

The native room server and its public ingress must remain running. A changed
upstream requires a verified Sites environment update and redeployment; automatic
upstream registration is not implemented. Initial client MCP installation remains
an external-account step. The owner-private Site is not a public integration.

Deployment contract and acceptance: `DEPLOYMENT.md`. Production verification is
recorded there after publishing; registration alone is not a successful deployment.

Keep the returned project ID in `.openai/hosting.json` and reuse this registration.
Do not create another Site to retry deployment. No credentials belong in this file.
