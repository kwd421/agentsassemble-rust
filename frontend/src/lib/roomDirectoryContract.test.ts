import { describe, expect, it } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

import {
  bindRoomDirectoryAuthority,
  currentRoomDirectoryAuthority,
  currentServerProductSurface,
  parseStrictRoomCreateResponse,
  parseRoomSessionSurface,
  parseStrictRoomDirectory,
  retainRoomDirectoryAuthority,
  verifyAndBindRoomSessionSurface,
} from "./roomDirectoryContract";

const serverId = "10000000-0000-4000-8000-000000000001";
const lineageId = "20000000-0000-4000-8000-000000000002";
const roomUid = "30000000-0000-4000-8000-000000000003";
const surface = TEST_SERVER_PRODUCT_SURFACE;

function room() {
  return {
    room_id: "general",
    room_uid: roomUid,
    label: "General",
    last_active_at: "2026-08-25T00:00:00Z",
    archived: false,
    status: "active",
    origin: "agent_session",
  };
}

function directoryRoom(
  roomId: string,
  uid: string = roomUid
) {
  return {
    ...room(),
    cleanup_pending: false,
    deletion_pending: false,
    room_id: roomId,
    room_uid: uid,
    room_settings: {
      room_id: roomId,
      settings_revision: `settings-${roomId}`,
      label: roomId,
      topic: roomId,
      appearance: {
        banner_preset: "default",
        banner_image_url: "",
        icon_image_url: "",
        icon_label: "R",
        invite_scope: "room",
      },
      conversation_mode: "ordered",
      tool_mode: "chat",
      ordered_exclude_previous_speaker: true,
      channels: [],
    },
  };
}

function directory(rooms: ReturnType<typeof directoryRoom>[]) {
  return {
    server_id: serverId,
    authority_lineage_id: lineageId,
    server_product_surface: surface,
    profile_revision: 1,
    rooms,
  };
}

describe("room directory contracts", () => {
  it("rejects a loose or lineage-free follow-up directory", () => {
    expect(() => parseStrictRoomDirectory({})).toThrow();
    expect(() =>
      parseStrictRoomDirectory({ server_id: serverId, rooms: [] })
    ).toThrow();
  });

  it("accepts DELETE and rejects methods outside the registered HTTP schema", () => {
    expect(() =>
      parseStrictRoomDirectory({
        ...directory([]),
        server_product_surface: {
          ...surface,
          http_routes: [
            { method: "DELETE", path: "/api/provider-credentials/deepseek" },
          ],
        },
      })
    ).not.toThrow();
    expect(() =>
      parseStrictRoomDirectory({
        ...directory([]),
        server_product_surface: {
          ...surface,
          http_routes: [
            { method: "PUT", path: "/api/provider-credentials/deepseek" },
          ],
        },
      })
    ).toThrow(/HTTP route/);
  });

  it("accepts expanded authority-bound room creation responses", () => {
    const payload = {
      status: "ready",
      server_id: serverId,
      authority_lineage_id: lineageId,
      room: room(),
      deduplicated: false,
    };
    expect(parseStrictRoomCreateResponse(payload)).toEqual(payload);
    expect(parseStrictRoomCreateResponse({ ...payload, ignored: true })).toEqual(payload);
  });

  it("accepts unknown fields at every directory and session response level", () => {
    const entry = directoryRoom("general");
    const expandedSurface = {
      ...surface, extra: null,
      http_routes: [{ method: "GET", path: "/api/rooms", extra: false }],
    };
    const payload = {
      ...directory([]), extra: [], server_product_surface: expandedSurface,
      rooms: [{ ...entry, extra: 1, room_settings: {
        ...entry.room_settings, extra: {},
        appearance: { ...entry.room_settings.appearance, extra: 42 },
        channels: [{ id: "chat", name: "Chat", type: "chat", position: 0,
          created_at: "2026-10-05T00:00:00Z", extra: null }],
      } }],
    };
    expect(parseStrictRoomDirectory(payload).rooms).toEqual(payload.rooms);
    expect(parseRoomSessionSurface(payload).server_product_surface).toEqual(expandedSurface);
    expect(parseStrictRoomCreateResponse({ status: "ready", server_id: serverId,
      authority_lineage_id: lineageId, room: { ...room(), extra: [] },
      deduplicated: false, extra: {} }).room.room_id).toBe("general");
  });

  it("retains required fields and types throughout nested directory responses", () => {
    const payload = directory([directoryRoom("general")]);
    const entry = payload.rooms[0];
    const channel = { id: "chat", name: "Chat", type: "chat", position: 0,
      created_at: "2026-10-05T00:00:00Z" };
    Object.assign(entry.room_settings, { channels: [channel] });
    const route = { method: "GET" as const, path: "/api/rooms" };
    payload.server_product_surface = { ...surface, http_routes: [route] };
    for (const target of [payload, entry, entry.room_settings,
      entry.room_settings.appearance, channel, payload.server_product_surface, route]) {
      const fields = target as Record<string, unknown>;
      for (const key of Object.keys(fields)) {
        const original = fields[key];
        delete fields[key];
        expect(() => parseStrictRoomDirectory(payload), `missing ${key}`).toThrow();
        fields[key] = null;
        expect(() => parseStrictRoomDirectory(payload), `null ${key}`).toThrow();
        fields[key] = original;
      }
    }
  });

  it("does not recompute a server-only surface digest", async () => {
    const payload = directory([]);
    payload.server_product_surface = { ...surface, digest: "d".repeat(64) };
    await expect(bindRoomDirectoryAuthority(parseStrictRoomDirectory(payload), null,
      "https://server-only-surface.example")).resolves.toBe(true);
  });

  it("rejects duplicate canonical room IDs or room UIDs", () => {
    const secondUid = "40000000-0000-4000-8000-000000000004";
    expect(() =>
      parseStrictRoomDirectory(
        directory([
          directoryRoom("general"),
          directoryRoom("general", secondUid),
        ])
      )
    ).toThrow(/중복/);
    expect(() =>
      parseStrictRoomDirectory(
        directory([
          directoryRoom("general"),
          directoryRoom("other", roomUid),
        ])
      )
    ).toThrow(/중복/);
  });

  it("uses Rust whitespace semantics for canonical room identifiers", () => {
    const rustCanonical = {
      status: "ready",
      server_id: serverId,
      authority_lineage_id: lineageId,
      room: { ...room(), room_id: "\ufeffgeneral" },
      deduplicated: false,
    };

    expect(parseStrictRoomCreateResponse(rustCanonical)).toEqual(rustCanonical);
    expect(() =>
      parseStrictRoomCreateResponse({
        ...rustCanonical,
        room: { ...room(), room_id: "\u0085general" },
      })
    ).toThrow(/정규 형식/);
  });

  it("never rebinds a lifetime pin even when native bootstrap matches the replacement", () => {
    const pinned = { server_id: serverId, authority_lineage_id: lineageId };
    const replacement = {
      server_id: "40000000-0000-4000-8000-000000000004",
      authority_lineage_id: "50000000-0000-4000-8000-000000000005",
    };
    expect(() =>
      retainRoomDirectoryAuthority(replacement, pinned, replacement)
    ).toThrow(/bootstrap 서버 및 계보/);
    expect(retainRoomDirectoryAuthority(pinned, pinned, pinned)).toEqual(pinned);
  });

  it("rejects a self-asserted surface downgrade before binding native authority", async () => {
    const authority = {
      server_id: serverId,
      authority_lineage_id: lineageId,
      server_product_surface: surface,
      profile_revision: 1,
      rooms: [],
    };
    await bindRoomDirectoryAuthority(
      parseStrictRoomDirectory(authority),
      surface,
      "https://surface-valid.example"
    );
    await expect(
      bindRoomDirectoryAuthority(
        parseStrictRoomDirectory({
          ...authority,
          server_product_surface: {
            ...surface,
            websocket_streams: [],
          },
        }),
        surface,
        "https://surface-forged-digest.example"
      )
    ).rejects.toThrow(/digest/);
    await expect(
      bindRoomDirectoryAuthority(
        parseStrictRoomDirectory({
          ...authority,
          server_product_surface: {
            ...surface,
            digest: "d1a4716e55db92d7177cdbf074173074421d60c4fba967fc1247d27636acfb9e",
            websocket_streams: [],
          },
        }),
        surface,
        "https://surface-recomputed-digest.example"
      )
    ).rejects.toThrow(/bootstrap/);
  });

  it("does not pin authority for a stale room-session verification", async () => {
    const origin = "https://stale-session.example";
    await expect(
      verifyAndBindRoomSessionSurface(
        {
          server_id: serverId,
          authority_lineage_id: lineageId,
          server_product_surface: {
            ...surface,
            http_routes: [...surface.http_routes],
            websocket_streams: [...surface.websocket_streams],
            websocket_actions: [...surface.websocket_actions],
          },
        },
        () => false,
        origin
      )
    ).resolves.toBe(false);
    expect(currentRoomDirectoryAuthority(origin)).toBeNull();
  });

  it("does not bind global authority or surface after guarded integrity work becomes stale", async () => {
    const origin = "https://stale-directory.example";
    let current = true;
    const binding = bindRoomDirectoryAuthority(
      parseStrictRoomDirectory(directory([])),
      surface,
      origin,
      () => current
    );
    current = false;

    await expect(binding).resolves.toBe(false);
    expect(currentRoomDirectoryAuthority(origin)).toBeNull();
    expect(currentServerProductSurface(origin)).toBeNull();
  });
});
