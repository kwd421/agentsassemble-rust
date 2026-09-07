import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { OperatorPairingCustody } from "../api/operatorPairing";
import { useManagedOperatorPairings } from "./useManagedOperatorPairings";

const api = vi.hoisted(() => ({ revoke: vi.fn() }));
vi.mock("../api/operatorPairing", () => ({ revokeOperatorPairing: api.revoke }));
const custody: OperatorPairingCustody = {
  authority: {
    server_id: "10000000-0000-4000-8000-000000000001",
    authority_lineage_id: "20000000-0000-4000-8000-000000000001",
    room_id: "general", room_uid: "30000000-0000-4000-8000-000000000001",
  },
  pairingId: "40000000-0000-4000-8000-000000000001",
  pairingUrl: `https://pair.example.test/pair?token=aap1.${"a".repeat(43)}`,
  origin: "https://pair.example.test",
  expiresAt: "2026-09-08T00:02:00Z",
  expiresAtMs: Date.parse("2026-09-08T00:02:00Z"),
};

afterEach(() => { vi.useRealTimers(); api.revoke.mockReset(); });

function fixture() {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-09-08T00:00:00Z"));
  let origin = custody.origin;
  let current = true;
  const copied: string[] = [];
  const publishStatus = vi.fn();
  const hook = renderHook(() => useManagedOperatorPairings({
    roomDockId: "room-one", publicOrigin: origin,
    resolveManager: () => custody.authority,
    captureOriginRefresh: () => async () => ({ publicOrigin: origin, isCurrent: () => current }),
    copyText: async (text, prepare) => {
      const assertCurrent = await prepare();
      assertCurrent(); copied.push(text); return true;
    },
    publishStatus,
  }));
  act(() => hook.result.current.retain(custody, "room-one", true));
  return { hook, copied, publishStatus,
    changeOrigin: () => { origin = "https://changed.example.test"; },
    retire: () => { current = false; },
  };
}

it("expires only link copying while retaining the exact grant for revocation", async () => {
  const { hook, copied } = fixture();
  const key = hook.result.current.pairings[0].key;
  await act(() => hook.result.current.copy(key));
  expect(copied).toEqual([custody.pairingUrl]);
  act(() => vi.advanceTimersByTime(120_000));
  expect(hook.result.current.pairings[0]).toMatchObject({ expired: true, copyable: false });
  api.revoke.mockImplementationOnce(async (_custody, beforeDispatch) => beforeDispatch());
  await act(() => hook.result.current.revoke(key));
  expect(api.revoke).toHaveBeenCalledWith(custody, expect.any(Function));
  expect(hook.result.current.pairings[0].state).toBe("revoked");
  hook.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("does not copy a link after its refreshed public origin changes", async () => {
  const { hook, copied, changeOrigin } = fixture();
  changeOrigin();
  await act(() => hook.result.current.copy(hook.result.current.pairings[0].key));
  expect(copied).toEqual([]);
  hook.unmount();
});

it("keeps uncertain revocation non-copyable and retries the same grant", async () => {
  const { hook, copied } = fixture();
  const key = hook.result.current.pairings[0].key;
  api.revoke.mockRejectedValueOnce(new Error("connection lost"));
  await act(() => hook.result.current.revoke(key));
  expect(hook.result.current.pairings[0]).toMatchObject({ state: "unknown", copyable: false });
  await act(() => hook.result.current.copy(key));
  expect(copied).toEqual([]);
  api.revoke.mockResolvedValueOnce(undefined);
  await act(() => hook.result.current.revoke(key));
  expect(api.revoke.mock.calls.map(([grant]) => grant)).toEqual([custody, custody]);
  expect(hook.result.current.pairings[0].state).toBe("revoked");
  hook.unmount();
});
