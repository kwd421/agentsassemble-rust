import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useRoomSideChat } from "./useRoomSideChat";
import { fetchSideChatSnapshot } from "../api/sideChat";
import type { SideChatSnapshot } from "../types/generated/SideChatSnapshot";
import type { SideChatUpdate } from "../types/generated/SideChatUpdate";
import type { RoomSocketHandle } from "../roomSocketTypes";

vi.mock("../api/sideChat", () => ({ fetchSideChatSnapshot: vi.fn() }));
const generation = "00000000-0000-4000-8000-000000000001";
function update(seq: number, retained_after_seq = 0): SideChatUpdate {
  return { room_id: "general", generation, retained_after_seq, message: {
    id: `00000000-0000-4000-8000-${String(seq).padStart(12, "0")}`, seq,
    created_at: "2026-09-08T00:00:00Z", participant_id: "operator-local", display_name: "Me", content: `message ${seq}`,
  } };
}
function snapshot(...seqs: number[]): SideChatSnapshot {
  return { room_id: "general", room_uid: "room-incarnation", generation, retained_after_seq: 0,
    latest_seq: seqs.at(-1) ?? 0, messages: seqs.map((seq) => update(seq).message) };
}
function pending<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

beforeEach(() => vi.resetAllMocks());
describe("human side-chat projection lifetime", () => {
  it("merges an overlapping HTTP cut, live delivery and exact ACK once, then applies the retained floor", async () => {
    const initial = pending<SideChatSnapshot>();
    vi.mocked(fetchSideChatSnapshot).mockReturnValue(initial.promise);
    const hook = renderHook(() => useRoomSideChat("general", { kind: "local" }));
    act(() => { hook.result.current.connect("room-incarnation"); hook.result.current.receive(update(2)); hook.result.current.receive(update(3)); });
    await act(async () => { initial.resolve(snapshot(1, 2)); await initial.promise; });
    expect(hook.result.current.snapshot?.messages.map((message) => message.seq)).toEqual([1, 2, 3]);
    const ack = pending<Awaited<ReturnType<RoomSocketHandle["command"]>>>();
    const command = vi.fn(() => ack.promise);
    const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
    let posting!: Promise<void>;
    act(() => { posting = hook.result.current.send("message 4", socket); });
    expect(hook.result.current.busy).toBe(true);
    expect(command).toHaveBeenCalledWith("side_chat.send", { generation, after_seq: 3, content: "message 4" });
    await expect(hook.result.current.send("message 4", socket)).rejects.toThrow();
    act(() => hook.result.current.receive(update(4, 2)));
    await act(async () => { ack.resolve({ op: "ack", request_id: "send", accepted: true, resolution: "committed", action: "side_chat.send", result: { update: update(4, 2) } }); await posting; });
    expect(hook.result.current.snapshot?.messages.map((message) => message.seq)).toEqual([3, 4]);
    expect(hook.result.current.busy).toBe(false);
    hook.unmount();
  });

  it("aborts stale bootstrap and hides private history immediately across authority, room and reconnect boundaries", async () => {
    const old = pending<SideChatSnapshot>();
    vi.mocked(fetchSideChatSnapshot).mockReturnValueOnce(old.promise).mockResolvedValue(snapshot(1));
    const hook = renderHook(({ token }) => useRoomSideChat("general", { kind: "remote", sessionToken: token }), { initialProps: { token: "first" } });
    act(() => hook.result.current.connect("room-incarnation"));
    const signal = vi.mocked(fetchSideChatSnapshot).mock.calls[0][3];
    hook.rerender({ token: "second" });
    expect(signal.aborted).toBe(true);
    await act(async () => { old.resolve(snapshot(1, 2)); await old.promise; });
    expect(hook.result.current.snapshot).toBeNull();
    await act(async () => hook.result.current.connect("room-incarnation"));
    expect(hook.result.current.snapshot?.latest_seq).toBe(1);
    act(() => hook.result.current.disconnect());
    expect(hook.result.current.snapshot).toBeNull();
    act(() => hook.result.current.receive(update(2)));
    expect(hook.result.current.snapshot).toBeNull();
    hook.unmount();
  });


  it("accepts an ACK ahead of queued live messages and keeps drafts isolated by current room incarnation", async () => {
    vi.mocked(fetchSideChatSnapshot).mockResolvedValue(snapshot(1));
    const hook = renderHook(({ room }) => useRoomSideChat(room, { kind: "local" }), { initialProps: { room: "general" } });
    await act(async () => hook.result.current.connect("room-incarnation"));
    act(() => hook.result.current.updateDraft("keep this draft"));
    const socket = { ready: () => true, command: vi.fn().mockResolvedValue({ result: { update: update(4) } }) } as unknown as RoomSocketHandle;
    await act(async () => hook.result.current.send("message 4", socket));
    expect(hook.result.current.snapshot?.latest_seq).toBe(1);
    act(() => { hook.result.current.receive(update(2)); hook.result.current.receive(update(3)); hook.result.current.receive(update(4)); });
    expect(hook.result.current.snapshot?.latest_seq).toBe(4);
    hook.rerender({ room: "other" });
    expect(hook.result.current.draft).toBe("");
    hook.rerender({ room: "general" });
    await act(async () => hook.result.current.connect("room-incarnation"));
    expect(hook.result.current.draft).toBe("keep this draft");
    vi.mocked(fetchSideChatSnapshot).mockResolvedValue({ ...snapshot(), room_uid: "recreated-room" });
    await act(async () => hook.result.current.connect("recreated-room"));
    expect(hook.result.current.draft).toBe("");
    hook.unmount();
  });

  it("exposes missing live sequence and process-generation changes without fabricating successful history", async () => {
    vi.mocked(fetchSideChatSnapshot).mockResolvedValue(snapshot(1));
    const hook = renderHook(() => useRoomSideChat("general", { kind: "local" }));
    await act(async () => hook.result.current.connect("room-incarnation"));
    act(() => hook.result.current.receive(update(3)));
    expect(hook.result.current.snapshot).toBeNull();
    expect(hook.result.current.error).toContain("일부");
    await act(async () => hook.result.current.connect("room-incarnation"));
    act(() => hook.result.current.receive({ ...update(2), generation: "00000000-0000-4000-8000-000000000002" }));
    expect(hook.result.current.snapshot).toBeNull();
    expect(hook.result.current.error).toContain("새로 시작");
    hook.unmount();
  });
});
