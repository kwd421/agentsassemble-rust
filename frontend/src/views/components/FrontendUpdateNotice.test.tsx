import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FrontendUpdateNotice from "./FrontendUpdateNotice";
import { PROTOCOL_VERSION } from "../../types/generated/PROTOCOL_VERSION";

const build = "a".repeat(64);
const response = (frontend_build_id: string | null, protocol_version = Number(PROTOCOL_VERSION)) =>
  new Response(JSON.stringify({ frontend_build_id, protocol_version }), { status: 200 });

beforeEach(() => {
  vi.useFakeTimers();
  document.documentElement.dataset.agentsassembleBuild = build;
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  delete document.documentElement.dataset.agentsassembleBuild;
});

describe("served frontend updates", () => {
  it("compares the first response with the loaded document and stops after detecting an update", async () => {
    const fetcher = vi.fn().mockResolvedValue(response("b".repeat(64)));
    vi.stubGlobal("fetch", fetcher);
    await act(async () => { render(<FrontendUpdateNotice connected />); });
    expect(screen.getByText("새 화면 버전이 준비됐어요.")).toBeTruthy();
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it("exposes failed observations and clears the error only after an explicit successful read", async () => {
    const fetcher = vi.fn().mockResolvedValueOnce(response(build))
      .mockRejectedValueOnce(new Error("unavailable"))
      .mockResolvedValueOnce(response(build));
    vi.stubGlobal("fetch", fetcher);
    await act(async () => { render(<FrontendUpdateNotice connected />); });
    expect(screen.queryByRole("status")).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
    expect(screen.getByText("화면 버전을 확인하지 못했어요.")).toBeTruthy();
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "다시 확인" })); });
    expect(screen.queryByRole("status")).toBeNull();
    expect(fetcher).toHaveBeenCalledTimes(3);
  });

  it("uses the compiled protocol and aborts an unfinished read on unmount", async () => {
    let signal: AbortSignal | undefined;
    const fetcher = vi.fn().mockResolvedValueOnce(response(build, Number(PROTOCOL_VERSION) + 1));
    vi.stubGlobal("fetch", fetcher);
    await act(async () => { render(<FrontendUpdateNotice connected />); });
    expect(screen.getByText("새 버전에 맞춰 화면을 새로고침해 주세요.")).toBeTruthy();
    cleanup();
    fetcher.mockImplementation((_url: string, init: RequestInit) => {
      signal = init.signal as AbortSignal;
      return new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => reject(new Error("aborted")), { once: true });
      });
    });
    const view = render(<FrontendUpdateNotice connected={false} />);
    expect(signal?.aborted).toBe(false);
    await act(async () => { view.unmount(); });
    expect(signal?.aborted).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
    expect(fetcher).toHaveBeenCalledTimes(2);
  });
});
