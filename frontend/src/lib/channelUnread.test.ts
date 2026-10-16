import { describe, expect, it } from "vitest";
import type { RoomEvent } from "../api";
import {
  filterCollapsedChannels,
  latestChannelSequence,
  parseChannelMessageCursors,
  projectChannelUnread,
} from "./channelUnread";

const A = "c0123456789ab";
const B = "c0123456789ac";

function lobbyMessage(seq: number, messageKind = "message"): RoomEvent {
  return {
    v: 1, id: `lobby-${seq}`, seq, created_at: "2026-10-16T00:00:00Z",
    room_id: "general", type: "message_final",
    actor: { participant_id: "agent-one", participant_type: "agent" },
    message_kind: messageKind, content: "hello",
  };
}

function channelMessage(seq: number, channelId: string): RoomEvent {
  return {
    v: 1, id: `channel-${seq}`, seq, created_at: "2026-10-16T00:00:00Z",
    room_id: "general", type: "channel_message_final",
    actor: { participant_id: "agent-one", participant_type: "agent" },
    message_kind: "message", content: "hello",
    channel_id: channelId,
  } as RoomEvent;
}

function settings(entries: Record<string, { notifications?: "default" | "mute"; lastReadAt?: string }>) {
  return Object.fromEntries(
    Object.entries(entries).map(([id, value]) => [
      id, { notifications: value.notifications || "default", ...(value.lastReadAt ? { lastReadAt: value.lastReadAt } : {}) },
    ])
  );
}

describe("per-channel sidebar unread", () => {
  it("marks only the channel with the newer message unread", () => {
    const unread = projectChannelUnread({
      channelIds: ["lobby", A, B],
      cursors: { [A]: 5 },
      eventLists: [[]],
      settings: settings({ lobby: { lastReadAt: "seq:9" }, [A]: { lastReadAt: "seq:3" }, [B]: { lastReadAt: "seq:9" } }),
    });
    expect(unread).toEqual({ lobby: false, [A]: true, [B]: false });
  });

  it("clears the channel once it is read", () => {
    const cursors = { [A]: 5 };
    const read = (lastReadAt: string) =>
      projectChannelUnread({
        channelIds: [A],
        cursors,
        eventLists: [[]],
        settings: settings({ [A]: { lastReadAt } }),
      })[A];
    expect(read("seq:3")).toBe(true);
    expect(read("seq:5")).toBe(false);
  });

  it("merges live and history events above the snapshot floor", () => {
    expect(
      latestChannelSequence(A, { [A]: 4 }, [[channelMessage(7, B)], [channelMessage(6, A)]])
    ).toBe(6);
    const unread = projectChannelUnread({
      channelIds: [A, B],
      cursors: {},
      eventLists: [[channelMessage(7, B)]],
      settings: settings({ [A]: {}, [B]: { lastReadAt: "seq:5" } }),
    });
    expect(unread).toEqual({ [A]: false, [B]: true });
  });

  it("never marks muted channels unread", () => {
    const unread = projectChannelUnread({
      channelIds: [A],
      cursors: { [A]: 9 },
      eventLists: [[]],
      settings: settings({ [A]: { notifications: "mute", lastReadAt: "seq:3" } }),
    });
    expect(unread[A]).toBe(false);
  });

  it("ignores system lines", () => {
    const unread = projectChannelUnread({
      channelIds: ["lobby"],
      cursors: {},
      eventLists: [[lobbyMessage(10, "system")]],
      settings: settings({ lobby: { lastReadAt: "seq:9" } }),
    });
    expect(unread.lobby).toBe(false);
    expect(
      projectChannelUnread({
        channelIds: ["lobby"],
        cursors: {},
        eventLists: [[lobbyMessage(10)]],
        settings: settings({ lobby: { lastReadAt: "seq:9" } }),
      }).lobby
    ).toBe(true);
  });

  it("treats channels as read when the host omits cursors", () => {
    expect(parseChannelMessageCursors(undefined)).toEqual({});
    const unread = projectChannelUnread({
      channelIds: ["lobby", A, B],
      cursors: parseChannelMessageCursors(undefined),
      eventLists: [[]],
      settings: settings({}),
    });
    expect(unread).toEqual({ lobby: false, [A]: false, [B]: false });
  });

  it("keeps the open channel and unread channels in a collapsed category", () => {
    const channels = [{ id: A }, { id: B }, { id: "lobby" }];
    expect(filterCollapsedChannels(channels, B, { [A]: true, [B]: false, lobby: false })).toEqual([
      { id: A }, { id: B },
    ]);
    expect(filterCollapsedChannels(channels, "lobby", { [A]: false, [B]: false, lobby: false })).toEqual([
      { id: "lobby" },
    ]);
  });
});
