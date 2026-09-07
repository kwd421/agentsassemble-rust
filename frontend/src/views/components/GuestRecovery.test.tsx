import { act, cleanup, fireEvent, render, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import GuestRecoverySettings from "./GuestRecoverySettings";
import GuestIdentityRecoveryPanel from "./GuestIdentityRecoveryPanel";

const api = vi.hoisted(() => ({ issue: vi.fn(), redeem: vi.fn() }));
vi.mock("../../api", () => ({
  issueGuestRecoveryCode: api.issue,
  redeemGuestRecoveryCode: api.redeem,
}));
afterEach(cleanup);
beforeEach(() => { api.issue.mockReset(); api.redeem.mockReset(); });

it("retires issued codes and pending issuance when the account device changes", async () => {
  let resolve: (value: unknown) => void = () => {};
  api.issue.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  api.issue.mockResolvedValueOnce({ recovery_code: "current-code", recovery_url: "current-url" });
  const view = render(<GuestRecoverySettings identity={{ sessionToken: "session-a", deviceToken: "device-a" }} />);
  fireEvent.click(within(view.container).getByRole("button", { name: "복구 코드 만들기" }));
  expect(api.issue).toHaveBeenCalledTimes(1);
  view.rerender(<GuestRecoverySettings identity={{ sessionToken: "session-b", deviceToken: "device-b" }} />);
  await act(async () => { resolve({ recovery_code: "retired-code", recovery_url: "retired-url" }); });
  expect(within(view.container).queryByDisplayValue("retired-code")).toBeNull();
  fireEvent.click(within(view.container).getByRole("button", { name: "복구 코드 만들기" }));
  await waitFor(() => expect(within(view.container).getByDisplayValue("current-code")).toBeTruthy());
  api.issue.mockRejectedValueOnce(new Error("응답을 확인하지 못했어요. 다시 시도해 주세요."));
  fireEvent.click(within(view.container).getByRole("button", { name: "새 코드로 교체" }));
  await waitFor(() => expect(within(view.container).getByText(/응답을 확인하지 못했어요/)).toBeTruthy());
  expect(within(view.container).queryByDisplayValue("current-code")).toBeNull();
});

it("retries an uncertain recovery with the same code and retires late results after a device change", async () => {
  api.redeem.mockRejectedValueOnce(new Error("응답 없음"));
  let resolve: (value: unknown) => void = () => {};
  api.redeem.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
  const accept = vi.fn();
  const request = { recoveryCode: "opaque-Code", roomId: "general" };
  const view = render(<GuestIdentityRecoveryPanel deviceToken="device-a" clientId="client-a" request={request} onRecovered={accept} />);
  fireEvent.click(within(view.container).getByRole("button", { name: "신원 복구" }));
  await waitFor(() => expect(within(view.container).getByText("응답 없음")).toBeTruthy());
  fireEvent.click(within(view.container).getByRole("button", { name: "신원 복구" }));
  expect(api.redeem.mock.calls[0]).toEqual(api.redeem.mock.calls[1]);
  expect((within(view.container).getByDisplayValue("opaque-Code") as HTMLInputElement).readOnly).toBe(true);
  view.rerender(<GuestIdentityRecoveryPanel deviceToken="device-b" clientId="client-a" request={request} onRecovered={accept} />);
  await act(async () => { resolve({ display_name: "Retired", recovery_code: "replacement" }); });
  expect(within(view.container).queryByText("방으로 계속")).toBeNull();
  expect(within(view.container).getByDisplayValue("opaque-Code")).toBeTruthy();
  expect(accept).not.toHaveBeenCalled();
});
