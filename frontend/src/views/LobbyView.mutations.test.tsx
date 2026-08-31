import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

const ownedMessage: LobbyEvent = {
  id: "message-transition",
  record_id: "message-record",
  kind: "message",
  name: "Mutation Writer",
  message: "원래 메시지",
  side: "mine",
  created_at: "2026-08-31T00:01:00Z",
  actor_id: "human-writer",
  actor_type: "human",
  flow_meeting_id: room.meetingId,
  flow_action: "message_final",
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("LobbyView message mutations", () => {
  it("uses the exact modify capability independently of current posting ability", async () => {
    const command = vi.fn().mockResolvedValue({});
    const socket = {
      ready: () => true,
      command,
    } as unknown as RoomSocketHandle;
    render(
      <RoomSocketProvider socket={socket}>
        <LobbyView
          activeRoom={room}
          agents={[]}
          canManageRoom={false}
          canPostMessages={false}
          canModifyMessages
          viewerParticipantId="human-writer"
          canonicalEvents={[ownedMessage]}
          canonicalHasMoreHistory={false}
        />
      </RoomSocketProvider>
    );

    fireEvent.click(await screen.findByRole("button", { name: "메시지 메뉴" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "수정" }));
    const editDialog = screen.getByRole("dialog", { name: "메시지 수정하기" });
    fireEvent.change(within(editDialog).getByRole("textbox"), {
      target: { value: "수정된 메시지" },
    });
    fireEvent.click(within(editDialog).getByRole("button", { name: "저장" }));
    await waitFor(() =>
      expect(command).toHaveBeenCalledWith("message.edit", {
        event_id: "message-record",
        content: "수정된 메시지",
      })
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "메시지 수정하기" })).toBeNull()
    );

    fireEvent.click(screen.getByRole("button", { name: "메시지 메뉴" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    const deleteDialog = screen.getByRole("dialog", { name: "메시지 삭제하기" });
    fireEvent.click(within(deleteDialog).getByRole("button", { name: "삭제" }));
    await waitFor(() =>
      expect(command).toHaveBeenCalledWith("message.delete", {
        event_id: "message-record",
      })
    );
  });

  it("does not expose mutation controls from posting authority alone", async () => {
    render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        canPostMessages
        canModifyMessages={false}
        viewerParticipantId="human-writer"
        canonicalEvents={[ownedMessage]}
        canonicalHasMoreHistory={false}
      />
    );

    await screen.findByText("원래 메시지");
    expect(screen.queryByRole("button", { name: "메시지 메뉴" })).toBeNull();
  });
});
