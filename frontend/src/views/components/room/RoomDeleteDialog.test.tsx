import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import RoomDeleteDialog, { type RoomLifecycleController } from "./RoomDeleteDialog";

afterEach(cleanup);

const listed = { room_id: "general", room_uid: "room-one", label: "General", status: "active" };
const target = { roomId: "general", roomUid: "room-one", label: "General" };

function controllerFor(overrides: Partial<RoomLifecycleController> = {}): RoomLifecycleController {
  return {
    canChange: true, busy: false, pending: null, error: "", notice: "",
    rooms: [listed], refresh: vi.fn(async () => {}), change: vi.fn(), retry: vi.fn(),
    ...overrides,
  };
}

it("deletes only after the exact room name is typed back", () => {
  const controller = controllerFor();
  render(<RoomDeleteDialog target={target} controller={controller} onClose={vi.fn()} />);

  expect(controller.refresh).toHaveBeenCalledOnce();
  expect(screen.getByRole("alertdialog").textContent).toContain("되돌릴 수 없어요");
  const confirm = () => screen.getByRole("button", { name: "방 삭제" }) as HTMLButtonElement;
  expect(confirm().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("방 이름 입력"), { target: { value: "general" } });
  expect(confirm().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("방 이름 입력"), { target: { value: "General" } });
  fireEvent.click(confirm());
  expect(controller.change).toHaveBeenCalledWith(listed, "delete", "General");
});

it("refuses a room the directory refresh did not return", () => {
  const controller = controllerFor({ rooms: [] });
  render(<RoomDeleteDialog target={target} controller={controller} onClose={vi.fn()} />);

  fireEvent.change(screen.getByLabelText("방 이름 입력"), { target: { value: "General" } });
  expect((screen.getByRole("button", { name: "방 삭제" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByRole("status").textContent).toContain("확인하지 못했어요");
});

it("keeps an uncertain deletion retryable and reports its failure", () => {
  const controller = controllerFor({
    error: "응답 미확인",
    pending: { serverId: "server", authorityLineageId: "lineage", requestId: "retry", roomId: "general", roomUid: "room-one", action: "room.delete", confirmationName: "General" },
  });
  render(<RoomDeleteDialog target={target} controller={controller} onClose={vi.fn()} />);

  expect(screen.getByRole("alert").textContent).toBe("응답 미확인");
  fireEvent.change(screen.getByLabelText("방 이름 입력"), { target: { value: "General" } });
  expect((screen.getByRole("button", { name: "방 삭제" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 확인" }));
  expect(controller.retry).toHaveBeenCalledOnce();
});
