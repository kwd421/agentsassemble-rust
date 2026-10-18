import { afterEach, describe, expect, it, vi } from "vitest";

import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { fetchRoomSettings, normalizeRoomGlobalSettings, saveRoomSettings, type ConversationMode } from "./room";

const HOST_SURFACE = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "3".repeat(64),
  commands: [
    "host_product_surface",
    "runtime_preferences_read_ticket",
    "runtime_preferences_write_ticket",
  ],
};

function response(
  roomId = "general",
  notifications: "all" | "mentions" | "mute" = "mentions",
  conversationMode: ConversationMode = "ordered"
) {
  return {
    room_id: roomId,
    settings: {
      room_id: roomId,
      lobby_name: "팀 대화",
      settings_revision: `room-settings-v1-${"a".repeat(64)}`,
      label: "General",
      topic: "",
      short_label: "G",
      appearance: {
        banner_preset: "default",
        banner_image_url: "",
        icon_image_url: "",
        icon_label: "G",
        notifications,
        invite_scope: "room",
      },
      channel_settings: {
        lobby: { notifications: "default", last_read_at: "cursor-1" },
      },
      conversation_mode: conversationMode,
      tool_mode: "chat",
      ordered_exclude_previous_speaker: true,
      channels: [],
    },
  };
}

describe("room preference HTTP authority", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it.each(["ordered", "ambient", "free_access"] as const)("reads %s settings and keeps preference writes separate", async (mode) => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce({
        ticket: "a".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49155",
      })
      .mockResolvedValueOnce({
        ticket: "b".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49155",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify(response("general", "mentions", mode)), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify(response("general", "mute", mode)), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    const loaded = await fetchRoomSettings("general", { deviceToken: "not-authority" });
    const saved = await saveRoomSettings({
      roomId: "general",
      roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
      appearance: { notifications: "mute" },
      channelSettings: {
        lobby: { notifications: "all" },
      },
      identity: { deviceToken: "not-authority" },
    });

    expect(normalizeRoomGlobalSettings(response("general", "mentions", mode).settings, "general")?.conversationMode).toBe(mode);
    expect(loaded.channelSettings.lobby.lastReadAt).toBe("cursor-1");
    expect(saved.appearance.notifications).toBe("mute");
    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_preferences_read_ticket", {
      roomId: "general",
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_preferences_write_ticket", {
      roomId: "general",
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49155/api/room-settings?room_id=general",
      expect.objectContaining({
        cache: "no-store",
        method: "GET",
        headers: expect.any(Headers),
      })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://127.0.0.1:49155/api/room-settings",
      expect.objectContaining({
        cache: "no-store",
        method: "POST",
        headers: expect.any(Headers),
      })
    );
    const readHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const writeInit = fetchMock.mock.calls[1]?.[1] as RequestInit;
    const writeHeaders = writeInit.headers as Headers;
    expect(readHeaders.get("Authorization")).toBe(`Bearer ${"a".repeat(64)}`);
    expect(writeHeaders.get("Authorization")).toBe(`Bearer ${"b".repeat(64)}`);
    expect(JSON.parse(String(writeInit.body))).toEqual({
      room_id: "general",
      room_uid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
      appearance: { notifications: "mute" },
      channel_settings: {
        lobby: { notifications: "all", last_read_at: "" },
      },
    });
  });

  it.each([
    { sessionToken: "aas1.guest-session", deviceToken: undefined },
    { sessionToken: "aops1.paired", deviceToken: "paired-device" },
  ])("desktop fails a forgotten session closed, never the local host ($sessionToken)", async ({ sessionToken, deviceToken }) => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    const identity = {
      sessionToken, deviceToken,
    };
    // A presented session token without a live transport is never a local
    // session on desktop: after a channel loss or confirmed-loss unbind, a
    // friend-room read or save must not run against the local host with
    // owner authority on the same room id, nor hit the blocked
    // `tauri://localhost` origin. Only calls with NO session token take the
    // desktop local-ticket path.
    await expect(fetchRoomSettings("general", identity)).rejects.toThrow(
      "연결이 끝났어요"
    );
    await expect(
      saveRoomSettings({
        roomId: "general",
        roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
        appearance: { notifications: "mute" },
        identity,
      })
    ).rejects.toThrow("연결이 끝났어요");
    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("surfaces a read-only denial from the preference target", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          error: "Read-only room sessions cannot change preferences.",
          code: "session_read_only",
        }),
        { status: 403, headers: { "Content-Type": "application/json" } }
      )
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      saveRoomSettings({
        roomId: "general",
        roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
        appearance: { notifications: "mute" },
        identity: { sessionToken: "aas1.read-only-session" },
      })
    ).rejects.toThrow("Read-only room sessions cannot change preferences.");
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock).toHaveBeenCalledWith("/api/room-settings", {
      cache: "no-store", redirect: "error", referrerPolicy: "no-referrer",
      method: "POST",
      headers: expect.any(Headers),
      body: JSON.stringify({
        room_id: "general",
        room_uid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
        appearance: { notifications: "mute" },
      }),
    });
    const headers = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    expect(headers.get("Authorization")).toBe("Bearer aas1.read-only-session");
  });

  it("rejects a mismatched response room instead of projecting defaults", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(HOST_SURFACE)
      .mockResolvedValueOnce({
        ticket: "c".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49156",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify(response("other")), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
    );

    await requestDesktopHostProductSurface();
    await expect(fetchRoomSettings("general")).rejects.toThrow("방 권위");
  });

  it("routes a bound friend-room session through the remote transport, never the local origin", async () => {
    const { bindRemoteSession } = await import("../lib/remote/remoteWorkspace");
    const origin = "https://friend.test";
    const transportFetch = vi.fn(async () =>
      new Response(JSON.stringify(response("general")), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    const transport = {
      hello: { origin },
      active: true,
      fetch: transportFetch,
      close: vi.fn(),
      onClose: vi.fn(),
    };
    bindRemoteSession("friend-session-live", transport as never, "friend-server");
    const globalFetch = vi.fn();
    vi.stubGlobal("fetch", globalFetch);

    const loaded = await fetchRoomSettings("general", { sessionToken: "friend-session-live" });
    expect(loaded.roomId).toBe("general");
    expect(transportFetch).toHaveBeenCalledOnce();
    const [fetchPath, fetchInit] = transportFetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(fetchPath).toBe("/api/room-settings?room_id=general");
    const headers = fetchInit.headers as Headers;
    expect(headers.get("Authorization")).toBe("Bearer friend-session-live");
    expect(globalFetch).not.toHaveBeenCalled();
  });

  it("fails a dead friend-room session closed instead of fetching the local origin", async () => {
    const { bindRemoteSession, sessionHostOrigin } = await import("../lib/remote/remoteWorkspace");
    const origin = "https://friend-dead.test";
    const closeListeners = new Set<() => void>();
    const transport = {
      hello: { origin },
      active: true,
      fetch: vi.fn(),
      close: vi.fn(),
      onClose: vi.fn((listener: () => void) => {
        closeListeners.add(listener);
        return () => {
          closeListeners.delete(listener);
        };
      }),
    };
    bindRemoteSession("friend-session-dead", transport as never, "friend-server");
    expect(sessionHostOrigin("friend-session-dead")).toBe(origin);
    // The channel dies while the app still holds the token: the binding
    // remembers the host origin but no longer has a live transport.
    for (const listener of [...closeListeners]) listener();
    const globalFetch = vi.fn();
    vi.stubGlobal("fetch", globalFetch);
    // Desktop: no relative fetch (CSP would block tauri://localhost) and no
    // local-ticket fallback for a dead remote session.
    Object.assign(window, { __TAURI_INTERNALS__: { invoke: vi.fn() } });
    await expect(fetchRoomSettings("general", { sessionToken: "friend-session-dead" })).rejects.toThrow(
      "연결이 끝났어요"
    );
    expect(globalFetch).not.toHaveBeenCalled();
  });
});
