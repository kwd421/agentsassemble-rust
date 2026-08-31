import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { afterEach, describe, expect, it } from "vitest";

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
  });
});
