import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import RoomManagementModal from "./RoomManagementModal";
import type { useRoomLifecycle } from "../../app/useRoomLifecycle";

afterEach(cleanup);

it("shows inactive rooms, confirms restoration and keeps uncertain requests locked", () => {
  const room = { room_id: "general", room_uid: "room-one", label: "General", status: "archived", cleanup_pending: false };
  const controller: ReturnType<typeof useRoomLifecycle> = {
    enabled: true, canChange: true, open: true, busy: false, pending: null, error: "", notice: "",
    rooms: [room], show: vi.fn(), close: vi.fn(), refresh: vi.fn(async () => {}),
    change: vi.fn(), onRoomLifecycle: vi.fn(), retry: vi.fn(),
  };
  const { rerender } = render(<RoomManagementModal controller={controller} />);
  expect(screen.getByText("보관됨")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "복원" }));
  expect(controller.change).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "확인" }));
  expect(controller.change).toHaveBeenCalledWith(room, "restore");
  fireEvent.click(screen.getByRole("button", { name: "방 삭제" }));
  fireEvent.change(screen.getByLabelText("현재 방 이름"), { target: { value: "Wrong" } });
  expect((screen.getByRole("button", { name: "확인" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("현재 방 이름"), { target: { value: "General" } });
  fireEvent.click(screen.getByRole("button", { name: "확인" }));
  expect(controller.change).toHaveBeenCalledWith(room, "delete", "General");
  rerender(<RoomManagementModal controller={{ ...controller, error: "응답 미확인", pending: {
    serverId: "server", authorityLineageId: "lineage", requestId: "retry", roomId: "general", roomUid: "room-one", action: "room.archive", archived: false,
  } }} />);
  expect((screen.getByRole("button", { name: "복원" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("alert").textContent).toBe("응답 미확인");
  fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 확인" }));
  expect(controller.retry).toHaveBeenCalledOnce();
});
