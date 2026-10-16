import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { createDelegatedActionProjection, registerDelegatedActionProjection } from "../../lib/delegatedActionProjection";
import type { RoomSocketHandle } from "../../roomSocketTypes";
import type { DelegatedActionRequest } from "../../types/generated/DelegatedActionRequest";
import PendingDelegatedActions from "./PendingDelegatedActions";

afterEach(() => cleanup());

function request(owner: string, id: string): DelegatedActionRequest {
  return {
    confirmation_id: id, owner_participant_id: owner, owner_display_name: "Owner",
    delegate_participant_id: "agent-1", delegate_display_name: "코덱스",
    action: { action: "delete_text_channel", request_id: "r", channel_id: "c", expected_settings_revision: "s" },
    target_name: "기획", consequences: "host text", created_at: new Date().toISOString(),
    expires_at: new Date(Date.now() + 300_000).toISOString(), state: "pending",
  } as DelegatedActionRequest;
}

it("shows only the viewer's own pending requests and sends the exact decision", async () => {
  const command = vi.fn().mockResolvedValue({ result: { confirmation_id: "mine", state: "succeeded", receipt: {} } });
  const socket = { command } as unknown as RoomSocketHandle;
  const store = createDelegatedActionProjection();
  registerDelegatedActionProjection(socket, store);
  store.replace([request("me", "mine"), request("someone-else", "theirs")]);
  render(<PendingDelegatedActions socket={socket} viewerParticipantId="me" />);

  expect(screen.getAllByRole("region")).toHaveLength(1);
  expect(screen.getByText("#기획 채널을 지우려고 해요")).toBeTruthy();
  expect(screen.queryByText("host text")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "허용" }));
  await waitFor(() => expect(command).toHaveBeenCalledWith("delegated_action.decide", { confirmation_id: "mine", decision: "approve" }));
});

it("keeps the request and says why when the decision is refused", async () => {
  const { RoomSocketSayError } = await import("../../roomSocketTypes");
  const command = vi.fn().mockRejectedValue(new RoomSocketSayError("changed", "target_changed"));
  const socket = { command } as unknown as RoomSocketHandle;
  const store = createDelegatedActionProjection();
  registerDelegatedActionProjection(socket, store);
  store.replace([request("me", "mine")]);
  render(<PendingDelegatedActions socket={socket} viewerParticipantId="me" />);

  fireEvent.click(screen.getByRole("button", { name: "거절" }));
  expect(await screen.findByText("그 사이 내용이 바뀌어서 실행하지 않았어요.")).toBeTruthy();
  expect(command).toHaveBeenCalledWith("delegated_action.decide", { confirmation_id: "mine", decision: "decline" });
});

it("shows a minted invite link only from the approval acknowledgement, until closed", async () => {
  const invite = {
    invite_id: "inv", invite_token: "token", join_code: "code", join_url: "https://example.test/join?token=code",
    display_name: "Guest", invite_scope: "read_write", max_uses: 1, expires_at: new Date(Date.now() + 86_400_000).toISOString(),
  };
  const command = vi.fn().mockResolvedValue({ result: { confirmation_id: "mine", state: "succeeded", receipt: {}, invite } });
  const socket = { command } as unknown as RoomSocketHandle;
  const store = createDelegatedActionProjection();
  registerDelegatedActionProjection(socket, store);
  store.replace([{
    ...request("me", "mine"),
    action: { action: "create_human_invite", request_id: "r", params: { request_id: "r", invite_scope: "read_write", ttl_seconds: 86_400, max_uses: 1 } },
  } as DelegatedActionRequest]);
  render(<PendingDelegatedActions socket={socket} viewerParticipantId="me" />);

  expect(screen.queryByDisplayValue(invite.join_url)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "허용" }));
  expect(await screen.findByDisplayValue(invite.join_url)).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "닫기" }));
  expect(screen.queryByDisplayValue(invite.join_url)).toBeNull();
});
