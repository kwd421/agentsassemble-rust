import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { readReleaseHealth } from "../../api/releaseHealth";
import ReleaseHealthPanel from "./ReleaseHealthPanel";
vi.mock("../../api/releaseHealth", () => ({ readReleaseHealth: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
it("distinguishes not-run, timed-out and unreadable reports without retaining stale results", async () => {
  const checks = [{ id: "git_diff", label: "Git 공백 검사" }];
  vi.mocked(readReleaseHealth).mockResolvedValueOnce({ checks, report: null })
    .mockResolvedValueOnce({ checks, report: { started_at: "2026-09-09T00:00:00Z", completed_at: "2026-09-09T00:01:00Z", results: [{ check_id: "git_diff", status: "timed_out", duration_seconds: 60 }] } })
    .mockRejectedValueOnce(new Error("corrupt report"));
  render(<ReleaseHealthPanel />);
  expect(await screen.findByText("미실행")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "결과 새로고침" }));
  expect(await screen.findByText("시간 초과")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "결과 새로고침" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.queryByText("시간 초과")).toBeNull();
  expect(readReleaseHealth).toHaveBeenCalledTimes(3);
});
