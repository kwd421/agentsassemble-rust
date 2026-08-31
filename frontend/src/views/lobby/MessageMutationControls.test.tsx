import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { LobbyEvent } from "../../api";
import MessageMutationControls from "./MessageMutationControls";

const message: LobbyEvent = {
  id: "message-transition",
  record_id: "message-record",
  kind: "message",
  name: "호스트",
  message: "원래 메시지",
  side: "mine",
  created_at: "2026-08-31T00:00:00Z",
  actor_id: "operator-local",
  actor_type: "human",
  flow_meeting_id: "room-a",
  flow_action: "message_final",
};

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("MessageMutationControls", () => {
  it("renders no control without an exact mutation authority", () => {
    render(
      <MessageMutationControls
        event={message}
        canEdit={false}
        canDelete={false}
        onEdit={vi.fn()}
        onDelete={vi.fn()}
      />
    );

    expect(screen.queryByRole("button", { name: "메시지 메뉴" })).toBeNull();
  });

  it("keeps a failed edit in its dialog and closes only after success", async () => {
    const onEdit = vi.fn()
      .mockRejectedValueOnce(new Error("서버가 수정을 거부했습니다."))
      .mockResolvedValueOnce(undefined);
    render(
      <MessageMutationControls
        event={message}
        canEdit
        canDelete={false}
        onEdit={onEdit}
        onDelete={vi.fn()}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "메시지 메뉴" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "수정" }));
    const dialog = screen.getByRole("dialog", { name: "메시지 수정하기" });
    const draft = within(dialog).getByRole("textbox");
    expect((draft as HTMLTextAreaElement).value).toBe("원래 메시지");

    fireEvent.change(draft, { target: { value: "   " } });
    expect(
      (within(dialog).getByRole("button", { name: "저장" }) as HTMLButtonElement).disabled
    ).toBe(true);
    fireEvent.change(draft, { target: { value: "수정된 메시지" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "저장" }));

    expect((await within(dialog).findByRole("alert")).textContent).toContain(
      "서버가 수정을 거부했습니다."
    );
    expect(onEdit).toHaveBeenCalledWith("수정된 메시지");
    fireEvent.click(within(dialog).getByRole("button", { name: "저장" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "메시지 수정하기" })).toBeNull()
    );
    expect(onEdit).toHaveBeenCalledTimes(2);
  });

  it("confirms an ordinary delete and preserves the explicit Shift bypass", async () => {
    const onDelete = vi.fn().mockResolvedValue(undefined);
    render(
      <MessageMutationControls
        event={message}
        canEdit={false}
        canDelete
        onEdit={vi.fn()}
        onDelete={onDelete}
      />
    );

    fireEvent.click(screen.getByRole("button", { name: "메시지 메뉴" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }));
    const dialog = screen.getByRole("dialog", { name: "메시지 삭제하기" });
    expect(onDelete).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "삭제" }));
    await waitFor(() => expect(onDelete).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "메시지 삭제하기" })).toBeNull()
    );

    fireEvent.click(screen.getByRole("button", { name: "메시지 메뉴" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "삭제" }), { shiftKey: true });
    await waitFor(() => expect(onDelete).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("dialog", { name: "메시지 삭제하기" })).toBeNull();
  });
});
