import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RoomEvent } from "../api";
import { memberRoomLiveKey, useMemberRoomLive } from "./useMemberRoomLive";
import { resetMemberRoomAutoReselectBudgetForTests } from "../lib/central/memberRoomAutoReselectBudget";
import type { MemberHostSnapshot } from "./useMemberHosts";
import type { RoomSocketHandle, RoomSocketHandlers } from "../roomSocketTypes";

const sessionFor = (meetingId: string) => ({
  session_token: `session-${meetingId}`,
  agent_id: "member-1",
  display_name: "Member",
  meeting_id: meetingId,
  server_id: "friend",
  server_product_surface: { websocket_streams: ["room_events"] },
});

const message = (overrides = {}) => ({
  v: 1,
  id: "event",
  seq: 11,
  created_at: "2026-10-17T00:00:00Z",
  room_id: "general",
  type: "message_final",
  actor: { participant_id: "other" },
  participant_id: "other",
  message_kind: "message",
  content: "hello",
  ...overrides,
}) as unknown as RoomEvent;

const snapshotFor = (serverId: string, rooms: string[], phase: MemberHostSnapshot["phase"] = "connected"): MemberHostSnapshot => ({
  serverId,
  phase,
  origin: "https://friend.test",
  rooms: rooms.map((room_id) => ({ room_id, name: room_id })),
  detail: null,
});

describe("useMemberRoomLive", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Automatic selects share a module-level per-account daily budget; each
    // unit test starts from a fresh budget so they stay independent.
    resetMemberRoomAutoReselectBudgetForTests();
  });

  function setup() {
    const selectBackgroundRoom = vi.fn(async (roomId: string) => sessionFor(roomId));
    const admission = {
      transport: { active: true },
      deviceToken: "device",
      selectBackgroundRoom,
    };
    const opened: { auth: unknown; handlers: RoomSocketHandlers; close: () => void }[] = [];
    const openSocket = vi.fn((auth: unknown, _streams: unknown, handlers: RoomSocketHandlers) => {
      const record = { auth, handlers, close: vi.fn() };
      opened.push(record);
      return { close: record.close } as unknown as RoomSocketHandle;
    });
    const admissionFor = vi.fn(() => admission);
    const hook = renderHook(
      ({ snapshots, activeServerId, activeMeetingId }) =>
        useMemberRoomLive({
          snapshots,
          admissionFor: admissionFor as never,
          activeServerId,
          activeMeetingId,
          openSocket: openSocket as never,
        }),
      {
        initialProps: {
          snapshots: [snapshotFor("friend", ["general", "second"])],
          activeServerId: "",
          activeMeetingId: "",
        },
      }
    );
    return { ...hook, selectBackgroundRoom, admission, opened, admissionFor };
  }

  it("selects every joined room after attach and raises unread without opening it", async () => {
    const { result, selectBackgroundRoom, opened } = setup();
    await waitFor(() => expect(selectBackgroundRoom).toHaveBeenCalledTimes(2));
    expect(selectBackgroundRoom).toHaveBeenCalledWith("general");
    expect(selectBackgroundRoom).toHaveBeenCalledWith("second");
    await waitFor(() => expect(opened).toHaveLength(2));

    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toBeUndefined();

    await act(async () => {
      general.handlers.onRoomEvents?.([message({ seq: 11 })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toEqual({ unread: true, mention: false });

    await act(async () => {
      general.handlers.onRoomEvents?.([message({ seq: 12, content: "hey <@member-1>" })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toEqual({ unread: true, mention: true });
  });

  it("never counts the viewer's own messages as unread", async () => {
    const { result, opened } = setup();
    await waitFor(() => expect(opened).toHaveLength(2));
    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
      general.handlers.onRoomEvents?.([message({ seq: 11, participant_id: "member-1" })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toBeUndefined();
  });

  it("suspends the active room and keeps its cached session for instant switch", async () => {
    const { result, opened, rerender } = setup();
    await waitFor(() => expect(opened).toHaveLength(2));
    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
      general.handlers.onRoomEvents?.([message({ seq: 11 })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]?.unread).toBe(true);

    rerender({ snapshots: [snapshotFor("friend", ["general", "second"])], activeServerId: "friend", activeMeetingId: "general" });
    // Opening marks read; the canonical socket owns the room now.
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toBeUndefined();
    expect(general.close).toHaveBeenCalled();
    // ...but the cached session stays for the instant switch (no re-admission).
    expect(result.current.liveSessionFor("friend", "general")).toMatchObject({ meeting_id: "general" });
  });

  it("clears badges while the host is offline and resubscribes on reconnect", async () => {
    const { result, opened, selectBackgroundRoom, rerender, admissionFor } = setup();
    await waitFor(() => expect(opened).toHaveLength(2));
    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
      general.handlers.onRoomEvents?.([message({ seq: 11 })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]?.unread).toBe(true);

    rerender({ snapshots: [snapshotFor("friend", ["general", "second"], "disconnected")], activeServerId: "", activeMeetingId: "" });
    // Unknown is shown as unknown: no stale counts while offline.
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toBeUndefined();

    const selectCalls = selectBackgroundRoom.mock.calls.length;
    // A fresh host channel means a fresh admission: cached sessions were
    // bound to the dead transport, so the hook re-selects on it. A genuine
    // external re-attach bumps the host generation, which releases one more
    // bounded automatic attempt.
    const { noteMemberHostExternalReselectAttach } =
      await import("../lib/central/memberRoomAutoReselectBudget");
    noteMemberHostExternalReselectAttach("", "friend");
    const freshAdmission = {
      transport: { active: true },
      deviceToken: "device",
      selectBackgroundRoom,
    };
    admissionFor.mockReturnValue(freshAdmission);
    rerender({
      snapshots: [snapshotFor("friend", ["general", "second"])],
      activeServerId: "",
      activeMeetingId: "",
    });
    await waitFor(() => expect(selectBackgroundRoom.mock.calls.length).toBeGreaterThan(selectCalls));
    // The instant-switch cache follows the live admission only.
    expect(result.current.liveSessionFor("friend", "general")).toMatchObject({ meeting_id: "general" });
  });

  it("marks a background room read without opening it", async () => {    const { result, opened } = setup();
    await waitFor(() => expect(opened).toHaveLength(2));
    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
      general.handlers.onRoomEvents?.([message({ seq: 11 })]);
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]?.unread).toBe(true);
    act(() => {
      result.current.markRoomRead("friend", "general");
    });
    expect(result.current.badges[memberRoomLiveKey("friend", "general")]).toBeUndefined();
    expect(result.current.hasLive("friend", "general")).toBe(true);
  });

  it("re-selects a dead session once, then parks instead of spinning", async () => {
    const { opened, selectBackgroundRoom } = setup();
    await waitFor(() => expect(opened).toHaveLength(2));
    const general = opened.find((entry) => (entry.auth as { sessionToken: string }).sessionToken === "session-general")!;
    await act(async () => {
      general.handlers.onRoomSnapshot?.({ room: { room_id: "general" }, last_seq: 10, events: [] } as never, "");
    });
    const calls = selectBackgroundRoom.mock.calls.length;
    const denied = Object.assign(new Error("denied"), { category: "authorization_failed" });
    await act(async () => {
      general.handlers.onError?.(denied);
    });
    await waitFor(() => expect(selectBackgroundRoom.mock.calls.length).toBe(calls + 1));
    await waitFor(() => expect(opened).toHaveLength(3));
    // The fresh session dies too: no further re-select, no grant spin.
    await act(async () => {
      opened[opened.length - 1].handlers.onError?.(denied);
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 50));
    });
    expect(selectBackgroundRoom.mock.calls.length).toBe(calls + 1);
    expect(opened).toHaveLength(3);
  });
});
