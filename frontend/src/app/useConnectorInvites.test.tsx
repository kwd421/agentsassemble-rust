import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useManagedAiInvites } from "./useManagedAiInvites";

const api = vi.hoisted(() => ({ create: vi.fn(), friend: vi.fn() }));
vi.mock("../api/connectorInvite", () => ({ createConnectorInvite: api.create }));
vi.mock("../api/attendeeInvite", async (importOriginal) => ({ ...await importOriginal<typeof import("../api/attendeeInvite")>(), createFriendAttendeeInvite: api.friend }));
afterEach(() => { vi.useRealTimers(); api.create.mockReset(); api.friend.mockReset(); });

it("retries uncertain creation with the same identity and guards copying by refreshed origin and expiry", async () => {
  vi.useFakeTimers(); vi.setSystemTime(new Date("2026-09-08T00:00:00Z"));
  const authority = { server_id: "server", authority_lineage_id: "lineage", room_id: "general", room_uid: "room-uid" };
  let origin = "https://public.example.test";
  const copied: string[] = [];
  const publishStatus = vi.fn();
  const hook = renderHook(() => useManagedAiInvites({
    roomDockId: "general", publicOrigin: origin, resolveManager: () => authority,
    captureOriginRefresh: () => async () => ({ publicOrigin: origin, isCurrent: () => true }),
    copyText: async (text, prepare) => { const guard = await prepare(); guard(); copied.push(text); return true; },
    publishStatus,
  }));
  api.create.mockRejectedValueOnce(new Error("response lost"));
  await act(() => hook.result.current.create());
  expect(hook.result.current.invites).toEqual([]);
  api.create.mockImplementationOnce(async (_authority, request, guard) => {
    guard();
    return { authority, origin, expiresAtMs: Date.now() + 60_000,
      result: { request_id: request.request_id, room_uid: authority.room_uid, invite_id: "invite", expires_at: "2026-09-08T00:01:00Z", join_url: `${origin}/join?token=private` } };
  });
  await act(() => hook.result.current.create());
  expect(api.create.mock.calls[0][1]).toEqual(api.create.mock.calls[1][1]);
  await act(() => hook.result.current.copy("invite"));
  expect(copied).toHaveLength(1);
  origin = "https://changed.example.test";
  await act(() => hook.result.current.copy("invite"));
  expect(copied).toHaveLength(1);
  act(() => vi.advanceTimersByTime(60_000));
  expect(hook.result.current.invites[0].copyable).toBe(false);
  hook.unmount(); expect(vi.getTimerCount()).toBe(0);
});

it("blocks a retired creation after ticket acquisition and before dispatch", async () => {
  let current = true;
  const publishStatus = vi.fn();
  const authority = { server_id: "server", authority_lineage_id: "lineage", room_id: "general", room_uid: "uid" };
  const hook = renderHook(() => useManagedAiInvites({
    roomDockId: "general", publicOrigin: "https://public.example.test", resolveManager: () => authority,
    captureOriginRefresh: () => async () => ({ publicOrigin: "https://public.example.test", isCurrent: () => current }),
    copyText: vi.fn(), publishStatus,
  }));
  api.create.mockImplementationOnce(async (_authority, _request, guard) => { current = false; guard(); });
  await act(() => hook.result.current.create());
  expect(hook.result.current.invites).toEqual([]);
  expect(publishStatus).not.toHaveBeenCalled();
  hook.unmount();
});

it("retains distinct retry receipts for each AI friend and copies the full attendee packet", async () => {
  const authority = { server_id: "server", authority_lineage_id: "lineage", room_id: "general", room_uid: "uid" };
  const origin = "https://public.example.test";
  const copied: string[] = [];
  const hook = renderHook(() => useManagedAiInvites({ roomDockId: "general", publicOrigin: origin, resolveManager: () => authority,
    captureOriginRefresh: () => async () => ({ publicOrigin: origin, isCurrent: () => true }), publishStatus: vi.fn(),
    copyText: async (text, prepare) => { (await prepare())(); copied.push(text); return true; } }));
  api.friend.mockRejectedValue(new Error("response lost"));
  await act(() => hook.result.current.create("friend-a"));
  await act(() => hook.result.current.create("friend-b"));
  api.friend.mockImplementationOnce(async (_authority, request, guard) => {
    guard(); return { origin, expiresAtMs: Date.now() + 60_000, result: { ...request, room_id: "general", room_uid: "uid",
      invite_id: "invite-a", display_name: "Friend A", provider: "codex", attend_command: "assemble room attend --provider codex",
      join_url: `${origin}/join?token=fixture`, expires_at: new Date(Date.now() + 60_000).toISOString() } };
  });
  await act(() => hook.result.current.create("friend-a"));
  expect(api.friend.mock.calls[0][1]).toEqual(api.friend.mock.calls[2][1]);
  expect(api.friend.mock.calls[0][1].request_id).not.toBe(api.friend.mock.calls[1][1].request_id);
  expect(api.create).not.toHaveBeenCalled();
  expect(hook.result.current.invites).toEqual([]);
  expect(hook.result.current.attendeeInvites[0].displayName).toBe("Friend A");
  await act(() => hook.result.current.copy("invite-a"));
  expect(copied[0]).toContain("assemble room attend --provider codex");
  expect(copied[0]).toContain(`${origin}/join?token=fixture`);
});
