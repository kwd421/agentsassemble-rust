import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { useState, type ComponentProps } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LobbyEvent } from "../api";
import { createMessageAttachmentReadOwner } from "../lib/messageAttachmentReadScheduler";
import type { RoomDockItem } from "../lib/roomDockModel";
import { RoomSocketProvider } from "../RoomSocketContext";
import type { RoomSocketHandle } from "../roomSocketClient";
import ProductionLobbyView from "./LobbyView";

function LobbyView(
  props: Omit<ComponentProps<typeof ProductionLobbyView>, "messageAttachmentReadOwner">
) {
  const [owner] = useState(() => createMessageAttachmentReadOwner());
  return <ProductionLobbyView {...props} messageAttachmentReadOwner={owner} />;
}

afterEach(cleanup);

const room: RoomDockItem = {
  id: "room-a",
  label: "Room A",
  meetingId: "room-a",
  topic: "테스트 방",
  shortLabel: "R",
  icon: Hash,
  createdAt: "2026-07-26T00:00:00Z",
  tone: "fresh",
};

const poll: LobbyEvent = {
  id: "vote-1",
  kind: "vote",
  name: "호스트",
  message: "",
  side: "mine",
  created_at: "2026-07-26T00:59:00Z",
  actor_id: "operator-local",
  actor_type: "human",
  flow_meeting_id: room.meetingId,
  vote_id: "vote-1",
  vote_question: "어느 길로 갈까요?",
  vote_options: ["북쪽", "남쪽"],
};

function voteMarker(id: string, voter: string, choice: string): LobbyEvent {
  return {
    id,
    kind: "vote_cast",
    name: "투표",
    message: `🗳️ ${voter}의 선택: 「${choice}」`,
    side: "other",
    created_at: "2026-07-26T01:00:00Z",
    actor_id: `voter-${id}`,
    actor_type: "human",
    flow_meeting_id: room.meetingId,
    flow_action: "message_final",
    vote_id: poll.vote_id,
    vote_choice: choice,
  };
}

describe("LobbyView vote results", () => {
  it("does not reveal individual ballot activity from retained timeline state", () => {
    const { container } = render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        canPostMessages={false}
        canonicalEvents={[
          voteMarker("ballot-a", "민지", "남쪽"),
          voteMarker("ballot-b", "준호", "북쪽"),
        ]}
      />
    );

    expect(container.querySelector('[data-room-event-id="ballot-a"]')).toBeNull();
    expect(container.querySelector('[data-room-event-id="ballot-b"]')).toBeNull();
    expect(screen.queryByText(/민지|준호/)).toBeNull();
  });

  it("refreshes the visible summary when an anonymous ballot marker arrives", async () => {
    const command = vi.fn().mockResolvedValue({
      op: "ack",
      request_id: "summary",
      accepted: true,
      resolution: "committed",
      action: "room.vote.summary",
      result: {
        vote_id: poll.vote_id,
        question: poll.vote_question,
        options: poll.vote_options,
        vote_duration_seconds: 0,
        vote_deadline_at: "",
        created_by: poll.name,
        created_at: poll.created_at,
        tallies: { 북쪽: 0, 남쪽: 0 },
        own_choice: "",
        total_votes: 0,
        closed: false,
        closed_at: "",
        close_reason: "",
      },
    });
    const socket = {
      ready: () => true,
      command,
    } as unknown as RoomSocketHandle;
    const renderView = (events: LobbyEvent[]) => (
      <RoomSocketProvider socket={socket}>
        <LobbyView
          activeRoom={room}
          agents={[]}
          canonicalEvents={events}
          canonicalHasMoreHistory={false}
        />
      </RoomSocketProvider>
    );
    const view = render(renderView([poll]));
    await waitFor(() => expect(command).toHaveBeenCalledTimes(1));

    view.rerender(renderView([poll, voteMarker("ballot-a", "민지", "남쪽")]));
    await waitFor(() => expect(command).toHaveBeenCalledTimes(2));
    expect(screen.queryByText("민지")).toBeNull();
  });
});
