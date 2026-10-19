import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { fetchLocalResources } from "../api/localResources";
import AdminPanel from "./AdminPanel";

vi.mock("./components/RuntimeRestartPanel", () => ({ default: () => null }));
vi.mock("../api/localResources", () => ({ fetchLocalResources: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("keeps unknown CPU distinct, keeps reading while open, clears stale results after failure and stops when closed", async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(fetchLocalResources).mockResolvedValueOnce({
      attachment_storage: { rooms: [], other: { files: 0, bytes: 0 }, total: { files: 0, bytes: 0 } },
      observed_at: "2026-09-09T00:00:00Z", cpu_sample_seconds: null,
      cpu_count: 8, total_memory_bytes: 24_000_000_000, available_memory_bytes: null,
      load_average: null, matching_process_count: 1,
      processes: [{ pid: 42, label: "codex", cpu_percent: null, memory_bytes: 1048576 }],
    }).mockRejectedValue(new Error("unavailable"));
    const view = render(<AdminPanel onClose={() => {}} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(0); });
    expect(screen.getByText("Codex")).toBeTruthy();
    expect(screen.getByText("확인 중")).toBeTruthy();
    expect(screen.getByText("24.0 GB")).toBeTruthy();
    expect(screen.getByText("1.0 MB")).toBeTruthy();
    expect(screen.queryByText(/PID|MiB/)).toBeNull();
    expect(fetchLocalResources).toHaveBeenCalledTimes(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(2000); });
    expect(fetchLocalResources).toHaveBeenCalledTimes(2);
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(screen.queryByText("Codex")).toBeNull();
    view.unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(10_000); });
    expect(fetchLocalResources).toHaveBeenCalledTimes(2);
  } finally { vi.useRealTimers(); }
});

it("groups same-name processes, retains unknown totals and lists every tool", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValue({
    attachment_storage: { rooms: [], other: { files: 0, bytes: 0 }, total: { files: 0, bytes: 0 } },
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
  const table = await screen.findByRole("table", { name: "프로세스 사용량" });
  const text = (name: string) => [...table.querySelectorAll("tbody tr")].find(row => row.textContent?.includes(name))?.textContent || "";
  expect(table.querySelectorAll("tbody tr")).toHaveLength(6);
  expect(text("Python × 2")).toContain("30.0%");
  expect(text("Python × 2")).toContain("3.0 MB");
  expect(text("Codex × 2")).toContain("확인 중");
  expect(text("Codex × 2")).toContain("확인 불가");
  expect(screen.getByText("Grok")).toBeTruthy();
});

it("lists stored files per room, largest first, with the parts and the total the server reported", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValue({
    observed_at: "2026-10-12T00:00:00Z", cpu_sample_seconds: 1, cpu_count: 8, total_memory_bytes: null, available_memory_bytes: null,
    load_average: null, matching_process_count: 0, processes: [],
    attachment_storage: { rooms: [{ room_id: "a", label: "작은 방", files: 1, bytes: 2_000_000 }, { room_id: "b", label: "큰 방", files: 3, bytes: 40_000_000 }],
      other: { files: 2, bytes: 1_000_000 }, total: { files: 6, bytes: 43_000_000 } },
  });
  render(<AdminPanel onClose={() => {}} />);
  const table = await screen.findByRole("table", { name: "방별 저장 공간" });
  const rows = [...table.querySelectorAll("tbody tr")].map(row => row.textContent);
  expect(rows[0]).toContain("큰 방");
  expect(rows[1]).toContain("작은 방");
  expect(rows[2]).toContain("1.0 MB");
  expect(table.querySelector("tfoot")?.textContent).toContain("43.0 MB");
});

it("never shows another owner's rooms or the restart control to a person who does not own this computer's server", async () => {
  vi.mocked(fetchLocalResources).mockResolvedValue({
    observed_at: "2026-10-12T00:00:00Z", cpu_sample_seconds: 1, cpu_count: 8, total_memory_bytes: null, available_memory_bytes: null,
    load_average: null, matching_process_count: 0, processes: [],
    attachment_storage: { rooms: [{ room_id: "a", label: "주인의 방", files: 1, bytes: 2_000_000 }],
      other: { files: 0, bytes: 0 }, total: { files: 1, bytes: 2_000_000 } },
  });
  render(<AdminPanel onClose={() => {}} ownsLocalServer={false} />);
  await screen.findByRole("region", { name: "앱과 관련 도구" });
  expect(screen.queryByRole("table", { name: "방별 저장 공간" })).toBeNull();
  expect(screen.queryByText("주인의 방")).toBeNull();
  expect(screen.queryByRole("button", { name: "서버 재시작" })).toBeNull();
});
