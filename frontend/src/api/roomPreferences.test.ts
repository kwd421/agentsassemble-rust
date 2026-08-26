import { afterEach, describe, expect, it, vi } from "vitest";

import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { fetchRoomSettings, saveRoomSettings } from "./room";

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
  notifications: "all" | "mentions" | "mute" = "mentions"
) {
  return {
    room_id: roomId,
    settings: {
      room_id: roomId,
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
      conversation_mode: "ordered",
      tool_mode: "chat",
      ordered_exclude_previous_speaker: true,
      channels: [],
      activity_plugin: "",
    },
  };
}

describe("room preference HTTP authority", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("uses a fresh purpose ticket and preference-only body for each desktop operation", async () => {
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
        new Response(JSON.stringify(response()), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify(response("general", "mute")), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    const loaded = await fetchRoomSettings("general", { deviceToken: "not-authority" });
    const saved = await saveRoomSettings({
      roomId: "general",
      appearance: { notifications: "mute" },
      channelSettings: {
        lobby: { notifications: "all" },
      },
      identity: { deviceToken: "not-authority" },
    });

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
      appearance: { notifications: "mute" },
      channel_settings: {
        lobby: { notifications: "all", last_read_at: "" },
      },
    });
  });

  it("exchanges a remote session for a fresh exact-purpose ticket per operation", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ticket: "d".repeat(64), ttl_seconds: 30 }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify(response()), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ ticket: "e".repeat(64), ttl_seconds: 30 }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify(response("general", "mute")), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      );
    vi.stubGlobal("fetch", fetchMock);

    const identity = {
      sessionToken: "aas1.guest-session",
      deviceToken: "guest-device",
    };
    await fetchRoomSettings("general", identity);
    await saveRoomSettings({
      roomId: "general",
      appearance: { notifications: "mute" },
      identity,
    });

    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenNthCalledWith(1, "/api/session-tickets/preferences-read", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer aas1.guest-session" },
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/room-settings?room_id=general",
      expect.objectContaining({ cache: "no-store", headers: expect.any(Headers) })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(3, "/api/session-tickets/preferences-write", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer aas1.guest-session" },
    });
    expect(fetchMock).toHaveBeenNthCalledWith(
      4,
      "/api/room-settings",
      expect.objectContaining({
        cache: "no-store",
        method: "POST",
        headers: expect.any(Headers),
      })
    );
    const readHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    const writeInit = fetchMock.mock.calls[3]?.[1] as RequestInit;
    const writeHeaders = writeInit.headers as Headers;
    expect(readHeaders.get("Authorization")).toBe(`Bearer ${"d".repeat(64)}`);
    expect(readHeaders.get("X-Device-Token")).toBeNull();
    expect(writeHeaders.get("Authorization")).toBe(`Bearer ${"e".repeat(64)}`);
    expect(writeHeaders.get("X-Device-Token")).toBeNull();
    expect(writeHeaders.get("Content-Type")).toBe("application/json");
    expect(JSON.parse(String(writeInit.body))).toEqual({
      room_id: "general",
      appearance: { notifications: "mute" },
    });
  });

  it("does not call the preference target when session exchange is denied", async () => {
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
        appearance: { notifications: "mute" },
        identity: { sessionToken: "aas1.read-only-session" },
      })
    ).rejects.toThrow("Read-only room sessions cannot change preferences.");
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock).toHaveBeenCalledWith("/api/session-tickets/preferences-write", {
      cache: "no-store",
      method: "POST",
      headers: { Authorization: "Bearer aas1.read-only-session" },
    });
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
});
