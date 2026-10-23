import "../../test/nativeDialog";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import LeaveRoomDialog from "./LeaveRoomDialog";

describe("LeaveRoomDialog", () => {
  it("explains that owned agents leave and confirms the action", async () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn().mockResolvedValue(undefined);

    render(
      <LeaveRoomDialog
        roomLabel="Pinebrook"
        onClose={onClose}
        onConfirm={onConfirm}
      />
    );

    expect(
      screen.getByText(
        "내 에이전트도 함께 나가고, 실행 중인 에이전트는 멈춰요."
      )
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "방 나가기" }));

    await waitFor(() => expect(onConfirm).toHaveBeenCalledOnce());
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("keeps the dialog open and reports a rejected leave", async () => {
    const onClose = vi.fn();
    const onConfirm = vi.fn().mockRejectedValue(new Error("퇴장 요청 실패"));

    render(
      <LeaveRoomDialog
        roomLabel="Pinebrook"
        onClose={onClose}
        onConfirm={onConfirm}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "방 나가기" }));

    expect((await screen.findByRole("alert")).textContent).toContain("퇴장 요청 실패");
    expect(onClose).not.toHaveBeenCalled();
  });
});
