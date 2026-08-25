import { describe, expect, it } from "vitest";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";

import {
  bindRoomDirectoryAuthority,
  parseStrictRoomCreateResponse,
  parseStrictRoomDirectory,
  retainRoomDirectoryAuthority,
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
  "participant.mute",
  "participant.role.update",
  "room.random.choose",
  "room.random.roll",
  "room.settings.update",
] as const;
const surface = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "b222eb2b710d635bd0b226942619f1e81b5f420b9cf4afa7b3d0a4e0e5150c0a",
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
            digest: "b450fcb4474817b305910d9067a22b35e72d79b81da2ddd0b3a512e8972df7e7",
            websocket_streams: [],
          },
        }),
        surface,
        "https://surface-recomputed-digest.example"
      )
    ).rejects.toThrow(/bootstrap/);
  });
});
