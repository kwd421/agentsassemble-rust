import { describe, expect, it, vi } from "vitest";

import type { LobbyAttachmentRef } from "../api/messageAttachments";
import {
  createMessageAttachmentReadScheduler,
  MESSAGE_ATTACHMENT_READ_CONCURRENCY,
} from "./messageAttachmentReadScheduler";

function attachment(index: number): LobbyAttachmentRef {
  const hex = index.toString(16);
  const id = `ma_${hex.repeat(32)}`;
  return {
    id,
    filename: `${hex}.png`,
    content_type: "image/png",
    size: 5,
    is_image: true,
    url: `/api/attachments/${id}?view=1`,
    download_url: `/api/attachments/${id}?download=1`,
  };
}

describe("message attachment read scheduler", () => {
  it("starts at most four reads and keeps the fifth behind the completion barrier", async () => {
    const pending: Array<{ resolve: (blob: Blob) => void }> = [];
    const transport = vi.fn(
      (_attachment, _roomId, _authority, _mode, signal: AbortSignal, beforeDispatch) =>
        new Promise<Blob>((resolve, reject) => {
          beforeDispatch?.();
          signal.addEventListener("abort", () => reject(signal.reason), { once: true });
          pending.push({ resolve });
        })
    );
    const scheduler = createMessageAttachmentReadScheduler(
      "general",
      { kind: "local" },
      transport
    );
    const controllers = Array.from({ length: 5 }, () => new AbortController());
    const reads = controllers.map((controller, index) =>
      scheduler.read(attachment(index), "view", controller.signal)
    );

    await vi.waitFor(() =>
      expect(transport).toHaveBeenCalledTimes(MESSAGE_ATTACHMENT_READ_CONCURRENCY)
    );
    expect(transport).toHaveBeenCalledTimes(4);
    pending[0]?.resolve(new Blob(["first"], { type: "image/png" }));
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(5));
    pending.slice(1).forEach(({ resolve }) =>
      resolve(new Blob(["next"], { type: "image/png" }))
    );
    await expect(Promise.all(reads)).resolves.toHaveLength(5);
  });

  it("retires active and queued reads without starting queued work", async () => {
    const activeSignals: AbortSignal[] = [];
    const transport = vi.fn(
      (_attachment, _roomId, _authority, _mode, signal: AbortSignal) =>
        new Promise<Blob>((_resolve, reject) => {
          activeSignals.push(signal);
          signal.addEventListener("abort", () => reject(signal.reason), { once: true });
        })
    );
    const scheduler = createMessageAttachmentReadScheduler(
      "general",
      { kind: "remote", sessionToken: "aas1.session" },
      transport
    );
    const reads = Array.from({ length: 5 }, (_, index) =>
      scheduler.read(attachment(index), "view", new AbortController().signal)
    );
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(4));

    scheduler.retire();

    const results = await Promise.allSettled(reads);
    expect(results.every((result) => result.status === "rejected")).toBe(true);
    expect(transport).toHaveBeenCalledTimes(4);
    expect(activeSignals.every((signal) => signal.aborted)).toBe(true);
    await expect(
      scheduler.read(attachment(6), "view", new AbortController().signal)
    ).rejects.toMatchObject({ name: "AbortError" });
  });
});
