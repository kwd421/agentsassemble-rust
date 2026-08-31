import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LobbyEvent } from "../../api";
import type { RoomDockItem } from "../../lib/roomDockModel";
import { useLobbyHistory } from "./useLobbyHistory";

afterEach(cleanup);

const room: RoomDockItem = {
  id: "room-a",
  label: "Room A",
  meetingId: "room-a",
  topic: "테스트 방",
  shortLabel: "R",
  icon: Hash,
  createdAt: "2026-08-31T00:00:00Z",
  tone: "fresh",
};

function event(id: string, kind = "message", voteId = "vote-1"): LobbyEvent {
  return {
    id,
    kind,
    name: "호스트",
    message: id,
    side: "mine",
    created_at: "2026-08-31T00:00:00Z",
    flow_meeting_id: room.meetingId,
    ...(kind === "vote" || kind === "vote_cast" ? { vote_id: voteId } : {}),
  };
}

describe("useLobbyHistory vote refresh ownership", () => {
  it("검색 문맥은 그대로 두고 투표 변경 신호만 받는다", async () => {
    const historicalPoll = event("vote-1", "vote");
    const hook = renderHook(({ canonicalEvents }) => useLobbyHistory({
      activeRoom: room,
      typingIndicators: [],
      canonicalEvents,
      canonicalHistoryReady: true,
      canonicalOldestSeq: 1,
      canonicalHasMoreHistory: false,
      canonicalWindowRevision: 1,
    }), { initialProps: { canonicalEvents: [historicalPoll] } });
    await waitFor(() => expect(hook.result.current.loaded).toBe(true));

    act(() => hook.result.current.showHistoryWindow([historicalPoll]));
    hook.rerender({ canonicalEvents: [
        event("live-message"),
        event("vote-transition-1", "vote_cast"),
        event("irrelevant-transition", "vote_cast", "vote-outside-window"),
        event("vote-transition-2", "vote_cast"),
      ] });

    expect(hook.result.current.historyWindowActive).toBe(true);
    expect(hook.result.current.visibleEvents.map(({ id }) => id)).toEqual(["vote-1"]);
    expect(hook.result.current.voteRevisions).toEqual({
      "vote-1": "vote-transition-2",
    });

    hook.rerender({ canonicalEvents: [event("later-live-message")] });
    expect(hook.result.current.voteRevisions).toEqual({
      "vote-1": "vote-transition-2",
    });
  });

  it("reconciles only mutations of records in a fixed search-history window", async () => {
    const historicalMessage: LobbyEvent = {
      ...event("search-target"),
      record_id: "record-1",
      message: "draft",
    };
    const hook = renderHook(({ canonicalEvents }) => useLobbyHistory({
      activeRoom: room,
      typingIndicators: [],
      canonicalEvents,
      canonicalHistoryReady: true,
      canonicalOldestSeq: 1,
      canonicalHasMoreHistory: false,
      canonicalWindowRevision: 1,
    }), { initialProps: { canonicalEvents: [] as LobbyEvent[] } });
    await waitFor(() => expect(hook.result.current.loaded).toBe(true));
    act(() => hook.result.current.showHistoryWindow([historicalMessage]));

    hook.rerender({ canonicalEvents: [
      { ...historicalMessage, message: "edited", edited_at: "2026-08-31T00:01:00Z" },
      event("unrelated-live-message"),
    ] });
    await waitFor(() => expect(hook.result.current.visibleEvents[0]).toMatchObject({
      id: "search-target",
      message: "edited",
    }));
    expect(hook.result.current.visibleEvents).toHaveLength(1);

    hook.rerender({ canonicalEvents: [{
      ...event("delete-search-target"),
      kind: "message_transition",
      message: "",
      flow_action: "message_deleted",
      target_event_id: "record-1",
    }] });
    await waitFor(() => expect(hook.result.current.visibleEvents[0]).toMatchObject({
      id: "search-target",
      message_deleted: true,
    }));
    expect(hook.result.current.visibleEvents).toHaveLength(1);
  });

  it("discards a history page from an authoritative window that was replaced", async () => {
    let resolvePage!: (page: {
      loadedCount: number;
      oldestSeq: number;
      hasMoreBefore: boolean;
      events: LobbyEvent[];
    }) => void;
    const pendingPage = new Promise<Parameters<typeof resolvePage>[0]>((resolve) => {
      resolvePage = resolve;
    });
    const loadCanonicalHistory = vi.fn().mockReturnValue(pendingPage);
    const hook = renderHook(({
      canonicalEvents,
      canonicalHasMoreHistory,
      canonicalOldestSeq,
      canonicalWindowRevision,
    }) => useLobbyHistory({
      activeRoom: room,
      typingIndicators: [],
      canonicalEvents,
      canonicalHistoryReady: true,
      canonicalOldestSeq,
      canonicalHasMoreHistory,
      canonicalWindowRevision,
      loadCanonicalHistory,
    }), {
      initialProps: {
        canonicalEvents: [event("old-window-message")],
        canonicalHasMoreHistory: true,
        canonicalOldestSeq: 100,
        canonicalWindowRevision: 1,
      },
    });
    await waitFor(() => expect(loadCanonicalHistory).toHaveBeenCalledWith(100));

    const replacement = Array.from({ length: 20 }, (_, index) =>
      event(`replacement-message-${index}`));
    hook.rerender({
      canonicalEvents: replacement,
      canonicalHasMoreHistory: false,
      canonicalOldestSeq: 200,
      canonicalWindowRevision: 2,
    });
    await waitFor(() => expect(hook.result.current.visibleEvents).toHaveLength(20));

    await act(async () => {
      resolvePage({
        loadedCount: 1,
        oldestSeq: 1,
        hasMoreBefore: true,
        events: [event("stale-page-message")],
      });
      await pendingPage;
    });

    expect(hook.result.current.visibleEvents.map(({ id }) => id)).not.toContain(
      "stale-page-message",
    );
    expect(hook.result.current.hasMoreHistory).toBe(false);
  });

  it("discards an in-flight page when search replaces the display in the same window", async () => {
    let resolvePage!: (page: {
      loadedCount: number;
      oldestSeq: number;
      hasMoreBefore: boolean;
      events: LobbyEvent[];
    }) => void;
    const pendingPage = new Promise<Parameters<typeof resolvePage>[0]>((resolve) => {
      resolvePage = resolve;
    });
    const loadCanonicalHistory = vi.fn().mockReturnValue(pendingPage);
    const canonicalEvents = [event("live-window-message")];
    const hook = renderHook(() => useLobbyHistory({
      activeRoom: room,
      typingIndicators: [],
      canonicalEvents,
      canonicalHistoryReady: true,
      canonicalOldestSeq: 100,
      canonicalHasMoreHistory: true,
      canonicalWindowRevision: 1,
      loadCanonicalHistory,
    }));
    await waitFor(() => expect(loadCanonicalHistory).toHaveBeenCalledWith(100));

    act(() => hook.result.current.showHistoryWindow([event("search-context-message")]));
    await act(async () => {
      resolvePage({
        loadedCount: 1,
        oldestSeq: 1,
        hasMoreBefore: true,
        events: [event("stale-page-message")],
      });
      await pendingPage;
    });

    expect(hook.result.current.visibleEvents.map(({ id }) => id)).toEqual([
      "search-context-message",
    ]);
    expect(hook.result.current.hasMoreHistory).toBe(false);
  });
});
