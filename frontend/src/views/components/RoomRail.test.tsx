import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { Radio } from "lucide-react";
import RoomRail from "./RoomRail";
import type { RoomDockItem } from "../../lib/roomDockModel";

afterEach(cleanup);

const room: RoomDockItem = {
  id: "server-room-one", label: "General", meetingId: "general", roomUid: "room-one",
  roomOrigin: "local", connectionState: "connected", topic: "일반", shortLabel: "G",
  icon: Radio, createdAt: "", tone: "resident",
};

it("keeps destructive room actions out of the rail and its menu", () => {
  render(
    <RoomRail
      rooms={[room]} activeRoom={room} roomAppearances={{}} guestLocked={false} adminOpen={false}
      menuRoom={room} roomMenu={{ roomId: room.id, x: 10, y: 10 }}
      onSelectRoom={vi.fn()} onAddRoom={vi.fn()} onOpenRoomMenu={vi.fn()} onMarkRoomRead={vi.fn()}
      onInviteRoom={vi.fn()} onOpenRoomSettings={vi.fn()} onLeaveRoom={vi.fn()}
    />
  );

  // Deleting a room belongs to its settings, next to everything else about that room.
  expect(screen.getByRole("menuitem", { name: "방 설정" })).toBeTruthy();
  expect(screen.queryByRole("menuitem", { name: "방 삭제" })).toBeNull();
  expect(screen.queryByRole("menuitem", { name: "방 보관" })).toBeNull();
  expect(screen.queryByRole("menuitem", { name: "방 종료" })).toBeNull();
  expect(screen.queryByRole("button", { name: "방 관리" })).toBeNull();
});
