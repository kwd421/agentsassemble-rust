import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import AgentMemorySettings, { keepsMemory } from "./AgentMemorySettings";
import type { RoomAgentSession } from "../../api";

const memoryApi = vi.hoisted(() => ({ fetchAgentMemory: vi.fn(), deleteAgentMemory: vi.fn() }));
vi.mock("../../api/agentMemory", () => memoryApi);

afterEach(cleanup);
beforeEach(() => {
  memoryApi.fetchAgentMemory.mockReset();
  memoryApi.deleteAgentMemory.mockReset();
});

const session = { room_id: "room", session_id: "agent", display_name: "Full-tester", runtime_kind: "api", execution_harness: "builtin" } as RoomAgentSession;

it("keeps memory only for API agents on the built-in harness", () => {
  expect(keepsMemory(session)).toBe(true);
  expect(keepsMemory({ ...session, execution_harness: "opencode" })).toBe(false);
  expect(keepsMemory({ ...session, runtime_kind: "cli" })).toBe(false);
});

it("loads memory when opened and deletes a piece on the second press", async () => {
  memoryApi.fetchAgentMemory.mockResolvedValue([{ from_seq: 5, to_seq: 20, summary: "등대 이야기", created_at: 1_790_000_000 }]);
  memoryApi.deleteAgentMemory.mockResolvedValue(undefined);
  const { container } = render(<AgentMemorySettings session={session} />);
  expect(memoryApi.fetchAgentMemory).not.toHaveBeenCalled();
  const details = container.querySelector("details")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  expect(await screen.findByText("등대 이야기")).toBeTruthy();
  expect(memoryApi.fetchAgentMemory).toHaveBeenCalledWith("room", "agent", undefined);
  fireEvent.click(screen.getByRole("button", { name: "이 기억 지우기" }));
  expect(memoryApi.deleteAgentMemory).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "지우기" }));
  await waitFor(() => expect(memoryApi.deleteAgentMemory).toHaveBeenCalledWith("room", "agent", 5, undefined));
  await waitFor(() => expect(screen.queryByText("등대 이야기")).toBeNull());
});

it("explains an empty memory", async () => {
  memoryApi.fetchAgentMemory.mockResolvedValue([]);
  const { container } = render(<AgentMemorySettings session={session} />);
  const details = container.querySelector("details")!;
  details.open = true;
  fireEvent(details, new Event("toggle"));
  expect(await screen.findByText(/아직 기억이 없어요/)).toBeTruthy();
});

it("shows loading, then drops the old list when a reload fails", async () => {
  memoryApi.fetchAgentMemory.mockResolvedValueOnce([{ from_seq: 5, to_seq: 20, summary: "등대 이야기", created_at: 1_790_000_000 }]);
  const { container } = render(<AgentMemorySettings session={session} />);
  const details = container.querySelector("details")!;
  const toggle = (open: boolean) => { details.open = open; fireEvent(details, new Event("toggle")); };
  toggle(true);
  expect(screen.getByText("기억을 불러오는 중이에요.")).toBeTruthy();
  expect(await screen.findByText("등대 이야기")).toBeTruthy();
  toggle(false);
  memoryApi.fetchAgentMemory.mockRejectedValueOnce(new Error("offline"));
  toggle(true);
  expect(screen.getByText("기억을 불러오는 중이에요.")).toBeTruthy();
  expect(await screen.findByRole("status")).toBeTruthy();
  expect(screen.queryByText("등대 이야기")).toBeNull();
});
