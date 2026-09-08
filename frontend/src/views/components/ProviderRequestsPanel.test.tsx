import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProviderRequestsPanel from "./ProviderRequestsPanel";
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
    const { rerender } = render(<ProviderRequestsPanel requests={[pendingRequest]} socket={socket(resolve)} connected canPost events={[]} />);
    fireEvent.click(screen.getByRole("button", { name: "에이전트 요청 (1)" }));
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
    rerender(<ProviderRequestsPanel requests={[{ ...pendingRequest, state: "resolving" }]} socket={socket(resolve)} connected canPost events={[]} />);
    expect(screen.queryByRole("button", { name: "응답 보내기" })).toBeNull();
  });

  it("keeps requests read-only while disconnected and restores focus on close", () => {
    render(<ProviderRequestsPanel requests={[pendingRequest]} socket={socket(vi.fn())} connected={false} canPost events={[]} />);
    const opener = screen.getByRole("button", { name: "에이전트 요청 (1)" });
    fireEvent.click(opener);
    expect((screen.getByRole("button", { name: "응답 보내기" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "닫기" }));
    expect(document.activeElement).toBe(opener);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
