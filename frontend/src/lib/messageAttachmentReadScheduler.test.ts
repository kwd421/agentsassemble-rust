import { describe, expect, it, vi } from "vitest";

import type { LobbyAttachmentRef } from "../api/messageAttachments";
import {
  createMessageAttachmentReadOwner,
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
    const scheduler = createMessageAttachmentReadOwner(transport).forAuthority(
      "general",
      { kind: "local" }
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

  it("cancels active and queued reads without starting queued work", async () => {
    const activeSignals: AbortSignal[] = [];
    const transport = vi.fn(
      (_attachment, _roomId, _authority, _mode, signal: AbortSignal) =>
        new Promise<Blob>((_resolve, reject) => {
          activeSignals.push(signal);
          signal.addEventListener("abort", () => reject(signal.reason), { once: true });
        })
    );
    const scheduler = createMessageAttachmentReadOwner(transport).forAuthority(
      "general",
      { kind: "remote", sessionToken: "aas1.session" }
    );
    const controllers = Array.from({ length: 5 }, () => new AbortController());
    const reads = controllers.map((controller, index) =>
      scheduler.read(attachment(index), "view", controller.signal)
    );
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(4));

    controllers.forEach((controller) => controller.abort());

    const results = await Promise.allSettled(reads);
    expect(results.every((result) => result.status === "rejected")).toBe(true);
    expect(transport).toHaveBeenCalledTimes(4);
    expect(activeSignals.every((signal) => signal.aborted)).toBe(true);
  });

  it("does not enter transport when caller cancellation wins before scheduled start", async () => {
    const transport = vi.fn();
    const scheduler = createMessageAttachmentReadOwner(transport).forAuthority(
      "general",
      { kind: "local" }
    );
    const controller = new AbortController();
    const read = scheduler.read(
      attachment(1),
      "view",
      controller.signal
    );

    controller.abort();

    await expect(read).rejects.toMatchObject({ name: "AbortError" });
    await Promise.resolve();
    expect(transport).not.toHaveBeenCalled();
  });

  it("retains an aborted transport slot until that transport actually settles", async () => {
    const pending: Array<{ resolve: (blob: Blob) => void }> = [];
    const transport = vi.fn(
      (_attachment: LobbyAttachmentRef, _roomId: string) =>
        new Promise<Blob>((resolve) => pending.push({ resolve }))
    );
    const scheduler = createMessageAttachmentReadOwner(transport).forAuthority(
      "general",
      { kind: "local" }
    );
    const controllers = Array.from({ length: 5 }, () => new AbortController());
    const reads = controllers.map((controller, index) =>
      scheduler.read(attachment(index), "view", controller.signal)
    );
    await vi.waitFor(() =>
      expect(transport).toHaveBeenCalledTimes(MESSAGE_ATTACHMENT_READ_CONCURRENCY)
    );

    controllers[0]?.abort();
    await expect(reads[0]).rejects.toMatchObject({ name: "AbortError" });
    await Promise.resolve();
    expect(transport).toHaveBeenCalledTimes(4);

    pending[0]?.resolve(new Blob(["settled"], { type: "image/png" }));
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(5));
    pending.slice(1).forEach(({ resolve }) =>
      resolve(new Blob(["done"], { type: "image/png" }))
    );
    const remaining = await Promise.all(reads.slice(1));
    expect(remaining).toHaveLength(4);
  });

  it("shares actual transport capacity across room and authority generations", async () => {
    const pending: Array<{ resolve: (blob: Blob) => void }> = [];
    const transport = vi.fn(
      (_attachment: LobbyAttachmentRef, _roomId: string) =>
        new Promise<Blob>((resolve) => pending.push({ resolve }))
    );
    const owner = createMessageAttachmentReadOwner(transport);
    const first = owner.forAuthority("room-a", { kind: "local" });
    const next = owner.forAuthority(
      "room-b",
      { kind: "remote", sessionToken: "aas1.next" }
    );
    const firstControllers = Array.from(
      { length: MESSAGE_ATTACHMENT_READ_CONCURRENCY },
      () => new AbortController()
    );
    const firstReads = firstControllers.map((controller, index) =>
      first.read(attachment(index), "view", controller.signal)
    );
    const firstSettlements = Promise.allSettled(firstReads);
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(4));

    firstControllers.forEach((controller) => controller.abort());
    const nextRead = next.read(
      attachment(8),
      "view",
      new AbortController().signal
    );
    await Promise.resolve();
    expect(transport).toHaveBeenCalledTimes(4);

    pending[0]?.resolve(new Blob(["released"], { type: "image/png" }));
    await vi.waitFor(() => expect(transport).toHaveBeenCalledTimes(5));
    expect(transport.mock.calls[4]?.[1]).toBe("room-b");
    pending.slice(1).forEach(({ resolve }) =>
      resolve(new Blob(["done"], { type: "image/png" }))
    );

    await expect(nextRead).resolves.toBeInstanceOf(Blob);
    expect((await firstSettlements).every((result) => result.status === "rejected"))
      .toBe(true);
  });
});
