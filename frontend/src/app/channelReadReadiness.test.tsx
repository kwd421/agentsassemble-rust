import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Hash } from "lucide-react";
import type { RoomSettings } from "../api";
import AppView from "./AppView";
import { useAppController, type AppController } from "./useAppController";

const boundary = vi.hoisted(() => ({
  fetchRoomSettings: vi.fn(), saveRoomSettings: vi.fn(),
  directory: {} as Record<string, unknown>, canonical: {} as Record<string, unknown>,
}));
vi.mock("../api", async () => ({ ...await vi.importActual("../api"),
  fetchRoomSettings: boundary.fetchRoomSettings, saveRoomSettings: boundary.saveRoomSettings,
}));
vi.mock("../lib/desktopBridge", async () => ({ ...await vi.importActual("../lib/desktopBridge"), isDesktopWebview: () => true }));
vi.mock("./useRoomDirectory", () => ({ useRoomDirectory: () => boundary.directory }));
vi.mock("../useCanonicalRoom", () => ({ useCanonicalRoom: () => boundary.canonical }));
// These panels do not own either shortcut or the preference writer.
vi.mock("../views/LobbyView", () => ({ default: () => null }));
vi.mock("../views/components/UserPanel", () => ({ default: () => null }));
vi.mock("../views/components/RoomConnectionPanel", () => ({ default: () => null }));
vi.mock("./AppOverlays", () => ({ default: () => null }));
vi.mock("../views/components/SideChatDock", () => ({ default: () => null }));

const room = { id: "room-a", meetingId: "meeting-a", label: "Room A", topic: "A", shortLabel: "A",
  roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c", icon: Hash, createdAt: "2026-09-10T00:00:00Z", tone: "fresh" };
const preferences: RoomSettings = {
  roomId: room.meetingId, label: room.label, topic: room.topic, shortLabel: room.shortLabel,
  appearance: { bannerPreset: "forest", notifications: "mentions", inviteScope: "room" },
  channelSettings: { lobby: { notifications: "mute", lastReadAt: "seq:3" },
    c123456789012: { notifications: "mentions", lastReadAt: "seq:2" } },
  conversationMode: "ordered", toolMode: "chat", orderedExcludePreviousSpeaker: true,
};
let controller: AppController;
function Harness() {
  controller = useAppController("device-test", "client-test");
  return <AppView controller={controller} />;
}

beforeEach(() => {
  localStorage.clear();
  vi.clearAllMocks();
  boundary.directory = { rooms: [room], managementRooms: [room], syncIssue: null,
    updateRoomByMeetingId: vi.fn(), managerAuthorityCurrent: false };
  boundary.canonical = {
    room: { room_id: room.meetingId, room_uid: room.roomUid },
    roomSettings: { ...preferences, revision: "settings-1", channels: [] },
    connectionState: "connected", syncIssue: null, socket: { ready: () => true },
    history: { initialized: true, lastSeq: 9 }, timelineEvents: [{ seq: 9 }],
    participants: [], agentSessions: [], capabilities: {}, agentSessionProgress: null,
    events: [], providerRequests: [],
  };
  boundary.saveRoomSettings.mockImplementation(async (updates) => ({ ...preferences, ...updates }));
});
afterEach(cleanup);

describe("existing channel read shortcuts", () => {
  it.each([false, true])("does not overwrite unloaded preferences (initial failure=%s)", async (fail) => {
    let resolve!: (settings: RoomSettings) => void;
    let reject!: (error: Error) => void;
    boundary.fetchRoomSettings.mockReturnValue(new Promise<RoomSettings>((done, error) => { resolve = done; reject = error; }));
    render(<Harness />);
    const shortcuts = () => [
      screen.getByRole("button", { name: "현재 채널 읽음으로 표시" }),
      screen.getByRole("button", { name: "알림" }),
    ] as HTMLButtonElement[];
    const assertBlocked = () => {
      for (const button of shortcuts()) { expect(button.disabled).toBe(true); fireEvent.click(button); }
      act(() => controller.markChannelRead("lobby"));
      expect(boundary.saveRoomSettings).not.toHaveBeenCalled();
    };
    assertBlocked();
    if (fail) {
      await act(async () => reject(new Error("preferences offline")));
      assertBlocked();
      boundary.fetchRoomSettings.mockResolvedValue(preferences);
      await act(async () => { controller.roomSettings.refresh(controller.activeRoom); });
    } else {
      await act(async () => resolve(preferences));
    }
    await waitFor(() => expect(shortcuts()[0].disabled).toBe(false));
    fireEvent.click(shortcuts()[fail ? 1 : 0]);
    await waitFor(() => expect(boundary.saveRoomSettings).toHaveBeenCalledTimes(1));
    expect(boundary.saveRoomSettings.mock.calls[0][0].channelSettings).toEqual({
      lobby: { notifications: "mute", lastReadAt: "seq:9" },
      c123456789012: { notifications: "mentions", lastReadAt: "seq:2" },
    });
  });
});
