import { afterEach, expect, test, vi } from "vitest";
import { fetchPlainProductTransport, messageAttachmentCapabilities, supportsMessageAttachmentPreview } from "./remoteWorkspace";

afterEach(() => vi.unstubAllGlobals());
test("only a successful settings probe changes established plain capabilities", async () => {
  const base = "https://capabilities.example";
  const advertised = () => new Response("{}", { headers: {
    "x-agentsassemble-upload-transfer": "1", "x-agentsassemble-message-attachment-limit": "10", "x-agentsassemble-image-preview": "1",
  } });
  const fetcher = vi.fn().mockResolvedValueOnce(advertised());
  vi.stubGlobal("fetch", fetcher);
  const capabilities = () => messageAttachmentCapabilities("fixture", base);
  const preview = () => supportsMessageAttachmentPreview("fixture", base);
  await fetchPlainProductTransport(`${base}/api/room-settings?room_id=general`);
  expect(capabilities()).toEqual({ transfer: true, limit: 10 }); expect(preview()).toBe(true);
  for (const [path, init, response] of [
    ["/api/message-attachments", {}, new Response("{}")],
    ["/api/anything", {}, new Response("{}", { status: 500 })],
    ["/api/room-settings?room_id=general", {}, new Response("{}", { status: 403 })],
    ["/api/room-settings", { method: "POST" }, new Response("{}")],
  ] as const) {
    fetcher.mockResolvedValueOnce(response);
    await fetchPlainProductTransport(base + path, init);
    expect(capabilities()).toEqual({ transfer: true, limit: 10 }); expect(preview()).toBe(true);
  }
  // Explicit new successful admission/probe can discover a genuinely older host.
  fetcher.mockResolvedValueOnce(new Response("{}"));
  await fetchPlainProductTransport(`${base}/api/room-settings?room_id=general`);
  expect(capabilities()).toEqual({ transfer: false, limit: 8 }); expect(preview()).toBe(false);
});
