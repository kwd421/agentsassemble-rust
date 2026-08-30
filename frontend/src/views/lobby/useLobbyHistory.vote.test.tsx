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

function event(id: string, kind = "message"): LobbyEvent {
  return {
    id,
    kind,
    name: "호스트",
    message: id,
    side: "mine",
    created_at: "2026-08-31T00:00:00Z",
    flow_meeting_id: room.meetingId,
    ...(kind === "vote" || kind === "vote_cast" ? { vote_id: "vote-1" } : {}),
  };
}

describe("useLobbyHistory vote refresh ownership", () => {
  it("검색 문맥은 그대로 두고 투표 변경 신호만 받는다", async () => {
    let receive: (incoming: LobbyEvent[]) => void = () => undefined;
    const bindLobbyStream = vi.fn((next: (incoming: LobbyEvent[]) => void) => {
      receive = next;
      return () => undefined;
    });
    const historicalPoll = event("vote-1", "vote");
    const canonicalEvents = [historicalPoll];
    const hook = renderHook(() => useLobbyHistory({
      activeRoom: room,
      typingIndicators: [],
      bindLobbyStream,
      canonicalEvents,
      canonicalHistoryReady: true,
      canonicalOldestSeq: 1,
      canonicalHasMoreHistory: false,
    }));
    await waitFor(() => expect(hook.result.current.loaded).toBe(true));

    act(() => hook.result.current.showHistoryWindow([historicalPoll]));
    act(() => receive([
      event("live-message"),
      event("vote-transition", "vote_cast"),
    ]));

    expect(hook.result.current.historyWindowActive).toBe(true);
    expect(hook.result.current.visibleEvents.map(({ id }) => id)).toEqual(["vote-1"]);
    expect(hook.result.current.voteRevisions["vote-1"]).toContain("vote-transition");
  });
});
