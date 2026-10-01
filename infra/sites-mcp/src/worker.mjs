// Sites owns caller authentication; Rust owns MCP tools, custody and room authority.
const requestHeaders = [
  "accept", "content-type", "mcp-protocol-version", "mcp-session-id", "last-event-id",
];
const responseHeaders = [
  "content-type", "mcp-protocol-version", "mcp-session-id", "allow", "retry-after",
];

function failure(status, code) {
  return Response.json({ error: { code } }, {
    status, headers: { "cache-control": "private, no-store" },
  });
}

function upstreamEndpoint(value) {
  const url = new URL(value);
  if (url.protocol !== "https:" || url.username || url.password || url.search || url.hash
    || url.pathname !== "/mcp" || (url.port && url.port !== "443")
    || !url.hostname.includes(".") || url.hostname.endsWith(".")
    || url.hostname === "localhost" || url.hostname.endsWith(".localhost")
    || url.hostname.endsWith(".local") || /^[\d.]+$/.test(url.hostname)
    || url.hostname.includes(":")) {
    throw new Error("invalid_upstream");
  }
  return url;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/" && request.method === "GET" && !url.search) {
      return new Response("<!doctype html><html lang=\"ko\"><meta charset=\"utf-8\">"
        + "<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">"
        + "<title>AgentsAssemble MCP</title><main><h1>AgentsAssemble MCP</h1>"
        + "<p>MCP 연결은 아직 준비 중이며 현재 사용할 수 없습니다.</p>"
        + "<p>활성화와 실제 연결 검증이 완료되면 연결 안내를 제공합니다.</p></main></html>", {
        headers: { "content-type": "text/html; charset=utf-8", "cache-control": "private, no-store" },
      });
    }
    if (url.pathname !== "/mcp" || url.search) return failure(404, "route_not_found");
    if (!["POST", "GET", "DELETE"].includes(request.method)) {
      return new Response(null, { status: 405, headers: {
        allow: "POST, GET, DELETE", "cache-control": "private, no-store",
      } });
    }
    const origin = request.headers.get("origin");
    if (origin !== null && origin !== url.origin) return failure(403, "origin_not_allowed");
    let upstream;
    try {
      upstream = upstreamEndpoint(env.MCP_UPSTREAM_URL);
    } catch {
      return failure(503, "mcp_upstream_not_configured");
    }
    const headers = new Headers();
    for (const name of requestHeaders) {
      const value = request.headers.get(name);
      if (value !== null) headers.set(name, value);
    }
    // Fetch derives Host from the configured URL. Never forward caller cookies,
    // authorization, identity, Origin, forwarding or native ingress credentials.
    try {
      const response = await fetch(upstream, {
        method: request.method, headers, redirect: "manual", signal: request.signal,
        body: request.method === "GET" ? undefined : request.body,
        // Node local verification requires duplex for a streaming request body.
        duplex: "half",
      });
      if (response.status >= 300 && response.status < 400) {
        await response.body?.cancel();
        return failure(502, "mcp_upstream_redirect_rejected");
      }
      const outgoing = new Headers({ "cache-control": "private, no-store" });
      for (const name of responseHeaders) {
        const value = response.headers.get(name);
        if (value !== null) outgoing.set(name, value);
      }
      return new Response(response.body, { status: response.status, headers: outgoing });
    } catch {
      return failure(502, "mcp_upstream_unavailable");
    }
  },
};
