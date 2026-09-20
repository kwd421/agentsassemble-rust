import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ArchivedRoomList from "./ArchivedRoomList";
import RoomLifecycleConfirm, { type RoomLifecycleController } from "./RoomLifecycleConfirm";

afterEach(cleanup);

const archived = { room_id: "general", room_uid: "room-one", label: "General", status: "archived", cleanup_pending: false };

function controllerFor(overrides: Partial<RoomLifecycleController> = {}): RoomLifecycleController {
  return {
    canChange: true, busy: false, pending: null, error: "", notice: "",
    rooms: [archived], refresh: vi.fn(async () => {}), change: vi.fn(), retry: vi.fn(),
    ...overrides,
  };
}

it("deletes only after the exact room name, and only a room the controller listed", () => {
  const controller = controllerFor();
  const { rerender } = render(
    <RoomLifecycleConfirm target={{ roomId: "general", roomUid: "room-one", label: "General" }}
      action="delete" controller={controller} onClose={vi.fn()} />
  );

  expect(controller.refresh).toHaveBeenCalledOnce();
  const confirm = () => screen.getByRole("button", { name: "방 삭제" }) as HTMLButtonElement;
  expect(confirm().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("현재 방 이름"), { target: { value: "Wrong" } });
  expect(confirm().disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("현재 방 이름"), { target: { value: "General" } });
  fireEvent.click(confirm());
  expect(controller.change).toHaveBeenCalledWith(archived, "delete", "General");

  // A room the refresh did not return stays unactionable instead of guessing at its identity.
  const unlisted = controllerFor({ rooms: [] });
  rerender(
    <RoomLifecycleConfirm target={{ roomId: "general", roomUid: "room-one", label: "General" }}
      action="delete" controller={unlisted} onClose={vi.fn()} />
  );
  fireEvent.change(screen.getByLabelText("현재 방 이름"), { target: { value: "General" } });
  expect(confirm().disabled).toBe(true);
});

it("keeps an uncertain lifecycle request retryable and reports its failure", () => {
  const controller = controllerFor({
    error: "응답 미확인",
    pending: { serverId: "server", authorityLineageId: "lineage", requestId: "retry", roomId: "general", roomUid: "room-one", action: "room.archive", archived: false },
  });
  render(
    <RoomLifecycleConfirm target={{ roomId: "general", roomUid: "room-one", label: "General" }}
      action="archive" controller={controller} onClose={vi.fn()} />
  );

  expect(screen.getByRole("alert").textContent).toBe("응답 미확인");
  expect((screen.getByRole("button", { name: "보관" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 확인" }));
  expect(controller.retry).toHaveBeenCalledOnce();
});

it("lists the rooms the rail cannot show and restores one after confirmation", () => {
  const controller = controllerFor({
    rooms: [archived, { room_id: "closed-room", label: "Closed", status: "closed" }, { room_id: "live", label: "Live", status: "active" }],
  });
  render(<ArchivedRoomList controller={controller} />);

  expect(screen.getByText("General")).toBeTruthy();
  expect(screen.getByText("Closed")).toBeTruthy();
  expect(screen.queryByText("Live")).toBeNull();
  // A closed room cannot come back, so only the archived one offers restoration.
  expect(screen.getAllByRole("button", { name: "복원" })).toHaveLength(1);

  fireEvent.click(screen.getByRole("button", { name: "복원" }));
  expect(controller.change).not.toHaveBeenCalled();
  const dialog = screen.getByRole("alertdialog");
  fireEvent.click(within(dialog).getByRole("button", { name: "복원" }));
  expect(controller.change).toHaveBeenCalledWith(archived, "restore");
});
