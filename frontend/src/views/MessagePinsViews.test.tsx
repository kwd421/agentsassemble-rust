import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { useState, type ComponentProps } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  fetchLobbyPins: vi.fn(),
  setLobbyPin: vi.fn(),
  fetchChannelLobby: vi.fn(),
  fetchRoomContext: vi.fn(),
}));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  fetchLobbyMessagePins: api.fetchLobbyPins,
  setLobbyMessagePinned: api.setLobbyPin,
  fetchChannelLobby: api.fetchChannelLobby,
  fetchRoomMessageContext: api.fetchRoomContext,
}));

import type { LobbyEvent, RoomChannel } from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";
import { createMessageAttachmentReadOwner } from "../lib/messageAttachmentReadScheduler";
import CustomChannelView from "./CustomChannelView";
import ProductionLobbyView from "./LobbyView";

function LobbyView(
  props: Omit<ComponentProps<typeof ProductionLobbyView>, "messageAttachmentReadOwner">
) {
  const [owner] = useState(() => createMessageAttachmentReadOwner());
  return <ProductionLobbyView {...props} messageAttachmentReadOwner={owner} />;
}

const room: RoomDockItem = {
  id: "general",
  label: "General",
  meetingId: "general",
  topic: "",
  shortLabel: "G",
  icon: Hash,
  createdAt: "2026-08-29T00:00:00Z",
  tone: "fresh",
};

const event: LobbyEvent = {
  id: "event-1",
  record_id: "event-1",
  seq: 1,
  kind: "message",
  name: "Operator",
  message: "pin this message",
  side: "self",
  created_at: "2026-08-29T00:00:00Z",
  actor_id: "operator-local",
  actor_type: "human",
  flow_action: "message_final",
};

const pin = {
  event_id: "event-1",
  channel_id: "lobby" as const,
  pinned_at: "2026-08-29T00:01:00Z",
  seq: 1,
  author: "Operator",
  content: "pin this message",
  created_at: "2026-08-29T00:00:00Z",
  attachment_filenames: [],
};

describe("message-pin view ownership", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    api.fetchLobbyPins.mockResolvedValue([pin]);
    api.setLobbyPin.mockResolvedValue([pin]);
    api.fetchChannelLobby.mockResolvedValue([]);
  });

  afterEach(cleanup);

  it("connects the lobby header and message action only through explicit authority", async () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        canPostMessages
        messagePinsAuthority={{ kind: "local" }}
        canonicalEvents={[event]}
        canonicalHasMoreHistory={false}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "메시지 고정" }));
    await waitFor(() =>
      expect(api.setLobbyPin).toHaveBeenCalledWith({
        roomId: "general",
        eventId: "event-1",
        pinned: true,
        authority: { kind: "local" },
        beforeDispatch: expect.any(Function),
      })
    );

    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    await waitFor(() =>
      expect(api.fetchLobbyPins).toHaveBeenCalledWith({
        roomId: "general",
        authority: { kind: "local" },
        beforeDispatch: expect.any(Function),
      })
    );
    await waitFor(() => expect(screen.getAllByText("pin this message")).toHaveLength(2));
    fireEvent.click(screen.getByRole("button", { name: /Operator.*pin this message/ }));
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "center" });
    expect(api.fetchRoomContext).not.toHaveBeenCalled();
  });

  it("does not send pin navigation through an unavailable search-context route", async () => {
    api.fetchLobbyPins.mockResolvedValue([{ ...pin, event_id: "older-event", seq: 1 }]);
    render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        messagePinsAuthority={{ kind: "remote", sessionToken: "aas1.session" }}
        canonicalEvents={[{ ...event, id: "current-event", record_id: "current-event", seq: 2 }]}
        canonicalHasMoreHistory
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    await waitFor(() => expect(api.fetchLobbyPins).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("button", { name: /Operator.*pin this message/ }));

    expect((await screen.findByRole("alert")).textContent).toContain(
      "현재 불러온 기록에 없는 고정 메시지입니다."
    );
    expect(api.fetchRoomContext).not.toHaveBeenCalled();
  });

  it("does not expose a lobby mutation when no exact authority exists", () => {
    render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        canPostMessages
        canonicalEvents={[event]}
        canonicalHasMoreHistory={false}
      />
    );

    expect(screen.queryByRole("button", { name: "메시지 고정" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    expect(
      screen.getByText("이 환경에서는 로비 메시지 핀을 사용할 수 없습니다.")
    ).toBeTruthy();
    expect(api.fetchLobbyPins).not.toHaveBeenCalled();
  });

  it("does not project an old room's delayed pin response into the next room", async () => {
    let releaseGrant: (() => void) | undefined;
    api.fetchLobbyPins.mockImplementationOnce(
      ({ beforeDispatch }: { beforeDispatch?: () => void }) =>
        new Promise<void>((resolve) => {
          releaseGrant = resolve;
        }).then(() => {
          beforeDispatch?.();
          return [pin];
        })
    );
    const { rerender } = render(
      <LobbyView
        activeRoom={room}
        agents={[]}
        messagePinsAuthority={{ kind: "local" }}
        canonicalEvents={[event]}
        canonicalHasMoreHistory={false}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    await waitFor(() => expect(api.fetchLobbyPins).toHaveBeenCalledOnce());
    rerender(
      <LobbyView
        activeRoom={{ ...room, id: "other", meetingId: "other", label: "Other" }}
        agents={[]}
        messagePinsAuthority={{ kind: "local" }}
        canonicalEvents={[]}
        canonicalHasMoreHistory={false}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    expect(api.fetchLobbyPins).toHaveBeenCalledOnce();
    await act(async () => releaseGrant?.());

    expect(screen.queryByRole("list", { name: "고정 메시지 목록" })).toBeNull();
    expect(screen.getByText("아직 고정된 메시지가 없습니다.")).toBeTruthy();
  });

  it("keeps custom-channel pins visibly unavailable", async () => {
    const channel: RoomChannel = {
      id: "planning",
      name: "planning",
      type: "text",
      position: 0,
      createdAt: "2026-08-29T00:00:00Z",
    };
    render(
      <CustomChannelView
        channel={channel}
        meetingId="general"
        sessionToken="aas1.session"
        localDisplayName="Guest"
        canPost
      />
    );

    await waitFor(() => expect(api.fetchChannelLobby).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "고정 메시지" }));
    expect(
      screen.getByText("커스텀 채널 메시지 핀은 아직 사용할 수 없습니다.")
    ).toBeTruthy();
    expect(api.fetchLobbyPins).not.toHaveBeenCalled();
    expect(api.setLobbyPin).not.toHaveBeenCalled();
  });
});
