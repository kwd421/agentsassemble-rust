import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { AttendeeFriendInviteCard } from "./AttendeeFriendInviteCard";
import type { useFriendsDirectory } from "../../app/useFriendsDirectory";

afterEach(cleanup);
it("selects only AI contacts and requires their provider before requesting an invitation", () => {
  const directory = { loaded: true, busy: false, error: "", reload: vi.fn(), save: vi.fn(), remove: vi.fn(), friends: [
    { friend_id: "human", details: { participant_type: "human", display_name: "Human", provider_kind: "" } },
    { friend_id: "missing", details: { participant_type: "remote", display_name: "Missing", provider_kind: "" } },
    { friend_id: "ai", details: { participant_type: "remote", display_name: "AI", provider_kind: "codex" } },
  ] } as unknown as ReturnType<typeof useFriendsDirectory>;
  const controls = { invites: [], creating: false, create: vi.fn(), copy: vi.fn() };
  render(<AttendeeFriendInviteCard directory={directory} controls={controls} disabled={false} />);
  expect(screen.queryByRole("option", { name: /Human/ })).toBeNull();
  const select = screen.getByLabelText("저장된 AI 친구");
  const button = screen.getByRole("button", { name: "AI 친구 초대 만들기" }) as HTMLButtonElement;
  fireEvent.change(select, { target: { value: "missing" } });
  expect(button.disabled).toBe(true);
  fireEvent.change(select, { target: { value: "ai" } });
  fireEvent.click(button);
  expect(controls.create).toHaveBeenCalledWith("ai");
});
