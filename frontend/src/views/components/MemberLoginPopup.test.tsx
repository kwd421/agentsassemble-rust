import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { StrictMode } from "react";
const mocks = vi.hoisted(() => ({ run: vi.fn(), register: vi.fn(), finish: vi.fn() }));
vi.mock("../../lib/central/memberPopup", () => ({ runMemberLoginPopup: mocks.run, registerMemberLoginPopup: mocks.register, finishMemberLoginPopup: mocks.finish }));
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import MemberLoginPopup from "./MemberLoginPopup";
beforeEach(() => { vi.clearAllMocks(); window.history.replaceState({}, "", "/?state=fixture"); });
afterEach(cleanup);
it("renders callback failure as the shared card with a Korean return/retry action, once in StrictMode", async () => {
  mocks.run.mockRejectedValue(new Error("private internal error"));
  const view = render(<StrictMode><MemberLoginPopup /></StrictMode>);
  await screen.findByText(/로그인을 마치지 못했어요/);
  expect(screen.getByRole("heading", { name: "Google 로그인" }).closest("section")?.classList.contains("dc-guest-join-card")).toBe(true);
  expect((screen.getByRole("button", { name: "초대 창으로 돌아가서 다시 시도" }) as HTMLButtonElement).disabled).toBe(false);
  expect(mocks.run).toHaveBeenCalledOnce(); expect(screen.queryByText("private internal error")).toBeNull();
  const signal = mocks.run.mock.calls[0][0] as AbortSignal;
  view.unmount(); expect(signal.aborted).toBe(true);
});
it.each(["absent", "deleted"] as const)("%s does not register until the separate signup click", async status => {
  const registration = new GoogleRegistrationRequired(status, "web", { handoff_id: "goh_fixture", code_verifier: "v".repeat(43), authorization_code: "fixture" }, Date.now() / 1000 + 300, null);
  mocks.run.mockRejectedValue(registration); mocks.register.mockResolvedValue(undefined);
  render(<MemberLoginPopup />);
  await screen.findByText(registration.message);
  expect(mocks.register).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /^새로 가입$/ }));
  await waitFor(() => expect(mocks.register).toHaveBeenCalledOnce());
  await screen.findByText("가입하고 로그인했어요. 초대 창으로 돌아가 주세요.");
});

it("ends the opener wait when the explicit registration prompt unmounts or closes", async () => {
  const registration = new GoogleRegistrationRequired("absent", "web", { handoff_id: "goh_fixture", code_verifier: "v".repeat(43), authorization_code: "fixture" }, Date.now() / 1000 + 300, null);
  mocks.run.mockRejectedValue(registration);
  const view = render(<MemberLoginPopup />);
  await screen.findByText(registration.message);
  window.dispatchEvent(new PageTransitionEvent("pagehide"));
  view.unmount(); expect(mocks.finish).toHaveBeenCalledExactlyOnceWith(false);
  expect((mocks.run.mock.calls[0][0] as AbortSignal).aborted).toBe(true);
});
it("keeps return disabled while registration is pending; reports the committed login-required result accurately", async () => {
  const registration = new GoogleRegistrationRequired("absent", "web", { handoff_id: "goh_fixture", code_verifier: "v".repeat(43), authorization_code: "fixture" }, Date.now() / 1000 + 300, null);
  mocks.run.mockRejectedValue(registration);
  let reject!: (error: Error) => void;
  mocks.register.mockImplementation(() => new Promise<void>((_, fail) => { reject = fail; }));
  render(<MemberLoginPopup />); await screen.findByText(registration.message);
  fireEvent.click(screen.getByRole("button", { name: /^새로 가입$/ }));
  const back = screen.getByRole("button", { name: "초대 창으로 돌아가서 다시 시도" }) as HTMLButtonElement;
  expect(back.disabled).toBe(true); fireEvent.click(back); expect(mocks.finish).not.toHaveBeenCalled();
  reject(new Error("새 계정을 만들었어요. Google로 다시 로그인해 주세요."));
  await screen.findByText("새 계정을 만들었어요. Google로 다시 로그인해 주세요.");
  expect(back.disabled).toBe(false);
});
it.each([
  "확인 시간이 만료됐어요. Google 계정을 다시 확인해 주세요.",
  "로그인 계정이 바뀌었어요. Google 계정을 다시 확인해 주세요.",
])("preserves the explicit registration outcome: %s", async message => {
  const registration = new GoogleRegistrationRequired("absent", "web", { handoff_id: "goh_fixture", code_verifier: "v".repeat(43), authorization_code: "fixture" }, Date.now() / 1000 + 300, null);
  mocks.run.mockRejectedValue(registration); mocks.register.mockRejectedValue(new Error(message));
  render(<MemberLoginPopup />); await screen.findByText(registration.message);
  fireEvent.click(screen.getByRole("button", { name: /^새로 가입$/ }));
  await screen.findByText(message);
});
