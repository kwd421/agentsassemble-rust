import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { readRuntimeRestart, requestRuntimeRestart } from "../../api/runtimeRestart";
import RuntimeRestartPanel from "./RuntimeRestartPanel";

vi.mock("../../api/runtimeRestart", () => ({ readRuntimeRestart: vi.fn(), requestRuntimeRestart: vi.fn() }));
afterEach(() => { cleanup(); vi.useRealTimers(); vi.resetAllMocks(); });
const receipt = (operation_id: string, phase: "recovering" | "completed") => ({ operation_id, phase, updated_at: "2026-09-09T00:00:00Z" });

it("resolves a lost reply by the same operation ID and never treats acceptance as completion", async () => {
  vi.useFakeTimers();
  let operation = "";
  vi.mocked(readRuntimeRestart).mockResolvedValueOnce({ supported: true, operation: null })
    .mockImplementation(async () => ({ supported: true, operation: receipt(operation, "recovering") }));
  vi.mocked(requestRuntimeRestart).mockImplementation(async (id, dispatch) => {
    operation = id; dispatch(); throw new Error("reply lost");
  });
  await act(async () => { render(<RuntimeRestartPanel />); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "서버 재시작" })); });
  expect(screen.getByRole("status").textContent).toContain("세션을 복구");
  expect(screen.queryByText("재시작을 완료했어요.")).toBeNull();
  expect(readRuntimeRestart).toHaveBeenLastCalledWith(operation, expect.any(AbortSignal));
  vi.mocked(readRuntimeRestart).mockResolvedValue({ supported: true, operation: receipt(operation, "completed") });
  await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
  expect(screen.getByRole("status").textContent).toBe("재시작을 완료했어요.");
  const calls = vi.mocked(readRuntimeRestart).mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(readRuntimeRestart).toHaveBeenCalledTimes(calls);
});

it("stops uncertain observation at its deadline and retries the identical request", async () => {
  vi.useFakeTimers();
  vi.mocked(readRuntimeRestart).mockResolvedValue({ supported: true, operation: null });
  vi.mocked(requestRuntimeRestart).mockImplementation(async (_id, dispatch) => { dispatch(); throw new Error("unknown"); });
  await act(async () => { render(<RuntimeRestartPanel />); });
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "서버 재시작" })); });
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(screen.getByRole("alert").textContent).toContain("아직 결과를 확인하지 못했어요");
  expect(screen.queryByText("재시작을 완료했어요.")).toBeNull();
  const calls = vi.mocked(readRuntimeRestart).mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
  expect(readRuntimeRestart).toHaveBeenCalledTimes(calls);
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 보내기" })); });
  const requests = vi.mocked(requestRuntimeRestart).mock.calls;
  expect(requests).toHaveLength(2);
  expect(requests[1][0]).toBe(requests[0][0]);
});
