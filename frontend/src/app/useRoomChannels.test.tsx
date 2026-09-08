import { act, renderHook } from "@testing-library/react";
import { Hash } from "lucide-react";
import { describe, expect, it, vi } from "vitest";
import type { RoomChannel, RoomGlobalSettings } from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomChannels } from "./useRoomChannels";

const room: RoomDockItem = {
  id: "room-a",
  label: "Room A",
  meetingId: "meeting-a",
  topic: "A",
  shortLabel: "A",
  icon: Hash,
  createdAt: "2026-07-12T00:00:00Z",
  tone: "fresh",
};

const firstChannel: RoomChannel = {
  id: "c000000000001",
  name: "notes",
  type: "text",
  position: 0,
  createdAt: "2026-07-12T00:00:00Z",
};

function settings(channels: RoomChannel[] = [firstChannel]): RoomGlobalSettings {
  return {
    roomId: room.meetingId,
    revision: "settings-meeting-a",
    label: room.label,
    topic: room.topic,
    shortLabel: room.shortLabel,
    appearance: {
      bannerPreset: "default",
      inviteScope: "room",
    },
    conversationMode: "ordered",
    toolMode: "chat",
    orderedExcludePreviousSpeaker: true,
    channels,
  };
}

describe("useRoomChannels", () => {
  it("projects the active room's channels from canonical settings", () => {
    const saveCanonicalSettings = vi.fn();
    const hook = renderHook(() =>
      useRoomChannels({
        activeRoom: room,
        canonicalSettings: settings(),
        saveCanonicalSettings,
      })
    );

    expect(hook.result.current.activeChannels).toEqual([firstChannel]);
    expect(hook.result.current.isActiveCustomChannel(firstChannel.id)).toBe(true);
    expect(hook.result.current.activeChannelFor(firstChannel.id)).toEqual(firstChannel);
  });

  it("creates a channel only through the canonical settings writer", async () => {
    const saveCanonicalSettings = vi.fn(async (updates) => {
      const channels = updates.channels || [];
      return settings(channels);
    });
    const hook = renderHook(() =>
      useRoomChannels({
        activeRoom: room,
        canonicalSettings: settings(),
        saveCanonicalSettings,
      })
    );

    let created: RoomChannel | null = null;
    await act(async () => {
      created = await hook.result.current.create({ name: "discussion", type: "text" });
    });

    expect(created).toMatchObject({
      name: "discussion",
      type: "text",
      position: 1,
    });
    expect(saveCanonicalSettings).toHaveBeenCalledWith({
      channels: [
        firstChannel,
        expect.objectContaining({
          id: expect.stringMatching(/^c[0-9a-f]{12}$/),
          name: "discussion",
          type: "text",
          position: 1,
        }),
      ],
    });
  });

  it("does not invent local channel state when the canonical write is rejected", async () => {
    const saveCanonicalSettings = vi.fn().mockRejectedValue(
      new Error("settings conflict")
    );
    const hook = renderHook(() =>
      useRoomChannels({
        activeRoom: room,
        canonicalSettings: settings(),
        saveCanonicalSettings,
      })
    );

    await expect(
      act(async () => {
        await hook.result.current.create({ name: "discussion", type: "text" });
      })
    ).rejects.toThrow("settings conflict");
    expect(hook.result.current.activeChannels).toEqual([firstChannel]);
  });
});
