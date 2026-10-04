import "../../test/nativeDialog";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import OwnerDevicesPanel from "./OwnerDevicesPanel";

const mocks = vi.hoisted(() => ({ list: vi.fn(), revoke: vi.fn() }));
vi.mock("../../api/ownerDevices", () => ({ listOwnerDevices: mocks.list, revokeOwnerDevices: mocks.revoke }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const host = { session_id: "host", device_name: "Nel의 Mac", browser: "AgentsAssemble 앱", os: "macOS",
  last_connected_at: 1791090000, current: true, connected: true, revocable: false, kind: "host" };
const chrome = { session_id: "30000000-0000-4000-8000-000000000003", device_name: "Chrome · Windows", browser: "Chrome", os: "Windows",
  last_connected_at: 1791090000, current: false, connected: true, revocable: true, kind: "owner" };

it("shows identifiable devices and keeps the host recovery owner outside single/all revocation", async () => {
  mocks.list.mockResolvedValue({ sessions: [host, chrome] }); mocks.revoke.mockResolvedValue(undefined);
  const view = render(<OwnerDevicesPanel identity={{}} />);
  await screen.findByText("Nel의 Mac");
  expect(screen.getByText("이 기기")).toBeTruthy();
  expect(screen.getAllByText(/마지막 접속/)).toHaveLength(2);
  expect(screen.queryByRole("button", { name: "Nel의 Mac 연결 해제" })).toBeNull();
  view.rerender(<OwnerDevicesPanel identity={{}} />);
  expect(mocks.list).toHaveBeenCalledOnce();
  await userEvent.click(screen.getByRole("button", { name: "Chrome 연결 해제" }));
  const confirmation = screen.getByRole("dialog", { name: "기기 연결 해제 확인" });
  expect(within(confirmation).getByRole("button", { name: "취소" })).toBe(document.activeElement);
  await userEvent.click(within(confirmation).getByRole("button", { name: "연결 해제" }));
  await waitFor(() => expect(mocks.revoke).toHaveBeenCalledWith({ centralSession: undefined, deviceToken: undefined },
    { scope: "session", session_id: chrome.session_id }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await userEvent.click(screen.getByRole("button", { name: "모든 기기 연결 해제" }));
  await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "취소" }));
  expect(mocks.revoke).toHaveBeenCalledOnce();
});

it("exposes list and revocation failures and permits an explicit retry", async () => {
  mocks.list.mockRejectedValueOnce(new Error("호스트에 연결하지 못했어요")).mockResolvedValue({ sessions: [chrome] });
  mocks.revoke.mockRejectedValueOnce(new Error("해제를 저장하지 못했어요")).mockResolvedValueOnce(undefined);
  render(<OwnerDevicesPanel identity={{ centralSession: { sessionToken: "root", generation: 1 }, deviceToken: "device" }} />);
  expect((await screen.findByRole("alert")).textContent).toContain("호스트에 연결하지 못했어요");
  await userEvent.click(screen.getByRole("button", { name: "기기 목록 새로고침" }));
  await screen.findByRole("button", { name: "Chrome 연결 해제" });
  await userEvent.click(screen.getByRole("button", { name: "Chrome 연결 해제" }));
  await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "연결 해제" }));
  await waitFor(() => expect(within(screen.getByRole("dialog")).getByRole("alert").textContent).toContain("저장하지 못했어요"));
  await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "연결 해제" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(mocks.revoke).toHaveBeenCalledTimes(2);
});

it.each([["macos", "macOS"], ["windows", "Windows"], ["linux", "Linux"]])("formats %s and avoids a duplicated browser OS title", async (raw, label) => {
  mocks.list.mockResolvedValue({ sessions: [{ ...host, os: raw }, chrome] });
  render(<OwnerDevicesPanel identity={{}} />);
  await screen.findByText("Nel의 Mac");
  expect(screen.getAllByRole("listitem")[0].textContent).toContain(label);
  expect(screen.getByText("Chrome", { selector: "strong" })).toBeTruthy();
});
