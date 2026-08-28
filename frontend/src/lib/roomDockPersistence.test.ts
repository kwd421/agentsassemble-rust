import { Hash } from "lucide-react";
import { beforeEach, describe, expect, it } from "vitest";
import { persistableRoom, type RoomDockItem } from "./roomDockModel";
import {
  loadRoomDockItems,
  persistRoomDockItems,
} from "./roomDockPersistence";


describe("room dock persistence", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("keeps the complete active room rail used by the current server", () => {
    const rooms: RoomDockItem[] = Array.from({ length: 32 }, (_, index) => ({
      id: `room-${index}`,
      label: `Room ${index}`,
      meetingId: `meeting-${index}`,
      roomOrigin: index === 1 ? "remote_server" : "local",
      serverOrigin: index === 1 ? "https://rooms.example.test" : undefined,
      topic: `Topic ${index}`,
      shortLabel: "R",
      appearance: {
        bannerPreset: index === 0 ? "custom" : "default",
        bannerImage:
          index === 0 ? `/api/attachments/ra_${"a".repeat(32)}?view=1` : undefined,
        iconImage:
          index === 0 ? `/api/attachments/ra_${"b".repeat(32)}?view=1` : undefined,
        iconLabel: "R",
        inviteScope: "room",
      },
      icon: Hash,
      createdAt: "",
      tone: "resident",
    }));

    persistRoomDockItems(rooms.map(persistableRoom));

    const restored = loadRoomDockItems();
    expect(restored.map((room) => room.meetingId)).toEqual(
      rooms.map((room) => room.meetingId)
    );
    expect(restored[0].appearance).toMatchObject({
      bannerImage: `/api/attachments/ra_${"a".repeat(32)}?view=1`,
      iconImage: `/api/attachments/ra_${"b".repeat(32)}?view=1`,
    });
    expect(restored[1]).toMatchObject({
      roomOrigin: "remote_server",
      serverOrigin: "https://rooms.example.test",
    });
  });

  it("drops generic attachment references from persisted room appearance", () => {
    window.localStorage.setItem(
      "agentsassemble.discord.rooms.v1",
      JSON.stringify([
        {
          id: "general",
          meetingId: "general",
          label: "General",
          roomOrigin: "local",
          topic: "",
          shortLabel: "G",
          createdAt: "",
          tone: "resident",
          appearance: {
            bannerPreset: "custom",
            bannerImage: "/api/attachments/legacy001?view=1",
            iconImage: `/api/attachments/ra_${"c".repeat(32)}?download=1`,
            inviteScope: "room",
          },
        },
      ])
    );

    expect(loadRoomDockItems()[0].appearance).toMatchObject({
      bannerImage: undefined,
      iconImage: undefined,
    });
  });

  it("round-trips canonical room IDs without ECMAScript trimming or UTF-16 truncation", () => {
    const meetingIds = ["\ufeffroom", "\u{10000}".repeat(128)];
    const rooms: RoomDockItem[] = meetingIds.map((meetingId, index) => ({
      id: `server-room-${index}`,
      label: `Room ${index}`,
      meetingId,
      roomUid: `30000000-0000-4000-8000-00000000000${index}`,
      serverId: "10000000-0000-4000-8000-000000000001",
      roomOrigin: "local",
      topic: "Exact room identity",
      shortLabel: "R",
      icon: Hash,
      createdAt: "2026-08-28T00:00:00Z",
      tone: "resident",
    }));

    persistRoomDockItems(rooms.map(persistableRoom));

    expect(loadRoomDockItems().map((room) => room.meetingId)).toEqual(meetingIds);
  });
});
