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
    request_kind: "permission" as const, title: "OpenCode requests bash",
    prompt: { response_kind: "option" as const, options: [
      { id: "once", kind: "allow_once", label: "Allow once", description: "" },
      { id: "always", kind: "allow_always", label: "Always allow", description: "" },
      { id: "reject", kind: "reject_once", label: "Reject", description: "" },
    ] },
  } };
  render(<ProviderRequestMessage entry={entry} providerKind="opencode_server" socket={socket(resolve)} connected canPost />);
  // Labels come from the structured kind; the provider's English label and title never render.
  expect(screen.queryByText(/Allow once/)).toBeNull();
  expect(screen.queryByText(/OpenCode requests bash/)).toBeNull();
  expect(screen.getByText(/OpenCode 실행 권한 요청/)).toBeTruthy();
  // One row of buttons, one click answers; OpenCode's remembered grant stays hidden.
  expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["이번만 허용", "거부"]);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "이번만 허용" })); });
  expect(resolve).toHaveBeenCalledWith(entry.request.provider_request_id, { response_kind: "option", option_id: "once" });
});

it("renders a neutral Korean label for an option kind it does not know", () => {
  const entry = { ...pendingRequest, request: { ...pendingRequest.request,
    request_kind: "permission" as const,
    prompt: { response_kind: "option" as const, options: [
      { id: "future", kind: "allow_from_orbit", label: "Allow from orbit", description: "" },
    ] },
  } };
  render(<ProviderRequestMessage entry={entry} providerKind="codex_live_session" socket={socket(vi.fn())} connected canPost />);
  expect(screen.queryByText(/Allow from orbit/)).toBeNull();
  expect(screen.getByRole("button", { name: "선택" })).toBeTruthy();
});

it("offers 앞으로도 허용 from the built-in harness and shows time left, not a deadline", () => {
  const entry = { ...pendingRequest, expires_at: new Date(Date.now() + 9.5 * 60_000).toISOString(),
    request: { ...pendingRequest.request, request_kind: "permission" as const,
      prompt: { response_kind: "option" as const, options: [
        { id: "allow_once", kind: "allow_once", label: "이번만 허용", description: "" },
        { id: "allow_always", kind: "allow_always", label: "앞으로도 허용", description: "이 에이전트가 멈출 때까지 같은 명령은 묻지 않아요." },
        { id: "deny", kind: "reject_once", label: "거부", description: "" },
      ] } } };
  render(<ProviderRequestMessage entry={entry} providerKind="deepseek" socket={socket(vi.fn())} connected canPost />);
  expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["이번만 허용", "앞으로도 허용", "거부"]);
  expect(screen.getByText("10분 남음")).toBeTruthy();
  expect(screen.queryByText(/응답 기한/)).toBeNull();
});
