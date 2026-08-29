import { describe, expect, it } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

import {
  bindRoomDirectoryAuthority,
  currentRoomDirectoryAuthority,
  currentServerProductSurface,
  parseStrictRoomCreateResponse,
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
      activity_plugin: "",
    },
  };
}

function directory(rooms: ReturnType<typeof directoryRoom>[]) {
  return {
    server_id: serverId,
    authority_lineage_id: lineageId,
    server_product_surface: surface,
    rooms,
  };
}

describe("room directory contracts", () => {
  it("rejects a loose or lineage-free follow-up directory", () => {
    expect(() => parseStrictRoomDirectory({})).toThrow(/계약/);
    expect(() =>
      parseStrictRoomDirectory({ server_id: serverId, rooms: [] })
    ).toThrow(/계약/);
  });

  it("accepts only an exact authority-bound room creation response", () => {
    const payload = {
      status: "ready",
      server_id: serverId,
      authority_lineage_id: lineageId,
      room: room(),
      deduplicated: false,
    };
    expect(parseStrictRoomCreateResponse(payload)).toEqual(payload);
    expect(() =>
      parseStrictRoomCreateResponse({ ...payload, ignored: true })
    ).toThrow(/계약/);
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
            digest: "907399f4f53bb9de6c5f30f2ad9f85f8f55146d6557592f7186bfe8d8b665b5a",
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
