import { describe, expect, it } from "vitest";
import {
  MEMBER_HOST_RECONNECT_BASE_MS,
  MEMBER_HOST_RECONNECT_CAP_MS,
  MEMBER_ROOM_LIVE_SOCKETS_PER_HOST,
  memberHostReconnectDelay,
  messageMentionsViewer,
  pickLiveRoomIds,
  projectMemberRoomBadge,
} from "./memberRoomLive";

const message = (overrides = {}) => ({
  v: 1,
  id: "event",
  seq: 1,
  created_at: new Date().toISOString(),
  room_id: "room",
  type: "message_final",
  actor: { participant_id: "other", participant_type: "human" },
  participant_id: "other",
  message_kind: "message",
  content: "hello",
  ...overrides,
});

describe("projectMemberRoomBadge", () => {
  it("marks a new real message unread", () => {
    const { badge, latestSeq } = projectMemberRoomBadge({
      events: [message({ seq: 4 })],
      floorSeq: 3,
      viewer: { participantId: "me", displayName: "나" },
    });
    expect(badge).toEqual({ unread: true, mention: false });
    expect(latestSeq).toBe(4);
  });

  it("never counts the viewer's own messages as unread", () => {
    const { badge } = projectMemberRoomBadge({
      events: [message({ seq: 4, participant_id: "me" })],
      floorSeq: 3,
      viewer: { participantId: "me", displayName: "나" },
    });
    expect(badge.unread).toBe(false);
  });

  it("never counts lobby system lines as unread", () => {
    const { badge } = projectMemberRoomBadge({
      events: [message({ seq: 4, message_kind: "system" })],
      floorSeq: 3,
      viewer: { participantId: "me", displayName: "나" },
    });
    expect(badge.unread).toBe(false);
  });

  it("sets the mention flag for structured and plain mentions", () => {
    const viewer = { participantId: "me", displayName: "나" };
    expect(messageMentionsViewer("hey <@me> look", viewer)).toBe(true);
    expect(messageMentionsViewer("hey @나 look", viewer)).toBe(true);
    expect(messageMentionsViewer("hey <@other> look", viewer)).toBe(false);
    expect(messageMentionsViewer("just hello", viewer)).toBe(false);
    const { badge } = projectMemberRoomBadge({
      events: [message({ seq: 4, content: "hey <@me>" })],
      floorSeq: 3,
      viewer,
    });
    expect(badge).toEqual({ unread: true, mention: true });
  });

  it("ignores events at or below the floor but still tracks the latest seq", () => {
    const { badge, latestSeq } = projectMemberRoomBadge({
      events: [message({ seq: 3 }), message({ seq: 5, message_kind: "system" })],
      floorSeq: 3,
      viewer: { participantId: "me", displayName: "나" },
    });
    expect(badge).toEqual({ unread: false, mention: false });
    expect(latestSeq).toBe(5);
  });
});

describe("pickLiveRoomIds", () => {
  it("excludes the active room and caps background coverage", () => {
    const rooms = ["active", ...Array.from({ length: MEMBER_ROOM_LIVE_SOCKETS_PER_HOST + 3 }, (_, index) => `room-${index}`)];
    const picked = pickLiveRoomIds(rooms, "active");
    expect(picked).toHaveLength(MEMBER_ROOM_LIVE_SOCKETS_PER_HOST);
    expect(picked).not.toContain("active");
  });
});

describe("memberHostReconnectDelay", () => {
  it("doubles from the base and caps", () => {
    expect(memberHostReconnectDelay(0)).toBe(MEMBER_HOST_RECONNECT_BASE_MS);
    expect(memberHostReconnectDelay(1)).toBe(MEMBER_HOST_RECONNECT_BASE_MS * 2);
    expect(memberHostReconnectDelay(100)).toBe(MEMBER_HOST_RECONNECT_CAP_MS);
  });
});
