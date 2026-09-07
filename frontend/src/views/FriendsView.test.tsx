import "../test/nativeDialog";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import FriendsView from "./FriendsView";
import type { SaveFriend } from "../types/generated/SaveFriend";

const api = vi.hoisted(() => ({ list: vi.fn(), save: vi.fn(), remove: vi.fn() }));
vi.mock("../api/friends", () => ({ fetchSavedFriends: api.list, saveFriend: api.save, deleteFriend: api.remove }));
afterEach(cleanup);
beforeEach(() => {
  vi.clearAllMocks();
  api.list.mockResolvedValue([]);
  api.remove.mockResolvedValue({ deleted: true });
  api.save.mockImplementation(async (request: SaveFriend) => ({ friend_id: request.friend_id, revision: request.expected_revision + 1, details: request.details, created_at: "2026-09-08T00:00:00Z", updated_at: "2026-09-08T00:00:00Z" }));
});

it("keeps a failed creation draft and its ID for retry, then cancels an edit without saving", async () => {
  api.save.mockRejectedValueOnce(new Error("저장 실패"));
  render(<FriendsView onClose={vi.fn()} />);
  const add = screen.getByRole("button", { name: "친구 추가" });
  await waitFor(() => expect((add as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(add);
  fireEvent.change(screen.getByLabelText("이름"), { target: { value: "새 친구" } });
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  await screen.findByText("저장 실패");
  expect((screen.getByLabelText("이름") as HTMLInputElement).value).toBe("새 친구");
  expect(screen.queryByRole("button", { name: "새 친구 편집" })).toBeNull();
  const original = api.save.mock.calls[0][0];
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  fireEvent.click(await screen.findByRole("button", { name: "새 친구 편집" }));
  expect(api.save.mock.calls[1][0]).toEqual(original);
  expect((screen.getByRole("button", { name: "저장" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("이름"), { target: { value: "취소할 수정" } });
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(api.save).toHaveBeenCalledTimes(2);
  expect(screen.getByText("새 친구")).toBeTruthy();
});

it("requires a successful initial read and keeps failed deletion pending until a confirmed retry", async () => {
  api.list.mockRejectedValueOnce(new Error("목록 읽기 실패"));
  render(<FriendsView onClose={vi.fn()} />);
  await screen.findByText("목록 읽기 실패");
  expect((screen.getByRole("button", { name: "친구 추가" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "새로고침" }));
  await waitFor(() => expect((screen.getByRole("button", { name: "친구 추가" }) as HTMLButtonElement).disabled).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "친구 추가" }));
  fireEvent.change(screen.getByLabelText("이름"), { target: { value: "지울 친구" } });
  fireEvent.click(screen.getByRole("button", { name: "저장" }));
  fireEvent.click(await screen.findByRole("button", { name: "지울 친구 삭제" }));
  expect(api.remove).not.toHaveBeenCalled();
  api.remove.mockRejectedValueOnce(new Error("삭제 실패"));
  let dialog = screen.getByRole("dialog", { name: "친구를 삭제할까요?" });
  fireEvent.click(within(dialog).getByRole("button", { name: "삭제" }));
  await screen.findByText("삭제 실패");
  expect(screen.getByText("지울 친구")).toBeTruthy();
  dialog = screen.getByRole("dialog", { name: "친구를 삭제할까요?" });
  fireEvent.click(within(dialog).getByRole("button", { name: "삭제" }));
  await waitFor(() => expect(screen.queryByText("지울 친구")).toBeNull());
});
