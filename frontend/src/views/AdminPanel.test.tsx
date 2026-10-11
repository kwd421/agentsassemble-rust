import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fetchLocalResources } from "../api/localResources";
import AdminPanel from "./AdminPanel";

vi.mock("./components/RuntimeRestartPanel", () => ({ default: () => null }));
vi.mock("./components/ReleaseHealthPanel", () => ({ default: () => null }));
vi.mock("../api/localResources", () => ({ fetchLocalResources: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("keeps unknown CPU distinct, refreshes explicitly and clears stale results after failure", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValueOnce({
    observed_at: "2026-09-09T00:00:00Z", cpu_sample_seconds: null,
    cpu_count: 8, total_memory_bytes: 24_000_000_000, available_memory_bytes: null,
    load_average: null, matching_process_count: 1,
    processes: [{ pid: 42, label: "codex", cpu_percent: null, memory_bytes: 1048576 }],
  }).mockRejectedValueOnce(new Error("unavailable"));
  render(<AdminPanel onClose={() => {}} />);
  expect(await screen.findByText("Codex")).toBeTruthy();
  expect(screen.getByText("확인 중")).toBeTruthy();
  expect(screen.getByText("24.0 GB")).toBeTruthy();
  expect(screen.getByText("1.0 MB")).toBeTruthy();
  expect(screen.queryByText(/PID|MiB/)).toBeNull();
  expect(fetchLocalResources).toHaveBeenCalledTimes(1);
  fireEvent.click(screen.getByRole("button", { name: "상태 새로고침" }));
  expect(await screen.findByRole("alert")).toBeTruthy();
  expect(screen.queryByText("Codex")).toBeNull();
  expect(fetchLocalResources).toHaveBeenCalledTimes(2);
});


it("groups same-name processes, retains unknown totals and expands the remaining names", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValue({
    observed_at: "2026-10-11T00:00:00Z", cpu_sample_seconds: 1,
    cpu_count: 8, total_memory_bytes: null, available_memory_bytes: null,
    load_average: null, matching_process_count: 8,
    processes: [
      { pid: 1, label: "python", cpu_percent: 10, memory_bytes: 1_000_000 },
      { pid: 2, label: "python3", cpu_percent: 20, memory_bytes: 2_000_000 },
      { pid: 3, label: "codex", cpu_percent: null, memory_bytes: null },
      { pid: 4, label: "codex", cpu_percent: 15, memory_bytes: 4_000_000 },
      ...["node", "claude", "cursor", "grok"].map((label, i) => ({ pid: 5 + i, label, cpu_percent: 1, memory_bytes: 1 })),
    ],
  });
  render(<AdminPanel onClose={() => {}} />);
  const table = await screen.findByRole("table");
  expect(table.querySelectorAll("tbody tr")).toHaveLength(5);
  const rows = table.querySelectorAll("tbody tr");
  expect(rows[0].textContent).toContain("Python × 2");
  expect(rows[0].textContent).toContain("30.0%");
  expect(rows[0].textContent).toContain("3.0 MB");
  expect(rows[1].textContent).toContain("확인 중");
  expect(rows[1].textContent).toContain("확인 불가");
  expect(screen.queryByText("Grok")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "더 보기" }));
  expect(screen.getByText("Grok")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "접기" }));
  expect(screen.queryByText("Grok")).toBeNull();
});
