import { beforeEach, expect, test, vi } from "vitest";
import { createMessageAttachmentUploadClient, UploadFailure } from "./messageAttachmentTransfers";
import { selectLobbyAttachmentFiles } from "../lib/lobbyComposerModel";
const fake = vi.hoisted(() => ({ fetch: vi.fn(), ticket: vi.fn(), legacy: vi.fn(), capabilities: { transfer: true, limit: 10 }, fileLimit: 1000000 }));
vi.mock("./room", () => ({ fetchRoomSettings: async () => ({ attachmentFileLimitBytes: fake.fileLimit }) }));
vi.mock("../lib/desktopBridge", () => ({ requestDesktopMessageAttachmentUploadTicket: fake.ticket }));
vi.mock("../lib/remote/remoteWorkspace", () => ({ fetchProductTransport: fake.fetch, messageAttachmentCapabilities: () => fake.capabilities }));
vi.mock("./messageAttachments", async importOriginal => ({ ...await importOriginal<object>(), uploadMessageAttachment: fake.legacy }));
const id = "ma_" + "a".repeat(32);
const status = { handle: "handle", attachment_id: id, piece_size: 65536, expires_at: 1, declared_size: 4, accepted_offset: 0, attachment: null };
const response = (value: unknown, status = 200) => new Response(JSON.stringify(value), { status, headers: { "Content-Type": "application/json", "Cache-Control": "private, no-store" } });
const signal = () => new AbortController().signal;
beforeEach(() => {
  vi.clearAllMocks(); fake.fileLimit = 1000000; fake.capabilities = { transfer: true, limit: 10 };
  fake.ticket.mockResolvedValue({ http_base_url: "http://localhost:1234", ticket: "fixture-ticket" });
});
test("Blob data uses exact authenticated lifecycle routes without buffering", async () => {
  fake.fetch.mockImplementation(async (_path: string, init: RequestInit) => response(init.method === "DELETE" ? { cancelled: true } : { transfer: status }));
  const client = await createMessageAttachmentUploadClient("general", { kind: "remote", sessionToken: "fixture-session", deviceToken: "fixture-device" });
  const file = new File(["abcd"], "evidence.bin");
  file.arrayBuffer = vi.fn(() => { throw new Error("whole-file buffering"); });
  await client.begin(file, "operation", signal()); await client.data("handle", 0, file, signal());
  await client.status("handle", signal()); await client.finish("handle", signal()); await client.cancel("handle");
  expect(fake.fetch.mock.calls.map(([path]) => path)).toEqual([
    "/api/message-attachment-transfers", "/api/message-attachment-transfers/handle/data?offset=0",
    "/api/message-attachment-transfers/handle", "/api/message-attachment-transfers/handle/finish", "/api/message-attachment-transfers/handle",
  ]);
  expect(fake.fetch.mock.calls[1][1].body).toBe(file);
  expect(new Headers(fake.fetch.mock.calls[1][1].headers).get("Content-Type")).toBe("application/octet-stream");
  expect(new Headers(fake.fetch.mock.calls[1][1].headers).get("X-Device-Token")).toBe("fixture-device");
  expect(file.arrayBuffer).not.toHaveBeenCalled();
  expect(selectLobbyAttachmentFiles(0, Array.from({ length: 10 }), client.limit).tooMany).toBe(false);
});
test("local client uses the preference discovery and operations get fresh one-use authority", async () => {
  fake.fetch.mockImplementation(async (_path: string, init: RequestInit) => init.method === "GET" ? new Response(null, { status: 405 }) : response({ transfer: status }));
  const client = await createMessageAttachmentUploadClient("general", { kind: "local" });
  expect(fake.fetch).not.toHaveBeenCalled();
  const file = new File(["abcd"], "f.bin");
  await client.begin(file, "one", signal()); await client.data("handle", 0, file, signal());
  expect(fake.ticket).toHaveBeenCalledTimes(2);
  expect(fake.fetch.mock.calls[1][0]).toBe("http://localhost:1234/api/message-attachment-transfers/handle/data?offset=0");
});
test("old host uses legacy and refuses a ten-file selection whole", async () => {
  fake.capabilities = { transfer: false, limit: 8 };
  const client = await createMessageAttachmentUploadClient("general", { kind: "remote", sessionToken: "fixture-session" });
  const file = new File(["abcd"], "f.bin"); await client.legacy(file, signal());
  expect(fake.legacy).toHaveBeenCalledWith(file, "general", { kind: "remote", sessionToken: "fixture-session" }, undefined, expect.any(AbortSignal));
  expect(selectLobbyAttachmentFiles(0, Array.from({ length: 10 }), client.limit)).toMatchObject({ accepted: [], tooMany: true });
  await expect(client.begin(file, "one", signal())).rejects.toMatchObject({ code: "upload_unsupported" });
  expect(fake.fetch).not.toHaveBeenCalled();
  fake.capabilities = { transfer: true, limit: 8 };
  const sliceThreeHost = await createMessageAttachmentUploadClient("general", { kind: "remote", sessionToken: "fixture-session" });
  expect(selectLobbyAttachmentFiles(0, Array.from({ length: 10 }), sliceThreeHost.limit)).toMatchObject({ accepted: [], tooMany: true });
});
test("defer is structured; preflight dispatches nothing", async () => {
  fake.fetch.mockResolvedValue(response({ code: "upload_deferred", error: "busy", disposition: "defer", retry_after_ms: 1000 }, 429));
  const client = await createMessageAttachmentUploadClient("general", { kind: "remote", sessionToken: "fixture-session" });
  expect(() => client.preflight(new File([new Uint8Array(1000001)], "large.bin"))).toThrow(UploadFailure);
  expect(fake.fetch).not.toHaveBeenCalled();
  fake.fileLimit = 2;
  await expect(client.begin(new File(["abcd"], "f.bin"), "one", signal())).rejects.toMatchObject({ code: "file_size_limit" });
  expect(fake.fetch).not.toHaveBeenCalled();
  fake.fileLimit = 1000000;
  await expect(client.begin(new File(["abcd"], "f.bin"), "one", signal())).rejects.toMatchObject({ code: "upload_deferred", retryable: true, retryAfterMs: 1000 });
});
