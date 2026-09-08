import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useChannelTranscript } from "./useChannelTranscript";
import type { RoomCommandAck, RoomSocketHandle } from "../roomSocketTypes";
import { channelId, channelMessage } from "../test/channelMessage";

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { resolve, promise };
}
function page(sequences: number[], last = sequences.at(-1) ?? 0, hasMore = false): RoomCommandAck {
  return { op: "ack", request_id: "history", action: "channel.history", accepted: true, resolution: "committed",
    result: { room_id: "general", channel_id: channelId, events: sequences.map((seq) => channelMessage(seq)), oldest_seq: sequences[0] ?? 0, last_seq: last, has_more_before: hasMore } };
}
function options(socket: RoomSocketHandle) {
  return { roomId: "general", roomUid: "uid", channelId, socket, connected: true };
}

describe("selected channel transcript", () => {
  it("merges only post-cut live events and bounds a burst without mixing another channel", async () => {
    const initial = deferred<RoomCommandAck>();
    const command = vi.fn(() => initial.promise);
    const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
    const hook = renderHook(() => useChannelTranscript(options(socket)));
    expect(command).toHaveBeenCalledWith("channel.history", { channel_id: channelId, before_seq: 0, limit: 80 });
    act(() => hook.result.current.receive([channelMessage(4), channelMessage(10), channelMessage(11, "c111111111111")]));
    await act(async () => initial.resolve(page([2, 6], 9)));
    expect(hook.result.current.events.map((event) => event.seq)).toEqual([2, 6, 10]);
    act(() => hook.result.current.receive(Array.from({ length: 300 }, (_, index) => channelMessage(index + 12))));
    expect(hook.result.current.events).toHaveLength(200);
    expect(hook.result.current.events[0].seq).toBe(112);
    expect(hook.result.current.hasMore).toBe(true);
    hook.unmount();
  });

  it("freezes the older window while paging and reloads current history on explicit latest navigation", async () => {
    const older = deferred<RoomCommandAck>();
    const command = vi.fn().mockResolvedValueOnce(page([81, 82], 90, true)).mockReturnValueOnce(older.promise).mockResolvedValueOnce(page([100], 100, true));
    const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
    const hook = renderHook(() => useChannelTranscript(options(socket)));
    await act(async () => {});
    let loading!: Promise<void>;
    act(() => { loading = hook.result.current.earlier(); hook.result.current.receive([channelMessage(100)]); });
    await act(async () => { older.resolve(page([79, 80], 100, true)); await loading; });
    expect(hook.result.current.events.map((event) => event.seq)).toEqual([79, 80, 81, 82]);
    expect(hook.result.current.newMessages).toBe(true);
    expect(hook.result.current.following).toBe(false);
    await act(async () => hook.result.current.latest());
    expect(hook.result.current.events.map((event) => event.seq)).toEqual([100]);
    expect(hook.result.current.following).toBe(true);
    hook.unmount();
  });

  it("hides a stale page on channel, room incarnation and connection changes", async () => {
    const initial = deferred<RoomCommandAck>();
    const command = vi.fn().mockReturnValueOnce(initial.promise).mockResolvedValue(page([20]));
    const socket = { ready: () => true, command } as unknown as RoomSocketHandle;
    const hook = renderHook((props) => useChannelTranscript(props), { initialProps: options(socket) });
    hook.rerender({ ...options(socket), connected: false });
    await act(async () => initial.resolve(page([1])));
    expect(hook.result.current.ready).toBe(false);
    expect(hook.result.current.events).toEqual([]);
    await act(async () => hook.rerender({ ...options(socket), roomUid: "recreated" }));
    expect(hook.result.current.events.map((event) => event.seq)).toEqual([20]);
    hook.rerender({ ...options(socket), channelId: "", connected: false });
    expect(hook.result.current.events).toEqual([]);
    hook.unmount();
  });
});
