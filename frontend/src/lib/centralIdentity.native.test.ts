import { afterEach, expect, it, vi } from "vitest";
import { loginCentralGoogle } from "./centralIdentity";

const { control } = vi.hoisted(() => ({ control: vi.fn() }));
vi.mock("./desktopBridge", () => ({ controlDesktopCentralLogin: control }));
afterEach(() => { vi.restoreAllMocks(); control.mockReset(); });

it("retires the native return if cancellation arrives while start is in flight", async () => {
  const controller = new AbortController();
  control.mockImplementation(async (action: string) => {
    if (action === "start") {
      controller.abort();
      return { redirect_uri: "http://127.0.0.1:43210/api/central-login/callback", result: { status: "pending", expires_at: 9_999_999_999 } };
    }
    return { result: { status: "cancelled" } };
  });
  await expect(loginCentralGoogle(undefined, controller.signal)).rejects.toMatchObject({ name: "AbortError" });
  expect(control.mock.calls.map(([action]) => action)).toEqual(["start", "cancel"]);
  expect(control.mock.calls[1][1]).toBe(control.mock.calls[0][1]);
});

it("attempts retirement after an uncertain native start and exposes the failure", async () => {
  control.mockRejectedValueOnce(new Error("native start failed")).mockResolvedValueOnce({ result: { status: "cancelled" } });
  await expect(loginCentralGoogle()).rejects.toThrow("native start failed");
  expect(control.mock.calls.map(([action]) => action)).toEqual(["start", "cancel"]);
});
