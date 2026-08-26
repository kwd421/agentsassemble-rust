import { describe, expect, it } from "vitest";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

import {
  bindRoomDirectoryAuthority,
  currentRoomDirectoryAuthority,
  parseStrictRoomCreateResponse,
  parseStrictRoomDirectory,
  retainRoomDirectoryAuthority,
  verifyAndBindRoomSessionSurface,
} from "./roomDirectoryContract";

const serverId = "10000000-0000-4000-8000-000000000001";
const lineageId = "20000000-0000-4000-8000-000000000002";
const roomUid = "30000000-0000-4000-8000-000000000003";
const actions = [
  "agent.configure",
  "agent.create",
  "agent.resume",
  "agent.start",
  "agent.stop",
  "message.send",
  "participant.leave",
  "participant.mute",
  "participant.role.update",
  "room.random.choose",
  "room.random.roll",
  "room.settings.update",
] as const;
const surface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "b49aff9ac3fe96d19687ddbc3af6c6f7c2e524f10a9177c34fa6ac37ebb3083b",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [...actions],
} as const;

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
            digest: "30f1f3d357fc4ff68f868259516826d6b49bdbfcb93f76dae8b0b206a0003467",
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
});
