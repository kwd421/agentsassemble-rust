import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_ROOM_APPEARANCE } from "../../lib/roomAppearance";
import { participantFixture } from "../../test/participant";
import MobileRoomInfoPanel from "./MobileRoomInfoPanel";
import RoomConnectionPanel from "./RoomConnectionPanel";

afterEach(cleanup);

describe("participant removal surfaces", () => {
  it.each(["desktop", "mobile"])("gates %s moderation and removes the selected human", (surface) => {
    const onRemove = vi.fn(async () => {});
    const props = {
      room: { id: "general", label: "General", meetingId: "general", topic: "", tone: "" },
      agents: [],
      members: [participantFixture(), participantFixture({ participant_id: "guest", display_name: "Guest" })],
      onParticipantRemove: onRemove,
    };
    const view = (capabilities: Record<string, boolean>) => surface === "desktop"
      ? <RoomConnectionPanel {...props} capabilities={capabilities} />
      : <MobileRoomInfoPanel {...props} capabilities={capabilities} appearance={DEFAULT_ROOM_APPEARANCE} channelLabel="general" onClose={vi.fn()} />;
    const { rerender } = render(view({}));
    expect(screen.queryByRole("button", { name: "Guest 강퇴" })).toBeNull();
    rerender(view({ "room.manage": true }));
    expect(screen.queryByRole("button", { name: "Guest 강퇴" })).toBeNull();
    fireEvent.click(screen.getByLabelText("Guest 관리 메뉴"));
    expect(screen.getAllByRole("button", { name: /강퇴$/ })).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Guest 강퇴" }));
    fireEvent.click(screen.getByRole("button", { name: "강퇴" }));
    expect(onRemove).toHaveBeenCalledWith("guest", "kick");
  });
});
