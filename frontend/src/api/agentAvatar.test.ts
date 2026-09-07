import { afterEach, beforeEach, expect, it, vi } from "vitest";
const bridge = vi.hoisted(() => ({ upload: vi.fn() }));
vi.mock("../lib/desktopBridge", async () => ({
  ...await vi.importActual<typeof import("../lib/desktopBridge")>("../lib/desktopBridge"),
  requestDesktopAgentAvatarUploadTicket: bridge.upload,
}));
import { uploadAgentAvatar } from "./agentAvatar";
import { profileAvatarReference, resolveAttachmentReference } from "../lib/attachmentReference";

const manager = { server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002", room_id: "general",
  room_uid: "30000000-0000-4000-8000-000000000003" };
const id = `aa_${"a".repeat(32)}`;
const url = `/api/agent-avatars/${id}`;
const origin = "http://127.0.0.1:49154";
const metadata = { id, url, filename: "avatar.png", content_type: "image/png", size: 1, is_image: true };
const file = () => new File(["image"], "avatar.png", { type: "image/png" });
const response = (attachment: object) => new Response(JSON.stringify({ attachment }), {
  headers: { "Content-Type": "application/json", "Cache-Control": "private, no-store" },
});
beforeEach(() => bridge.upload.mockReset().mockResolvedValue({ ticket: "b".repeat(64), ttl_seconds: 30, http_base_url: origin }));
afterEach(() => vi.unstubAllGlobals());

it("uploads through the exact Agent ticket and keeps human reference parsing distinct", async () => {
  const fetchMock = vi.fn().mockResolvedValue(response(metadata));
  vi.stubGlobal("fetch", fetchMock);
  const controller = new AbortController();
  await expect(uploadAgentAvatar(file(), { kind: "local", manager }, "agent-one", controller.signal)).resolves.toBe(url);
  expect(bridge.upload).toHaveBeenCalledWith(manager, "agent-one");
  expect(fetchMock).toHaveBeenCalledWith(`${origin}/api/agent-avatars/upload/agent-one`, expect.objectContaining({
    redirect: "error", signal: controller.signal, method: "POST", cache: "no-store",
    headers: { Authorization: `Bearer ${"b".repeat(64)}`, "Content-Type": "application/json" },
  }));
  expect(resolveAttachmentReference(url, origin)).toBe(`${origin}${url}`);
  expect(() => profileAvatarReference(url)).toThrow();
  expect(resolveAttachmentReference(`${url}?view=1`, origin)).toBeUndefined();
  expect(resolveAttachmentReference(url, "https://user:pass@example.com")).toBeUndefined();
});

it("rejects foreign metadata and cancellation before any upload", async () => {
  const fetchMock = vi.fn().mockResolvedValue(response({ ...metadata, url: "/api/attachments/human-avatar?view=1" }));
  vi.stubGlobal("fetch", fetchMock);
  await expect(uploadAgentAvatar(file(), { kind: "local", manager }, "agent-one", new AbortController().signal)).rejects.toThrow();
  const controller = new AbortController();
  controller.abort();
  await expect(uploadAgentAvatar(file(), { kind: "local", manager }, "agent-one", controller.signal)).rejects.toThrow();
  expect(fetchMock).toHaveBeenCalledOnce();
  expect(bridge.upload).toHaveBeenCalledOnce();
});

it("uploads the paired session and exact device without requesting native authority", async () => {
  const fetchMock = vi.fn().mockResolvedValue(response(metadata));
  vi.stubGlobal("fetch", fetchMock);
  await expect(uploadAgentAvatar(file(), { kind: "remote", sessionToken: "aops1.paired", deviceToken: "paired-device" },
    "agent-one", new AbortController().signal)).resolves.toBe(url);
  expect(bridge.upload).not.toHaveBeenCalled();
  expect(fetchMock).toHaveBeenCalledWith("/api/agent-avatars/upload/agent-one", expect.objectContaining({
    headers: { Authorization: "Bearer aops1.paired", "Content-Type": "application/json", "X-Device-Token": "paired-device" },
  }));
});
