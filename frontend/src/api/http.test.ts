import { afterEach, describe, expect, it, vi } from "vitest";

import {
  fetchJsonServerOperator,
  fetchJsonWithIdentity,
  postEmptyServerOperator,
  postJsonServerOperator,
  postJsonWithIdentity,
  responseError,
} from "./http";
import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { getWsTicket } from "./room";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

const HOST_SURFACE = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "1".repeat(64),
  commands: ["host_product_surface", "runtime_operator_ticket"],
};

it("preserves a structured server error code separately from its message", async () => {
  const error = await responseError(
    new Response(
      JSON.stringify({ code: "invite_revoked", error: "Invite was revoked." }),
      { status: 403, headers: { "Content-Type": "application/json" } }
    )
  );

  expect(error).toMatchObject({
    status: 403,
    code: "invite_revoked",
    message: "Invite was revoked.",
  });
});

it("preserves the nested error contract used by ingress controls", async () => {
  const error = await responseError(
    new Response(
      JSON.stringify({
        error: {
          code: "ingress_cleanup_failed",
          message: "Managed public ingress cleanup failed.",
        },
      }),
      { status: 503, headers: { "Content-Type": "application/json" } }
    )
  );

  expect(error).toMatchObject({
    status: 503,
    code: "ingress_cleanup_failed",
    message: "Managed public ingress cleanup failed.",
  });
});

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
        ticket: "a".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49152",
      })
      .mockResolvedValueOnce({
        ticket: "b".repeat(64),
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
    expect(firstHeaders.get("Authorization")).toBe(`Bearer ${"a".repeat(64)}`);
    expect(secondHeaders.get("Authorization")).toBe(`Bearer ${"b".repeat(64)}`);
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
        ticket: "c".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49153",
      })
      .mockResolvedValueOnce({
        ticket: "d".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49153",
      })
      .mockResolvedValueOnce({
        ticket: "e".repeat(64),
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
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ mode: "managed" }), {
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
    await postEmptyServerOperator("/api/public-invite/tunnel/start");

    expect(invoke).toHaveBeenNthCalledWith(1, "host_product_surface");
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_operator_ticket");
    expect(invoke).toHaveBeenNthCalledWith(4, "runtime_operator_ticket");
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49153/api/rooms?include_archived=true",
      expect.objectContaining({ headers: expect.any(Headers) })
    );
    const listHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const createHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    const emptyPost = fetchMock.mock.calls[2]?.[1] as RequestInit;
    const emptyPostHeaders = emptyPost.headers as Headers;
    expect(listHeaders.get("Authorization")).toBe(`Bearer ${"c".repeat(64)}`);
    expect(createHeaders.get("Authorization")).toBe(`Bearer ${"d".repeat(64)}`);
    expect(createHeaders.get("Content-Type")).toBe("application/json");
    expect(emptyPost.method).toBe("POST");
    expect(emptyPost.body).toBeUndefined();
    expect(emptyPostHeaders.get("Authorization")).toBe(`Bearer ${"e".repeat(64)}`);
    expect(emptyPostHeaders.get("Content-Type")).toBeNull();
  });

  it("rechecks operation ownership after a delayed operator ticket", async () => {
    let resolveTicket!: (value: unknown) => void;
    const ticket = new Promise((resolve) => {
      resolveTicket = resolve;
    });
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockReturnValueOnce(ticket);
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    let current = true;
    const request = postEmptyServerOperator(
      "/api/public-invite/tunnel/start",
      () => {
        if (!current) throw new Error("retired operator request");
      }
    );
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    const rejection = expect(request).rejects.toThrow("retired operator request");

    current = false;
    resolveTicket({
      ticket: "f".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49154",
    });

    await rejection;
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rechecks directory GET and JSON POST ownership at post-ticket dispatch", async () => {
    let resolvePostTicket!: (value: unknown) => void;
    let resolveGetTicket!: (value: unknown) => void;
    const postTicket = new Promise((resolve) => {
      resolvePostTicket = resolve;
    });
    const getTicket = new Promise((resolve) => {
      resolveGetTicket = resolve;
    });
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockReturnValueOnce(postTicket)
      .mockReturnValueOnce(getTicket);
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    let postCurrent = true;
    const post = postJsonServerOperator(
      "/api/rooms",
      { room_id: "project-room" },
      () => {
        if (!postCurrent) throw new Error("retired room create");
      }
    );
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
    postCurrent = false;
    resolvePostTicket({
      ticket: "1".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49155",
    });
    await expect(post).rejects.toThrow("retired room create");

    let getCurrent = true;
    const get = fetchJsonServerOperator("/api/rooms", () => {
      if (!getCurrent) throw new Error("retired room directory");
    });
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(3));
    getCurrent = false;
    resolveGetTicket({
      ticket: "2".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49155",
    });
    await expect(get).rejects.toThrow("retired room directory");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("browser session WebSocket ticket routing", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("exchanges the raw session once and derives the socket origin without fallback", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ticket: "c".repeat(64),
          ttl_seconds: 30,
          server_proof_key: "d".repeat(64),
        }),
        { status: 200, headers: { "Content-Type": "application/json" } }
      )
    );
    vi.stubGlobal("fetch", fetchMock);

    const grant = await getWsTicket({
      kind: "session",
      sessionToken: "aas1.browser-session",
    });

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock).toHaveBeenCalledWith("/api/session-tickets/socket", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer aas1.browser-session" },
    });
    expect(grant).toEqual({
      ticket: "c".repeat(64),
      ttl_seconds: 30,
      websocket_base_url: window.location.origin.replace(/^http/, "ws"),
      server_proof_key: "d".repeat(64),
      displayResourceBase: window.location.origin,
    });
  });
});
