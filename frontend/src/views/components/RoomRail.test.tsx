import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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

function renderRail(overrides: Partial<Parameters<typeof RoomRail>[0]> = {}) {
  const onRoomLifecycle = vi.fn();
  render(
    <RoomRail
      rooms={[room]} activeRoom={room} roomAppearances={{}} guestLocked={false} adminOpen={false}
      menuRoom={room} roomMenu={{ roomId: room.id, x: 10, y: 10 }}
      onSelectRoom={vi.fn()} onAddRoom={vi.fn()} onOpenRoomMenu={vi.fn()} onMarkRoomRead={vi.fn()}
      onInviteRoom={vi.fn()} onOpenRoomSettings={vi.fn()} onLeaveRoom={vi.fn()}
      onRoomLifecycle={onRoomLifecycle}
      {...overrides}
    />
  );
  return { onRoomLifecycle };
}

it("keeps room lifecycle on the server menu rather than a rail button of its own", () => {
  const { onRoomLifecycle } = renderRail();

  expect(screen.queryByRole("button", { name: "방 관리" })).toBeNull();
  fireEvent.click(screen.getByRole("menuitem", { name: "방 보관" }));
  expect(onRoomLifecycle).toHaveBeenCalledWith(room, "archive");
  fireEvent.click(screen.getByRole("menuitem", { name: "방 종료" }));
  expect(onRoomLifecycle).toHaveBeenLastCalledWith(room, "close");
  fireEvent.click(screen.getByRole("menuitem", { name: "방 삭제" }));
  expect(onRoomLifecycle).toHaveBeenLastCalledWith(room, "delete");
});

it("offers no lifecycle action to a guest or without a controller", () => {
  renderRail({ guestLocked: true });
  expect(screen.queryByRole("menuitem", { name: "방 삭제" })).toBeNull();

  cleanup();
  renderRail({ onRoomLifecycle: undefined });
  expect(screen.queryByRole("menuitem", { name: "방 삭제" })).toBeNull();
});
