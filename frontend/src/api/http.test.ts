import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchJsonWithIdentity, postJsonWithIdentity } from "./http";

describe("desktop profile HTTP routing", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("uses a fresh one-use runtime ticket for every local profile operation", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        ticket: "ticket-read",
        ttl_seconds: 30,
        websocket_base_url: "ws://127.0.0.1:49152",
        server_proof_key: "proof-read",
      })
      .mockResolvedValueOnce({
        ticket: "ticket-write",
        ttl_seconds: 30,
        websocket_base_url: "ws://127.0.0.1:49152",
        server_proof_key: "proof-write",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ profile: { display_name: "SeiNel" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ profile: { display_name: "Canonical" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    await fetchJsonWithIdentity("/api/user-profile", { roomId: "general" });
    await postJsonWithIdentity(
      "/api/user-profile",
      { display_name: "Canonical" },
      { roomId: "general" }
    );

    expect(invoke).toHaveBeenNthCalledWith(1, "runtime_ticket", { roomId: "general" });
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_ticket", { roomId: "general" });
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49152/api/user-profile",
      expect.objectContaining({
        headers: expect.objectContaining({}),
      })
    );
    const firstHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const secondHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(firstHeaders.get("Authorization")).toBe("Bearer ticket-read");
    expect(secondHeaders.get("Authorization")).toBe("Bearer ticket-write");
    expect(secondHeaders.get("Content-Type")).toBe("application/json");
  });

  it("keeps an admitted participant on its own session authority", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ profile: { display_name: "Guest" } }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    await fetchJsonWithIdentity("/api/user-profile", {
      roomId: "general",
      sessionToken: "guest-session",
    });

    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledWith("/api/user-profile", {
      headers: { Authorization: "Bearer guest-session" },
    });
  });
});
