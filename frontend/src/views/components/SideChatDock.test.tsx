import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, expect, it, vi } from "vitest";
import SideChatDock from "./SideChatDock";
import { useRoomSideChat } from "../../app/useRoomSideChat";
import type { RoomSocketHandle } from "../../roomSocketTypes";
vi.mock("../../api/sideChat", () => ({ fetchSideChatSnapshot: vi.fn(async () => ({
  room_id: "general", room_uid: "uid", generation: "00000000-0000-4000-8000-000000000001", retained_after_seq: 0, latest_seq: 0, messages: [],
})) }));
afterEach(() => vi.clearAllMocks());

it("retains a rejected draft, confirms a successful send, and disables the read-only composer", async () => {
  const command = vi.fn().mockRejectedValueOnce(new Error("보낼 권한이 없어요."));
  const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
  function Fixture({ canPost }: { canPost: boolean }) {
    const chat = useRoomSideChat("general", { kind: "local" });
    useEffect(() => chat.connect("uid"), [chat.connect]);
    return <SideChatDock chat={chat} socket={socket} canPost={canPost} mentionables={[]} />;
  }
  const view = render(<Fixture canPost />);
  fireEvent.click(screen.getByRole("button", { name: "사이드챗 열기" }));
  const input = await screen.findByRole("textbox", { name: "비공식 사이드챗 입력" });
  await waitFor(() => expect((input as HTMLTextAreaElement).disabled).toBe(false));
  fireEvent.change(input, { target: { value: "keep my text" } });
  await act(async () => fireEvent.keyDown(input, { key: "Enter" }));
  expect((await screen.findByRole("alert")).textContent).toContain("보낼 권한이 없어요.");
  expect((input as HTMLTextAreaElement).value).toBe("keep my text");
  command.mockResolvedValueOnce({ result: { update: {
    room_id: "general", generation: "00000000-0000-4000-8000-000000000001", retained_after_seq: 0,
    message: { id: "00000000-0000-4000-8000-000000000002", seq: 1, created_at: "2026-09-08T00:00:00Z", participant_id: "operator-local", display_name: "Me", content: "keep my text" },
  } } });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "사이드챗 보내기" })));
  expect((input as HTMLTextAreaElement).value).toBe("");
  expect(document.activeElement).toBe(input);
  view.rerender(<Fixture canPost={false} />);
  expect((input as HTMLTextAreaElement).disabled).toBe(true);
  expect((screen.getByRole("button", { name: "사이드챗 보내기" }) as HTMLButtonElement).disabled).toBe(true);
  view.unmount();
});
