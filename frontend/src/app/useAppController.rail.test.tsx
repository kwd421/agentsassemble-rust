import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { useAppController } from "./useAppController";

const mocks = vi.hoisted(() => ({ refresh: vi.fn(), open: vi.fn(), directory: null as CentralDirectoryState | null }));
vi.mock("./useCentralDirectory", () => ({ useCentralDirectory: () => ({ directory: mocks.directory, refresh: mocks.refresh }) }));
vi.mock("../lib/central/identity", async (original) => ({
  ...await original<typeof import("../lib/central/identity")>(), openCentralOwnedServer: mocks.open,
}));
vi.mock("../useCanonicalRoom", () => {
  const room = { room: null, socket: null, connectionState: "disconnected", roomSettings: null,
    participants: [], agentSessions: [], capabilities: {}, timelineEvents: [],
    history: { lastSeq: 0 }, displayResourceBase: "", agentSessionProgress: null };
  return { useCanonicalRoom: () => room };
});
afterEach(cleanup);

it("retains missing-server feedback across connected directory refreshes until the next open", async () => {
  const server = { server_id: "missing", alias: "Offline server", relation: "owner" as const, host_os: null };
  const { result, rerender } = renderHook(() => useAppController("", "rail-test"));
  mocks.refresh.mockImplementation(async () => {
    mocks.directory = { status: "connected", servers: [], live: { servers: [] } } as unknown as CentralDirectoryState;
    return mocks.directory;
  });
  await act(async () => { await result.current.openRailServer(server); });
  expect(result.current.serverConnectionError).toBe("서버 연결이 끊겼어요. 로그인 서버 연결을 다시 확인하고 있어요.");
  expect(mocks.open).not.toHaveBeenCalled();
  mocks.directory = { ...mocks.directory! };
  rerender();
  expect(result.current.serverConnectionError).toBe("서버 연결이 끊겼어요. 로그인 서버 연결을 다시 확인하고 있어요.");
  let resolve!: (value: unknown) => void;
  mocks.refresh.mockImplementation(() => new Promise((done) => { resolve = done; }));
  let opening!: Promise<void>;
  act(() => { opening = result.current.openRailServer({ ...server, server_id: "online" }); });
  expect(result.current.serverConnectionError).toBe("");
  expect(result.current.connectingServerId).toBe("online");
  const live = { ...server, server_id: "online" };
  await act(async () => { resolve({ live: { servers: [live] } }); await opening; });
  expect(mocks.open).toHaveBeenCalledWith(live);
  expect(result.current.serverConnectionError).toBe("");
});
