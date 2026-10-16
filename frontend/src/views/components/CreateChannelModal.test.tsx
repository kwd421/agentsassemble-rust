import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import CreateChannelModal from "./CreateChannelModal";

function fastClick(element: HTMLElement) {
  fireEvent.pointerDown(element);
  fireEvent.pointerUp(element);
  fireEvent.click(element);
}

afterEach(cleanup);
describe("CreateChannelModal", () => {
  it("answers a fast click with an empty name instead of ignoring it", () => {
    const onCreate = vi.fn(async () => true);
    const onClose = vi.fn();
    render(<CreateChannelModal onCreate={onCreate} onClose={onClose} />);
    fastClick(screen.getByRole("button", { name: "만들기" }));
    expect(screen.getByRole("alert").textContent).toContain("채널 이름을 입력해 주세요.");
    expect(onCreate).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces an unconfirmed creation instead of resetting to idle", async () => {
    const onCreate = vi.fn(async () => false);
    const onClose = vi.fn();
    render(<CreateChannelModal room initialName="새 방" onCreate={onCreate} onClose={onClose} />);
    const input = screen.getByLabelText("방 이름") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "새 방" } });
    fastClick(screen.getByRole("button", { name: "만들기" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("방을 만들지 못했어요."));
    expect(onClose).not.toHaveBeenCalled();
    expect(input.disabled).toBe(false);
  });

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


it("asks for a room name before creation, cancels without a request and retains an unconfirmed draft", async () => {
  const onCreate = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(true);
  const onClose = vi.fn();
  const view = render(<CreateChannelModal room initialName="새 방" onCreate={onCreate} onClose={onClose} />);
  expect(onCreate).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(onCreate).not.toHaveBeenCalled();
  view.unmount(); onClose.mockClear();
  render(<CreateChannelModal room initialName="새 방" onCreate={onCreate} onClose={onClose} />);
  const input = screen.getByLabelText("방 이름") as HTMLInputElement;
  fireEvent.change(input, { target: { value: "  디자인 검토  " } });
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(input.disabled).toBe(false));
  expect(onClose).not.toHaveBeenCalled();
  expect(input.value).toBe("  디자인 검토  ");
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(onCreate.mock.calls.map(([value]) => value.name)).toEqual(["디자인 검토", "디자인 검토"]);
});
