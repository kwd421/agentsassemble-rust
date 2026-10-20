import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { Radio } from "../../ui/icons";
import RoomRail from "./RoomRail";
import type { RoomDockItem } from "../../lib/roomDockModel";

afterEach(cleanup);

const room: RoomDockItem = {
  id: "server-room-one", label: "General", meetingId: "general", roomUid: "room-one",
  roomOrigin: "local", connectionState: "connected", topic: "일반", shortLabel: "G",
  icon: Radio, createdAt: "", tone: "resident",
};

it("shows rooms only with per-room connection state and no server icons", () => {
  const offline: RoomDockItem = {
    ...room, id: "server-room-two", label: "Friend", meetingId: "friend", roomUid: "room-two",
    roomOrigin: "remote_server", serverId: "friend-host", connectionState: "disconnected",
  };
  render(
    <RoomRail
      rooms={[room, offline]} activeRoom={room} roomAppearances={{}} guestLocked={false} adminOpen={false}
      roomMenu={null}
      onSelectRoom={vi.fn()} onAddRoom={vi.fn()} onOpenRoomMenu={vi.fn()} onMarkRoomRead={vi.fn()}
      onInviteRoom={vi.fn()} onOpenRoomSettings={vi.fn()} onLeaveRoom={vi.fn()}
    />
  );

  // The rail carries one button per room plus the add button: no server or
  // computer icons, and each room reports its own connection state.
  const buttons = screen.getAllByRole("button");
  expect(buttons).toHaveLength(3);
  expect(screen.getByRole("button", { name: "General" }).getAttribute("data-connection-state")).toBe("connected");
  const offlineButton = screen.getByRole("button", { name: "Friend · 연결이 끊긴 방" });
  expect(offlineButton.getAttribute("data-connection-state")).toBe("disconnected");
  expect(offlineButton.title).toContain("연결 끊김");
});

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

const secondRoom: RoomDockItem = {
  ...room, id: "server-room-two", label: "Second", meetingId: "second", roomUid: "room-two",
};

function renderRail(rooms: RoomDockItem[], active: RoomDockItem) {
  render(
    <RoomRail
      rooms={rooms} activeRoom={active} roomAppearances={{}} guestLocked={false} adminOpen={false}
      roomMenu={null}
      onSelectRoom={vi.fn()} onAddRoom={vi.fn()} onOpenRoomMenu={vi.fn()} onMarkRoomRead={vi.fn()}
      onInviteRoom={vi.fn()} onOpenRoomSettings={vi.fn()} onLeaveRoom={vi.fn()}
    />
  );
}

it("draws the unread pill and mention badge only where live state says so", () => {
  const unread: RoomDockItem = {
    ...room, id: "server-room-unread", label: "Unread", meetingId: "unread",
    roomOrigin: "remote_server", serverId: "friend-host", connectionState: "connected",
    liveUnread: true,
  };
  const mentioned: RoomDockItem = {
    ...room, id: "server-room-mentioned", label: "Mentioned", meetingId: "mentioned",
    roomOrigin: "remote_server", serverId: "friend-host", connectionState: "connected",
    liveUnread: true, liveMention: true,
  };
  renderRail([room, unread, mentioned], room);

  // Unread background room renders the small white pill.
  expect(screen.getByTestId("room-unread-server-room-unread")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Unread" }).getAttribute("data-live-unread")).toBe("true");
  // Mention renders the red badge dot.
  expect(screen.getByTestId("room-mention-server-room-mentioned")).toBeTruthy();
  expect(screen.getByRole("button", { name: "Mentioned" }).getAttribute("data-live-mention")).toBe("true");
  // Read room renders neither.
  expect(screen.queryByTestId("room-unread-server-room-one")).toBeNull();
  expect(screen.queryByTestId("room-mention-server-room-one")).toBeNull();
  // Active room keeps its taller active pill, never the small unread pill.
  const activeButton = screen.getByRole("button", { name: "General" });
  expect(activeButton.getAttribute("data-active")).toBe("true");
  expect(screen.queryByTestId("room-unread-server-room-one")).toBeNull();
});

it("keeps the active pill for an active room with stale live state", () => {
  const staleActive: RoomDockItem = { ...room, liveUnread: true, liveMention: true };
  renderRail([staleActive], staleActive);
  const activeButton = screen.getByRole("button", { name: "General" });
  expect(activeButton.getAttribute("data-active")).toBe("true");
  expect(screen.queryByTestId(`room-unread-${staleActive.id}`)).toBeNull();
});

function renderSelectableRail(onSelectRoom: (roomId: string) => void) {
  render(
    <RoomRail
      rooms={[room, secondRoom]} activeRoom={room} roomAppearances={{}} guestLocked={false} adminOpen={false}
      roomMenu={null}
      onSelectRoom={onSelectRoom} onAddRoom={vi.fn()} onOpenRoomMenu={vi.fn()} onMarkRoomRead={vi.fn()}
      onInviteRoom={vi.fn()} onOpenRoomSettings={vi.fn()} onLeaveRoom={vi.fn()}
    />
  );
  return screen.getByRole("button", { name: "Second" });
}

it("selects the room on a back-to-back press with no hold", () => {
  const onSelectRoom = vi.fn();
  const button = renderSelectableRail(onSelectRoom);
  fireEvent.pointerDown(button, { button: 0, clientX: 10, clientY: 10, pointerType: "mouse" });
  fireEvent.mouseDown(button, { button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerUp(button, { button: 0, clientX: 10, clientY: 10, pointerType: "mouse" });
  fireEvent.mouseUp(button, { button: 0, clientX: 10, clientY: 10 });
  fireEvent.click(button);
  expect(onSelectRoom).toHaveBeenCalledTimes(1);
  expect(onSelectRoom).toHaveBeenCalledWith("server-room-two");
});

it("selects the room on pointerdown/pointerup alone when no click follows", () => {
  const onSelectRoom = vi.fn();
  const button = renderSelectableRail(onSelectRoom);
  fireEvent.pointerDown(button, { button: 0, clientX: 10, clientY: 10, pointerType: "mouse" });
  fireEvent.pointerUp(button, { button: 0, clientX: 10, clientY: 10, pointerType: "mouse" });
  expect(onSelectRoom).toHaveBeenCalledTimes(1);
  expect(onSelectRoom).toHaveBeenCalledWith("server-room-two");
});

it("still selects on click alone for keyboard and assistive activation", () => {
  const onSelectRoom = vi.fn();
  const button = renderSelectableRail(onSelectRoom);
  fireEvent.click(button);
  expect(onSelectRoom).toHaveBeenCalledTimes(1);
  expect(onSelectRoom).toHaveBeenCalledWith("server-room-two");
});

it("does not select after a drag beyond the reorder tolerance", () => {
  const onSelectRoom = vi.fn();
  const button = renderSelectableRail(onSelectRoom);
  fireEvent.pointerDown(button, { button: 0, clientX: 10, clientY: 10, pointerType: "mouse" });
  fireEvent.pointerUp(button, { button: 0, clientX: 10, clientY: 40, pointerType: "mouse" });
  fireEvent.click(button);
  expect(onSelectRoom).not.toHaveBeenCalled();
});
