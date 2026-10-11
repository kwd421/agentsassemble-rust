import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import AppUpdateIndicator from "./AppUpdateIndicator";

const native = vi.hoisted(() => ({ bundled: true, invoke: vi.fn(), listen: vi.fn(), stop: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isBundledDesktopWebview: () => native.bundled }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: native.listen }));
let emit: (payload: unknown) => void;
beforeEach(() => {
  native.bundled = true;
  native.listen.mockImplementation(async (_name, callback) => {
    emit = (payload) => callback({ payload });
    return native.stop;
  });
  native.invoke.mockResolvedValue({ status: "none" });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("hides web and unverified downloads, and reads already-ready state on mount", async () => {
  native.bundled = false;
  const web = render(<AppUpdateIndicator />);
  expect(native.listen).not.toHaveBeenCalled();
  expect(native.invoke).not.toHaveBeenCalled();
  web.unmount();
  native.bundled = true;
  native.invoke.mockResolvedValue({ status: "available", version: "0.1.5-test.2.1" });
  const desktop = render(<AppUpdateIndicator />);
  await waitFor(() => expect(native.invoke).toHaveBeenCalled());
  expect(screen.queryByRole("button")).toBeNull();
  desktop.unmount();
  native.invoke.mockResolvedValue({ status: "ready", version: "0.1.5-test.2.1" });
  render(<AppUpdateIndicator />);
  expect(await screen.findByRole("button", { name: "새 버전 0.1.5-test.2.1" })).toBeTruthy();
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("keeps a ready event when the initial snapshot resolves late, dismisses without installing, and disables installation", async () => {
  let resolveSnapshot!: (value: unknown) => void;
  native.invoke.mockImplementationOnce(() => new Promise(resolve => { resolveSnapshot = resolve; }));
  const view = render(<AppUpdateIndicator />);
  await waitFor(() => expect(native.invoke).toHaveBeenCalledWith("app_update_state"));
  await act(async () => {
    emit({ status: "ready", version: "0.1.5-test.3.1" });
    resolveSnapshot({ status: "none" });
  });
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "새 버전 0.1.5-test.3.1" }));
  await user.click(screen.getByRole("button", { name: "나중에" }));
  expect(native.invoke).not.toHaveBeenCalledWith("app_update_install");
  await user.click(screen.getByRole("button", { name: "새 버전 0.1.5-test.3.1" }));
  native.invoke.mockImplementationOnce(async () => { emit({ status: "installing", version: "0.1.5-test.3.1" }); });
  await user.click(screen.getByRole("button", { name: "다시 시작해서 업데이트" }));
  expect(native.invoke).toHaveBeenCalledWith("app_update_install");
  expect((screen.getByRole("button", { name: "업데이트하는 중…" }) as HTMLButtonElement).disabled).toBe(true);
  view.unmount();
  expect(native.stop).toHaveBeenCalledOnce();
});

it("shows install rejection and permits retry", async () => {
  native.invoke.mockResolvedValueOnce({ status: "ready", version: "1.0.0" });
  render(<AppUpdateIndicator />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "새 버전 1.0.0" }));
  native.invoke.mockRejectedValueOnce(new Error("busy"));
  await user.click(screen.getByRole("button", { name: "다시 시작해서 업데이트" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect((screen.getByRole("button", { name: "다시 시작해서 업데이트" }) as HTMLButtonElement).disabled).toBe(false);
});
