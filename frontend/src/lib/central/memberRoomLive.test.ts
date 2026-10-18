import { describe, expect, it } from "vitest";
import {
  MEMBER_HOST_LIVE_LOSS_FAST_MAX_MS,
  MEMBER_HOST_LIVE_LOSS_FAST_MIN_MS,
  MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP,
  MEMBER_HOST_RECONNECT_BASE_MS,
  MEMBER_HOST_RECONNECT_CAP_MS,
  MEMBER_HOST_RECONNECT_RATE_LIMIT_MS,
  MEMBER_HOST_RECONNECT_SLOW_BASE_MS,
  MEMBER_HOST_RECONNECT_SLOW_CAP_MS,
  MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS,
  MEMBER_HOST_RECONNECT_SLOW_STEADY_MAX_MS,
  MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS,
  MEMBER_ROOM_LIVE_SOCKETS_PER_HOST,
  memberHostOfflineBudgetExhausted,
  memberHostLiveLossFastDelay,
  memberHostOfflineParkMs,
  memberHostOfflinePreviewCount,
  memberHostReconnectCause,
  memberHostReconnectDelay,
  memberHostRetryAfterMs,
  noteMemberHostOfflinePreview,
  resetMemberHostOfflineBudgetForTests,
} from "./memberRoomLive";
import { messageMentionsViewer, pickLiveRoomIds, projectMemberRoomBadge } from "./memberRoomLive";

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
  // Deterministic jitter: random () => 0.5 keeps the exact midpoint.
  const mid = () => 0.5;

  it("doubles the transient backoff from the base and caps it", () => {
    expect(memberHostReconnectDelay(0, "transient", null, mid)).toBe(MEMBER_HOST_RECONNECT_BASE_MS);
    expect(memberHostReconnectDelay(1, "transient", null, mid)).toBe(MEMBER_HOST_RECONNECT_BASE_MS * 2);
    expect(memberHostReconnectDelay(100, "transient", null, mid)).toBe(MEMBER_HOST_RECONNECT_CAP_MS);
  });

  it("keeps transient retries within jitter of the base", () => {
    const delay = memberHostReconnectDelay(0);
    expect(delay).toBeGreaterThanOrEqual(Math.floor(MEMBER_HOST_RECONNECT_BASE_MS * 0.8));
    expect(delay).toBeLessThanOrEqual(Math.ceil(MEMBER_HOST_RECONNECT_BASE_MS * 1.2));
  });

  it("rests a revoked endpoint on a slow ladder, then 30-60 min steady", () => {
    expect(memberHostReconnectDelay(0, "offline", null, mid)).toBe(MEMBER_HOST_RECONNECT_SLOW_BASE_MS);
    expect(memberHostReconnectDelay(1, "offline", null, mid)).toBe(MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS[1]);
    expect(memberHostReconnectDelay(2, "offline", null, mid)).toBe(MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS[2]);
    expect(memberHostReconnectDelay(3, "offline", null, mid)).toBe(MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS[3]);
    expect(memberHostReconnectDelay(4, "offline", null, mid)).toBe(MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS[4]);
    // Steady state is uniform 30–60 min: mid lands on the 45 min midpoint.
    expect(memberHostReconnectDelay(5, "offline", null, mid)).toBe(
      Math.floor((MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS + MEMBER_HOST_RECONNECT_SLOW_STEADY_MAX_MS) / 2),
    );
    expect(memberHostReconnectDelay(100, "offline", null, mid)).toBe(
      Math.floor((MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS + MEMBER_HOST_RECONNECT_SLOW_STEADY_MAX_MS) / 2),
    );
    expect(memberHostReconnectDelay(5, "offline", null, () => 0)).toBe(MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS);
    expect(memberHostReconnectDelay(5, "offline", null, () => 0.9999)).toBeLessThanOrEqual(MEMBER_HOST_RECONNECT_SLOW_CAP_MS);
    // Even the fastest jitter never drops below ~48 s on the slow path.
    expect(memberHostReconnectDelay(0, "offline", null, () => 0)).toBeGreaterThanOrEqual(48_000);
  });

  it("parks the host after the daily offline-preview budget is spent", () => {
    resetMemberHostOfflineBudgetForTests();
    const now = Date.UTC(2026, 9, 17, 12, 0, 0);
    for (let i = 0; i < MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP; i += 1) {
      expect(memberHostOfflineBudgetExhausted("person", "host", now)).toBe(false);
      noteMemberHostOfflinePreview("person", "host", now);
    }
    expect(memberHostOfflinePreviewCount("person", "host", now)).toBe(MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP);
    expect(memberHostOfflineBudgetExhausted("person", "host", now)).toBe(true);
    // Another host on the same account keeps its own budget.
    expect(memberHostOfflineBudgetExhausted("person", "other-host", now)).toBe(false);
    expect(memberHostOfflineParkMs(now)).toBe(Date.UTC(2026, 9, 18) - now);
    // The next UTC day resets the budget.
    expect(memberHostOfflineBudgetExhausted("person", "host", Date.UTC(2026, 9, 18))).toBe(false);
    resetMemberHostOfflineBudgetForTests();
  });

  it("honors the server's Retry-After on a 429 exactly", () => {
    expect(memberHostReconnectDelay(0, "rate_limited", 120_000, mid)).toBe(120_000);
    expect(memberHostReconnectDelay(3, "rate_limited", 500, mid)).toBe(1_000);
    expect(memberHostReconnectDelay(0, "rate_limited", 3_600_000, mid)).toBe(600_000);
  });

  it("rests a long default without a Retry-After header", () => {
    const delay = memberHostReconnectDelay(0, "rate_limited", null, mid);
    expect(delay).toBe(MEMBER_HOST_RECONNECT_RATE_LIMIT_MS);
  });
});

describe("memberHostLiveLossFastDelay", () => {
  it("lands in the 15-30 s jittered band so a restarting host can publish", () => {
    expect(memberHostLiveLossFastDelay(() => 0)).toBe(MEMBER_HOST_LIVE_LOSS_FAST_MIN_MS);
    expect(memberHostLiveLossFastDelay(() => 0.5)).toBe(22_500);
    expect(memberHostLiveLossFastDelay(() => 1)).toBe(MEMBER_HOST_LIVE_LOSS_FAST_MAX_MS);
    for (const random of [0.1, 0.33, 0.75, 0.99]) {
      const delay = memberHostLiveLossFastDelay(() => random);
      expect(delay).toBeGreaterThanOrEqual(MEMBER_HOST_LIVE_LOSS_FAST_MIN_MS);
      expect(delay).toBeLessThanOrEqual(MEMBER_HOST_LIVE_LOSS_FAST_MAX_MS);
    }
    // The fast slot is well ahead of the slow ladder's first step.
    expect(memberHostLiveLossFastDelay(() => 1)).toBeLessThan(memberHostReconnectDelay(0, "offline", null, () => 0));
  });
});

describe("memberHostReconnectCause", () => {
  it("treats a revoked endpoint as host-offline", () => {
    expect(memberHostReconnectCause(Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" }))).toBe("offline");
  });

  it("treats central rate limiting as rate-limited", () => {
    expect(memberHostReconnectCause(Object.assign(new Error("slow"), { status: 429, code: "rate_limited" }))).toBe("rate_limited");
  });

  it("treats everything else as transient", () => {
    expect(memberHostReconnectCause(new Error("boom"))).toBe("transient");
    expect(memberHostReconnectCause(Object.assign(new Error("gone"), { status: 409, code: "other" }))).toBe("transient");
    expect(memberHostReconnectCause(Object.assign(new Error("denied"), { status: 403, code: "forbidden" }))).toBe("transient");
  });

  it("reads the server's wait from retryAfterMs only", () => {
    expect(memberHostRetryAfterMs(Object.assign(new Error("slow"), { status: 429, retryAfterMs: 45_000 }))).toBe(45_000);
    expect(memberHostRetryAfterMs(new Error("boom"))).toBeNull();
    expect(memberHostRetryAfterMs(Object.assign(new Error("bad"), { retryAfterMs: Number.NaN }))).toBeNull();
  });
});
