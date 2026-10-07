import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProviderRequestMessage from "./ProviderRequestMessage";
import { pendingRequest } from "../../test/providerRequest";
import { RoomSocketSayError, type RoomCommandAck, type RoomSocketHandle } from "../../roomSocketTypes";

afterEach(cleanup);

function socket(resolveProviderRequest: RoomSocketHandle["resolveProviderRequest"]): RoomSocketHandle {
  return { close: vi.fn(), ready: () => true, say: vi.fn(), command: vi.fn(), historyBefore: vi.fn(), resolveProviderRequest };
}
const ack: RoomCommandAck = { op: "ack", accepted: true, resolution: "committed", request_id: pendingRequest.request.provider_request_id, action: "provider.request.resolve", deduplicated: false };

describe("provider request controls", () => {
  it("masks a secret answer and only retries the same answer after uncertain delivery", async () => {
    const resolve = vi.fn().mockRejectedValueOnce(new RoomSocketSayError("Unknown", "outcome_unknown")).mockResolvedValueOnce(ack);
    const { rerender } = render(<ProviderRequestMessage entry={pendingRequest} socket={socket(resolve)} connected canPost />);
    const input = screen.getByLabelText("답변") as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "synthetic-secret" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "응답 보내기" })); });
    expect(screen.getByRole("alert").textContent).toContain("같은 응답");
    expect(input.closest("fieldset")?.parentElement?.closest("fieldset")?.disabled).toBe(true);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "같은 응답으로 다시 확인" })); });
    expect(resolve.mock.calls).toEqual([
      [pendingRequest.request.provider_request_id, { response_kind: "answers", answers: { secret: ["synthetic-secret"] } }],
      [pendingRequest.request.provider_request_id, { response_kind: "answers", answers: { secret: ["synthetic-secret"] } }],
    ]);
    expect(input.value).toBe("");
    expect(screen.getByRole("status").textContent).toContain("결과를 기다리고");
    rerender(<ProviderRequestMessage entry={{ ...pendingRequest, state: "resolving" }} socket={socket(resolve)} connected canPost />);
    expect(screen.queryByRole("button", { name: "응답 보내기" })).toBeNull();
  });

  it("keeps in-message requests read-only while disconnected, without a separate opener or modal", () => {
    render(<ProviderRequestMessage entry={pendingRequest} socket={socket(vi.fn())} connected={false} canPost />);
    expect((screen.getByRole("button", { name: "응답 보내기" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.queryByRole("button", { name: /에이전트 요청/ })).toBeNull();
  });
  it("replaces only this request's form with its result", () => {
    const { rerender } = render(<ProviderRequestMessage entry={pendingRequest} socket={socket(vi.fn())} connected canPost />);
    rerender(<ProviderRequestMessage title={pendingRequest.request.title} state="resolved" socket={socket(vi.fn())} connected canPost />);
    expect(screen.getByRole("status").textContent).toBe("응답을 전달했어요.");
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByLabelText("답변")).toBeNull();
  });
});

it("hides pre-upgrade OpenCode remembered grants and submits only a one-time choice", async () => {
  const resolve = vi.fn().mockResolvedValue(ack);
  const entry = { ...pendingRequest, request: { ...pendingRequest.request,
    request_kind: "permission" as const,
    prompt: { response_kind: "option" as const, options: [
      { id: "once", kind: "allow_once", label: "Allow once", description: "" },
      { id: "always", kind: "allow_always", label: "Always allow", description: "" },
      { id: "reject", kind: "reject_once", label: "Reject", description: "" },
    ] },
  } };
  render(<ProviderRequestMessage entry={entry} providerKind="opencode_server" socket={socket(resolve)} connected canPost />);
  expect(screen.queryByLabelText("Always allow")).toBeNull();
  fireEvent.click(screen.getByLabelText("Allow once"));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "응답 보내기" })); });
  expect(resolve).toHaveBeenCalledWith(entry.request.provider_request_id, { response_kind: "option", option_id: "once" });
});
