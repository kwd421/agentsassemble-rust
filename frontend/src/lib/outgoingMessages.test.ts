import { afterEach, expect, test, vi } from "vitest";
import { OutgoingMessages, type OutgoingAuthority, type OutgoingMessage, type OutgoingOptions } from "./outgoingMessages";
import { UploadFailure, type MessageAttachmentUploadClient, type TransferStatus } from "../api/messageAttachmentTransfers";
import type { LobbyAttachmentRef } from "../api/messageAttachments";
import { RoomSocketSayError, type RoomCommandAck } from "../roomSocketTypes";
import type { RoomEvent } from "../types/generated/RoomEvent";

const authority = { roomId: "general", participantId: "person", generation: 1 };
const ack: RoomCommandAck = { op: "ack", accepted: true, request_id: "request", action: "message.send", resolution: "committed", deduplicated: false };
function gate<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const reads = new Set<Promise<string>>();
function read(blob: Blob): Promise<string> {
  const promise = new Promise<string>((resolve, reject) => {
    const reader = new FileReader(); reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error); reader.readAsText(blob);
  });
  reads.add(promise); void promise.finally(() => reads.delete(promise)); return promise;
}
async function flush() {
  for (let i = 0; i < 100; i++) { await Promise.resolve(); if (reads.size) await Promise.all(reads); }
}
function file(name = "one.bin") {
  const file = new File(["abcdefgh"], name);
  file.arrayBuffer = vi.fn(() => { throw new Error("whole-file buffering"); });
  return file;
}
function fixture() {
  const stored = new Map<string, { status: TransferStatus; bytes: string; file: File }>();
  let next = 0;
  const client: MessageAttachmentUploadClient = {
    transfer: true, limit: 10, fileLimit: 50 * 1024 * 1024, preflight: () => {},
    begin: vi.fn(async file => {
      const handle = `handle-${++next}`;
      const status = { handle, attachment_id: `ma_${next.toString(16).padStart(32, "0")}`, piece_size: 4, expires_at: 9999999999, declared_size: file.size, accepted_offset: 0, attachment: null };
      stored.set(handle, { status, bytes: "", file }); return { ...status };
    }),
    data: vi.fn(async (handle, offset, body) => {
      const entry = stored.get(handle)!;
      expect(body).toBeInstanceOf(Blob); expect(body.size).toBeLessThanOrEqual(64 * 4);
      expect(offset).toBe(entry.bytes.length);
      entry.bytes += await read(body); entry.status = { ...entry.status, accepted_offset: entry.bytes.length };
      return { ...entry.status };
    }),
    status: vi.fn(async handle => ({ ...stored.get(handle)!.status })),
    finish: vi.fn(async handle => {
      const entry = stored.get(handle)!;
      const attachment: LobbyAttachmentRef = { id: entry.status.attachment_id, filename: entry.file.name, content_type: "application/octet-stream", size: entry.file.size, is_image: false, url: "", download_url: "" };
      entry.status = { ...entry.status, attachment }; return { ...entry.status };
    }),
    cancel: vi.fn(async () => {}), legacy: vi.fn(async () => { throw new Error("unexpected legacy"); }),
  };
  const publish = vi.fn<OutgoingOptions["publish"]>(async () => ack);
  const model = new OutgoingMessages({ hostId: "host", personId: "person", publish });
  model.admit(authority, client);
  return { client, stored, model, publish };
}
const one = (model: OutgoingMessages): OutgoingMessage => model.snapshot()[0];
function delivery(message: OutgoingMessage, who: OutgoingAuthority = authority): RoomEvent {
  return { v: 1, id: "delivered", seq: 1, created_at: "", room_id: who.roomId, type: "message_final", actor: { participant_id: who.participantId, participant_type: "human" }, attachments: message.files.map(file => ({ ...file.attachment! })) };
}
afterEach(() => vi.useRealTimers());

test("ten ordered files across messages/rooms share three admissions and publish complete sets", async () => {
  const { model, client, stored, publish } = fixture();
  const entered: string[] = [];
  const release = gate<void>();
  const begin = client.begin;
  client.begin = vi.fn<MessageAttachmentUploadClient["begin"]>(async (...args) => { entered.push(args[0].name); await release.promise; return begin(...args); });
  const other = { ...authority, roomId: "other" }; model.admit(other, client);
  const staged = Array.from({ length: 10 }, (_, index) => file(`${index}.bin`));
  const id = model.send(authority, { text: "original", replyToEventId: "reply", files: staged });
  model.send(other, { text: "next draft", files: [file("next.bin")] });
  staged.reverse(); await flush(); expect(entered).toEqual(["0.bin", "1.bin", "2.bin"]);
  expect(publish).not.toHaveBeenCalled(); release.resolve(); await flush();
  const result = model.snapshot().find(message => message.id === id)!;
  expect(result).toMatchObject({ phase: "sent", acceptedBytes: 80, totalBytes: 80 });
  expect(publish).toHaveBeenCalledTimes(2);
  expect(publish.mock.calls[0]).toEqual([authority, expect.objectContaining({ message: "original", replyToEventId: "reply", attachments: expect.any(Array) })]);
  expect((publish.mock.calls[0] as unknown as [unknown, { attachments: LobbyAttachmentRef[] }])[1].attachments.map(a => a.filename)).toEqual(Array.from({ length: 10 }, (_, i) => `${i}.bin`));
  for (const entry of stored.values()) { expect(entry.bytes).toBe("abcdefgh"); expect(entry.file.arrayBuffer).not.toHaveBeenCalled(); }
  expect(Object.isFrozen(result.files)).toBe(true); model.close();
});

test("host defer returns its slot to the queue and clock hint retries without failure", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] }); const { model, client, publish } = fixture();
  const begin = client.begin;
  client.begin = vi.fn<MessageAttachmentUploadClient["begin"]>().mockRejectedValueOnce(new UploadFailure("upload_deferred", true, 1000)).mockImplementation(begin);
  const phases: string[] = []; model.subscribe(() => { if (one(model)) phases.push(one(model).phase); });
  model.send(authority, { text: "", files: [file()] }); await flush();
  expect(one(model).phase).toBe("deferred"); expect(publish).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(999); expect(client.begin).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1); await flush();
  expect(one(model).phase).toBe("sent"); expect(phases).not.toContain("failed"); model.close();
});

test("accepted offset resumes interrupted data; lost finish uses status, one attachment/publication", async () => {
  const { model, client, stored, publish } = fixture();
  const data = client.data; const finish = client.finish;
  client.data = vi.fn<MessageAttachmentUploadClient["data"]>().mockImplementationOnce(async (handle, offset, body, signal) => { await data(handle, offset, body.slice(0, 4), signal); throw new Error("connection lost mid-request"); }).mockImplementation(data);
  client.finish = vi.fn<MessageAttachmentUploadClient["finish"]>(async (...args) => { await finish(...args); throw new Error("finish reply lost"); });
  model.send(authority, { text: "", files: [file()] }); await flush();
  expect(one(model).phase).toBe("sent"); expect([...stored.values()][0].bytes).toBe("abcdefgh");
  expect(vi.mocked(client.data).mock.calls.map(call => call[1])).toEqual([0, 4]);
  expect(client.status).toHaveBeenCalledTimes(2); expect(client.finish).toHaveBeenCalledTimes(1);
  expect(client.begin).toHaveBeenCalledTimes(1); expect(publish).toHaveBeenCalledTimes(1); model.close();
});

test("partial failure keeps finished files, explicit retry transfers only the failed file", async () => {
  const { model, client, publish } = fixture(); const begin = client.begin;
  let fail = true;
  client.begin = vi.fn<MessageAttachmentUploadClient["begin"]>(async (...args) => {
    if (args[0].name === "bad.bin" && fail) { fail = false; throw new UploadFailure("storage_failure", true); }
    return begin(...args);
  });
  const id = model.send(authority, { text: "", files: [file("good.bin"), file("bad.bin")] }); await flush();
  expect(one(model)).toMatchObject({ phase: "failed", failure: { retryable: true } });
  expect(one(model).files[0].phase).toBe("ready"); expect(publish).not.toHaveBeenCalled();
  expect(model.retry(id)).toBe(true); await flush(); expect(publish).toHaveBeenCalledTimes(1);
  expect(vi.mocked(client.begin).mock.calls.map(call => call[0].name)).toEqual(["good.bin", "bad.bin", "bad.bin"]); model.close();
});

test("lost begin response retains its operation identity and resumes one host receipt on retry", async () => {
  const { model, client, stored, publish } = fixture(); const begin = client.begin;
  const receipts = new Map<string, TransferStatus>(); let lost = false;
  client.begin = vi.fn<MessageAttachmentUploadClient["begin"]>(async (...args) => {
    const prior = receipts.get(args[1]); if (prior) return prior;
    const receipt = await begin(...args); receipts.set(args[1], receipt);
    if (!lost) { lost = true; throw new Error("begin response lost"); }
    return receipt;
  });
  const id = model.send(authority, { text: "", files: [file()] }); await flush();
  expect(one(model)).toMatchObject({ phase: "failed", failure: { retryable: true } });
  expect(model.retry(id)).toBe(true); await flush();
  const calls = vi.mocked(client.begin).mock.calls;
  expect(calls).toHaveLength(2); expect(calls[1][1]).toBe(calls[0][1]);
  expect(stored.size).toBe(1); expect(one(model).phase).toBe("sent"); expect(publish).toHaveBeenCalledTimes(1); model.close();
});

test("cancel aborts transfers, retires host handles, retains ready files and never publishes", async () => {
  const { model, client, publish } = fixture(); const data = client.data;
  const entered = gate<void>();
  client.data = vi.fn<MessageAttachmentUploadClient["data"]>(async (...args) => {
    if (args[0] === "handle-2") {
      entered.resolve(); await new Promise<void>(resolve => args[3].addEventListener("abort", () => resolve(), { once: true }));
      args[3].throwIfAborted();
    }
    return data(...args);
  });
  const id = model.send(authority, { text: "", files: [file(), file()] }); await entered.promise; await flush();
  expect(await model.cancel(id)).toBe(true); await flush();
  expect(one(model).phase).toBe("cancelled"); expect(client.cancel).toHaveBeenCalledWith("handle-2");
  expect(publish).not.toHaveBeenCalled(); client.data = data;
  expect(model.retry(id)).toBe(true); await flush(); expect(one(model).phase).toBe("sent");
  expect(client.begin).toHaveBeenCalledTimes(3); model.close();
});

test("new admission restarts incomplete files; stale completions/departure cannot publish", async () => {
  const { model, client, publish } = fixture(); const begin = client.begin;
  const failures: (string | undefined)[] = [];
  model.subscribe(() => { failures.push(one(model)?.failure?.code); });
  client.cancel = vi.fn().mockRejectedValueOnce(new UploadFailure("session_revoked", false)).mockResolvedValue(undefined);
  const late = gate<TransferStatus>();
  client.begin = vi.fn<MessageAttachmentUploadClient["begin"]>().mockImplementationOnce(() => late.promise).mockImplementation(begin);
  model.send(authority, { text: "", files: [file()] }); await flush();
  const fresh = { ...authority, generation: 2 }; model.admit(fresh, client);
  late.resolve(await begin(file(), "old-operation", new AbortController().signal)); await flush();
  expect(client.cancel).toHaveBeenCalledWith("handle-1"); expect(one(model).authority).toEqual(fresh);
  expect(failures).not.toContain("session_revoked");
  expect(publish).toHaveBeenCalledTimes(1); expect(publish.mock.calls[0][0]).toEqual(fresh);
  const pending = gate<TransferStatus>(); client.begin = () => pending.promise;
  model.send(fresh, { text: "", files: [file()] }); await flush(); model.leaveRoom(fresh.roomId);
  pending.resolve(await begin(file(), "leaving", new AbortController().signal)); await flush();
  expect(model.snapshot()).toEqual([]); expect(publish).toHaveBeenCalledTimes(1); model.close();
});

test("publication is non-cancellable; delivery before lost ACK wins and unknown requires explicit retry", async () => {
  const { model, publish } = fixture(); const pending = gate<RoomCommandAck>();
  publish.mockImplementationOnce(() => pending.promise);
  const id = model.send(authority, { text: "", files: [file()] }); await flush();
  expect(one(model).phase).toBe("sending"); expect(await model.cancel(id)).toBe(false);
  const delivered = delivery(one(model)); model.observe(authority, [delivered]);
  pending.reject(new RoomSocketSayError("lost", "outcome_unknown")); await flush();
  expect(one(model)).toMatchObject({ phase: "sent", eventId: "delivered" });
  const exactRetry = vi.fn(async () => ack);
  publish.mockRejectedValueOnce(new RoomSocketSayError("lost", "outcome_unknown", exactRetry));
  const uncertain = model.send(authority, { text: "", files: [file()] }); await flush();
  expect(model.snapshot()[1].phase).toBe("outcome_unknown"); await flush(); expect(publish).toHaveBeenCalledTimes(2);
  expect(model.retry(uncertain)).toBe(true); await flush();
  expect(exactRetry).toHaveBeenCalledTimes(1); expect(publish).toHaveBeenCalledTimes(2);
  await model.discard(id); model.close();
});

test("uncertain delivery matches complete ordered ids and authority; definite refusals retain ready files until expiry", async () => {
  const { model, publish, client } = fixture(); publish.mockRejectedValueOnce(new RoomSocketSayError("lost", "outcome_unknown"));
  model.send(authority, { text: "", files: [file(), file()] }); await flush();
  const delivered = delivery(one(model));
  model.observe({ ...authority, generation: 0 }, [delivered]);
  model.observe(authority, [{ ...delivered, room_id: "other" }, { ...delivered, attachments: [...delivered.attachments as []].reverse() }, { ...delivered, actor: { participant_id: "other", participant_type: "human" } }]);
  expect(one(model).phase).toBe("outcome_unknown"); model.observe(authority, [delivered]); expect(one(model).phase).toBe("sent");
  for (const code of ["muted", "persistence_failed"]) {
    publish.mockRejectedValueOnce(new RoomSocketSayError("refused", code));
    const refused = model.send(authority, { text: "", files: [file()] }); await flush();
    expect(model.snapshot().at(-1)).toMatchObject({ phase: "failed", failure: { retryable: true } });
    const uploads = vi.mocked(client.begin).mock.calls.length;
    expect(model.retry(refused)).toBe(true); await flush();
    expect(model.snapshot().at(-1)?.phase).toBe("sent"); expect(client.begin).toHaveBeenCalledTimes(uploads);
  }
  publish.mockRejectedValueOnce(new RoomSocketSayError("expired", "attachment_unavailable"));
  const id = model.send(authority, { text: "", files: [file()] }); await flush();
  expect(model.snapshot().at(-1)).toMatchObject({ phase: "failed", failure: { code: "attachment_unavailable", retryable: false } });
  expect(model.retry(id)).toBe(false); expect(await model.cancel(id)).toBe(true);
  expect(model.retry(id)).toBe(false); expect(await model.discard(id)).toBe(true); model.close();
});

test("legacy host keeps its eight-file limit and uploads whole files one at a time", async () => {
  const { model, client, publish } = fixture();
  let inFlight = 0; let peak = 0;
  const old = { ...client, transfer: false, limit: 8, legacy: vi.fn(async (f: File) => {
    peak = Math.max(peak, ++inFlight); await Promise.resolve(); inFlight--;
    return { id: `ma_${f.name.padStart(32, "0")}`, filename: f.name, size: f.size, content_type: "application/octet-stream", is_image: false, url: "", download_url: "" };
  }) };
  const next = { ...authority, generation: 2 }; model.admit(next, old);
  expect(() => model.send(next, { text: "", files: Array.from({ length: 10 }, () => file()) })).toThrow(UploadFailure);
  expect(() => model.send(next, { text: "text only", files: [] })).toThrow(UploadFailure);
  model.send(next, { text: "", files: [file("1"), file("2")] }); await flush();
  expect(old.legacy).toHaveBeenCalledTimes(2); expect(client.begin).not.toHaveBeenCalled(); expect(publish).toHaveBeenCalledTimes(1);
  expect(peak).toBe(1); model.close();
});
