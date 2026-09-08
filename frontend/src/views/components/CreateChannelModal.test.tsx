import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import CreateChannelModal from "./CreateChannelModal";

afterEach(cleanup);
describe("CreateChannelModal", () => {
  it("keeps failed drafts, blocks dismissal while saving, and closes only after a receipt", async () => {
    let reject!: (error: Error) => void;
    const onCreate = vi.fn().mockImplementationOnce(() => new Promise<void>((_, fail) => { reject = fail; })).mockResolvedValueOnce(undefined);
    const onClose = vi.fn();
    render(<CreateChannelModal onCreate={onCreate} onClose={onClose} />);
    const input = screen.getByLabelText("채널 이름") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "구현 토론" } });
    fireEvent.click(screen.getByRole("button", { name: "만들기" }));
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { bubbles: true, cancelable: true }));
    expect(onClose).not.toHaveBeenCalled();
    expect((screen.getByRole("button", { name: "취소" }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => reject(new Error("설정이 바뀌었어요.")));
    expect(screen.getByRole("alert").textContent).toContain("설정이 바뀌었어요.");
    expect(input.value).toBe("구현 토론");
    fireEvent.click(screen.getByRole("button", { name: "만들기" }));
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(onCreate).toHaveBeenLastCalledWith({ name: "구현 토론", type: "text" });
  });

  it("ignores composing Enter and does not close a replacement after an old receipt", async () => {
    let resolve!: () => void;
    const onCreate = vi.fn(() => new Promise<void>((done) => { resolve = done; }));
    const onClose = vi.fn();
    const view = render(<CreateChannelModal onCreate={onCreate} onClose={onClose} />);
    const input = screen.getByLabelText("채널 이름");
    fireEvent.change(input, { target: { value: "새 채널" } });
    fireEvent.keyDown(input, { key: "Enter", isComposing: true });
    expect(onCreate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "만들기" }));
    view.unmount();
    await act(async () => resolve());
    expect(onClose).not.toHaveBeenCalled();
  });
});
