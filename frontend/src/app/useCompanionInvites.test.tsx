import { act, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useCompanionInvites } from "./useCompanionInvites";
import type { RoomGuestSession } from "../lib/roomGuestSession";

const api = vi.hoisted(() => ({ create: vi.fn(), copy: vi.fn() }));
vi.mock("../api/attendeeInvite", async (original) => ({ ...await original<typeof import("../api/attendeeInvite")>(), createCompanionAttendeeInvite: api.create }));
vi.mock("../lib/copyInviteText", () => ({ copyText: api.copy }));
afterEach(() => { vi.useRealTimers(); vi.clearAllMocks(); });
const session = { roomUid: "uid", meetingId: "general", sessionToken: "fixture-human", expiresAt: "2099-01-01T00:00:00Z" } as RoomGuestSession;
function packet() {
  return { origin: window.location.origin, expiresAtMs: Date.now() + 60_000, result: {
    request_id: "request", room_id: "general", room_uid: "uid", invite_id: "invite", display_name: "Companion", provider: "codex",
    attend_command: "assemble room attend --provider codex", join_url: `${window.location.origin}/join?token=fixture`, expires_at: new Date(Date.now() + 60_000).toISOString(),
  } };
}

it("reuses uncertain receipts, expires packet copying, and retires authority on disconnect", async () => {
  vi.useFakeTimers();
  const hook = renderHook(({ owner }) => useCompanionInvites(owner), { initialProps: { owner: session as RoomGuestSession | null } });
  act(() => { hook.result.current.setProvider("codex"); hook.result.current.setDisplayName("Companion"); });
  api.create.mockRejectedValueOnce(new Error("response lost"));
  await act(() => hook.result.current.create());
  api.create.mockResolvedValueOnce(packet());
  await act(() => hook.result.current.create());
  expect(api.create.mock.calls[0][1]).toEqual(api.create.mock.calls[1][1]);
  api.copy.mockImplementation(async (_text, prepare) => { (await prepare())(); return true; });
  await act(() => hook.result.current.copy("invite"));
  expect(api.copy.mock.calls[0][0]).toContain("assemble room attend --provider codex");
  act(() => vi.advanceTimersByTime(60_000));
  expect(hook.result.current.invites[0].copyable).toBe(false);
  const retiredCopy = hook.result.current.copy;
  hook.rerender({ owner: null });
  await act(() => retiredCopy("invite"));
  expect(api.copy).toHaveBeenCalledTimes(1);
  expect(hook.result.current.invites).toEqual([]);
  hook.unmount(); expect(vi.getTimerCount()).toBe(0);
});

it("does not publish a response into a replacement human session", async () => {
  let complete!: (value: ReturnType<typeof packet>) => void;
  api.create.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
  const hook = renderHook(({ owner }) => useCompanionInvites(owner), { initialProps: { owner: session } });
  act(() => { hook.result.current.setProvider("codex"); hook.result.current.setDisplayName("Companion"); });
  let operation!: Promise<void>;
  act(() => { operation = hook.result.current.create(); });
  hook.rerender({ owner: { ...session, sessionToken: "replacement" } });
  await act(async () => { complete(packet()); await operation; });
  expect(hook.result.current.invites).toEqual([]);
  expect(hook.result.current.status).toBe("");
});
