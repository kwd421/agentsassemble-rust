import { afterEach, describe, expect, it, vi } from "vitest";

import {
  fetchJsonServerOperator,
  fetchJsonWithIdentity,
  postJsonServerOperator,
  postJsonWithIdentity,
} from "./http";
import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

const HOST_SURFACE = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "1".repeat(64),
  commands: ["host_product_surface", "runtime_operator_ticket"],
};

describe("desktop profile HTTP routing", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("uses a fresh server-wide operator ticket for every local profile operation", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce({
        ticket: "ticket-read",
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49152",
      })
      .mockResolvedValueOnce({
        ticket: "ticket-write",
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49152",
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

    await requestDesktopHostProductSurface();
    await fetchJsonWithIdentity("/api/user-profile", { roomId: "general" });
    await postJsonWithIdentity(
      "/api/user-profile",
      { display_name: "Canonical" },
      { roomId: "general" }
    );

    expect(invoke).toHaveBeenNthCalledWith(1, "host_product_surface");
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_operator_ticket");
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49152/api/user-profile",
      expect.objectContaining({
        cache: "no-store",
        headers: expect.objectContaining({}),
      })
    );
    const firstHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const secondHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(firstHeaders.get("Authorization")).toBe("Bearer ticket-read");
    expect(secondHeaders.get("Authorization")).toBe("Bearer ticket-write");
    expect(secondHeaders.get("Content-Type")).toBe("application/json");
  });

  it("exchanges an admitted session for a fresh one-use profile ticket", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ticket: "a".repeat(64), ttl_seconds: 30 }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ profile: { display_name: "Guest" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ticket: "b".repeat(64), ttl_seconds: 30 }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ profile: { display_name: "Changed" } }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    await fetchJsonWithIdentity("/api/user-profile", {
      roomId: "general",
      sessionToken: "guest-session",
    });
    await postJsonWithIdentity(
      "/api/user-profile",
      { display_name: "Changed" },
      { roomId: "general", sessionToken: "guest-session" }
    );

    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenNthCalledWith(1, "/api/session-tickets/profile", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer guest-session" },
    });
    expect(fetchMock).toHaveBeenNthCalledWith(2, "/api/user-profile", {
      cache: "no-store",
      headers: { Authorization: `Bearer ${"a".repeat(64)}` },
    });
    expect(fetchMock).toHaveBeenNthCalledWith(3, "/api/session-tickets/profile", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer guest-session" },
    });
    expect(fetchMock).toHaveBeenNthCalledWith(4, "/api/user-profile", {
      cache: "no-store",
      method: "POST",
      headers: {
        Authorization: `Bearer ${"b".repeat(64)}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({ display_name: "Changed" }),
    });
  });

  it("uses purpose-separated one-use operator tickets for room control HTTP", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce({
        ticket: "operator-list",
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49153",
      })
      .mockResolvedValueOnce({
        ticket: "operator-create",
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49153",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ server_id: "server", rooms: [] }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ status: "ready", server_id: "server", room: {} }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    await fetchJsonServerOperator("/api/rooms?include_archived=true");
    await postJsonServerOperator("/api/rooms", {
      room_id: "project-room",
      label: "Project Room",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "host_product_surface");
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_operator_ticket");
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49153/api/rooms?include_archived=true",
      expect.objectContaining({ headers: expect.any(Headers) })
    );
    const listHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const createHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(listHeaders.get("Authorization")).toBe("Bearer operator-list");
    expect(createHeaders.get("Authorization")).toBe("Bearer operator-create");
    expect(createHeaders.get("Content-Type")).toBe("application/json");
  });
});
