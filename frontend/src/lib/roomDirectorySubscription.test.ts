import { afterEach, describe, expect, it, vi } from "vitest";
import { readDirectoryStream, subscribeRoomDirectory } from "./roomDirectorySubscription";

const headers = { "content-type": "text/event-stream", "cache-control": "private, no-store" };
afterEach(() => vi.useRealTimers());

describe("room directory transport", () => {
  it("consumes host state controls on one stream without reloading the directory and stops on revocation", async () => {
    const statuses = vi.fn();
    const changed = vi.fn();
    const frames = ['event: directory_changed\ndata: {}\n\n',
      ...[{ state: "active" }, { state: "active" }, { state: "ended", reason: "revoked" }]
        .map(status => `event: owner_session\ndata: ${JSON.stringify(status)}\n\n`)];
    const response = new Response(frames.join(""), { headers });
    await expect(readDirectoryStream(response, new AbortController().signal, changed, statuses)).rejects.toMatchObject({ status: 401 });
    expect(changed).toHaveBeenCalledOnce();
    expect(statuses).toHaveBeenCalledTimes(3);
    expect(statuses).toHaveBeenLastCalledWith({ state: "ended", reason: "revoked" });
    await expect(readDirectoryStream(new Response(frames[1], { headers }), new AbortController().signal, changed)).rejects.toThrow("알림이 올바르지");
  });
  it("accepts a fragmented empty invalidation and cancels its owned body on exit", async () => {
    const abort = new AbortController();
    const cancel = vi.fn();
    const changed = vi.fn(async () => { abort.abort(); });
    const response = new Response(new ReadableStream({
      start(controller) {
        for (const text of [":\n\nevent: directory_", "changed\ndata: {}\n", "\n"]) {
          controller.enqueue(new TextEncoder().encode(text));
        }
      }, cancel,
    }), { headers });
    await readDirectoryStream(response, abort.signal, changed);
    expect(changed).toHaveBeenCalledOnce();
    expect(cancel).toHaveBeenCalledOnce();
  });

  it("rejects data-bearing or oversized frames without notifying the directory", async () => {
    const changed = vi.fn();
    for (const text of ['event: directory_changed\ndata: {"rooms":[]}\n\n', "x".repeat(4097)]) {
      const response = new Response(new TextEncoder().encode(text), { headers });
      await expect(readDirectoryStream(response, new AbortController().signal, changed)).rejects.toThrow();
    }
    expect(changed).not.toHaveBeenCalled();
  });

  it("stops on rejected ownership and cancels a pending bounded reconnect", async () => {
    vi.useFakeTimers();
    const open = vi.fn().mockResolvedValue(new Response('{"code":"permission_denied","error":"denied"}', { status: 403 }));
    const failed = vi.fn();
    const first = subscribeRoomDirectory(open, vi.fn(), failed);
    await vi.runAllTimersAsync();
    expect(open).toHaveBeenCalledOnce();
    expect(failed).toHaveBeenCalledOnce();
    first.close();
    open.mockRejectedValue(new Error("offline"));
    const second = subscribeRoomDirectory(open, vi.fn(), failed);
    await vi.advanceTimersByTimeAsync(0);
    second.close();
    await vi.runAllTimersAsync();
    expect(open).toHaveBeenCalledTimes(2);
  });
});
