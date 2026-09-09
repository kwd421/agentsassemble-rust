import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fetchLocalResources } from "../api/localResources";
import AdminPanel from "./AdminPanel";

vi.mock("../api/localResources", () => ({ fetchLocalResources: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("keeps unknown CPU distinct, refreshes explicitly and clears stale results after failure", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValueOnce({
    observed_at: "2026-09-09T00:00:00Z", cpu_sample_seconds: null,
    cpu_count: 8, total_memory_bytes: null, available_memory_bytes: null,
    load_average: null, matching_process_count: 1,
    processes: [{ pid: 42, label: "AgentsAssemble", cpu_percent: null, memory_bytes: 1048576 }],
  }).mockRejectedValueOnce(new Error("unavailable"));
  render(<AdminPanel onClose={() => {}} />);
  expect(await screen.findByText("AgentsAssemble")).toBeTruthy();
  expect(screen.getByText(/CPU 측정 전 또는 확인 불가/)).toBeTruthy();
  expect(fetchLocalResources).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "상태 새로고침" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.queryByText("AgentsAssemble")).toBeNull();
  expect(fetchLocalResources).toHaveBeenCalledTimes(2);
});
