import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import AppUpdateCheck from "./AppUpdateCheck";

const native = vi.hoisted(() => ({ bundled: true, invoke: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isBundledDesktopWebview: () => native.bundled }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
afterEach(() => { cleanup(); vi.resetAllMocks(); native.bundled = true; });

it("does not expose checks or invoke native commands on the web", () => {
  native.bundled = false;
  render(<AppUpdateCheck />);
  expect(screen.queryByRole("button")).toBeNull();
  expect(native.invoke).not.toHaveBeenCalled();
});

it("waits for the native result, prevents duplicate clicks and shows the returned version", async () => {
  let finish!: (value: string | null) => void;
  native.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  render(<AppUpdateCheck />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "업데이트 확인" }));
  expect(native.invoke).toHaveBeenCalledWith("app_update_check");
  expect(screen.getByRole("status").textContent).toBe("");
  const pending = screen.getByRole("button", { name: "확인하는 중…" });
  expect((pending as HTMLButtonElement).disabled).toBe(true);
  await user.click(pending);
  expect(native.invoke).toHaveBeenCalledTimes(1);
  await act(async () => finish("0.1.5-test.12.1"));
  expect(screen.getByRole("status").textContent).toBe("새 버전 0.1.5-test.12.1");
});

it("shows failure inline and lets a retry report no update", async () => {
  native.invoke.mockRejectedValueOnce("네트워크 연결을 확인해 주세요.").mockResolvedValueOnce(null);
  render(<AppUpdateCheck />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "업데이트 확인" }));
  expect(screen.getByRole("alert").textContent).toBe("네트워크 연결을 확인해 주세요.");
  expect(screen.getByRole("status").textContent).toBe("");
  await user.click(screen.getByRole("button", { name: "업데이트 확인" }));
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.getByRole("status").textContent).toBe("최신 버전이에요");
});
