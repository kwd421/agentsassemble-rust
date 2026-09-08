import { afterEach, describe, expect, it, vi } from "vitest";
import { fetchSideChatSnapshot } from "./sideChat";
import { requestDesktopSideChatReadTicket } from "../lib/desktopBridge";
vi.mock("../lib/desktopBridge", () => ({ requestDesktopSideChatReadTicket: vi.fn() }));
afterEach(() => { vi.unstubAllGlobals(); vi.resetAllMocks(); });

describe("private side-chat HTTP cut", () => {
  it("binds remote authority and expected room incarnation and requires private no-store responses", async () => {
    const data = { room_id: "general", room_uid: "uid", generation: "00000000-0000-4000-8000-000000000001", retained_after_seq: 0, latest_seq: 0, messages: [] };
    const fetch = vi.fn().mockResolvedValue(new Response(JSON.stringify(data), { headers: { "Content-Type": "application/json", "Cache-Control": "private, no-store" } }));
    vi.stubGlobal("fetch", fetch);
    const authority = { kind: "remote" as const, sessionToken: "session", deviceToken: "device" };
    const signal = new AbortController().signal;
    await expect(fetchSideChatSnapshot("general", "uid", authority, signal)).resolves.toEqual(data);
    expect(fetch).toHaveBeenCalledWith("/api/side-chat?room_id=general", { signal, cache: "no-store", headers: { Authorization: "Bearer session", "X-Device-Token": "device" } });
    fetch.mockResolvedValue(new Response(JSON.stringify(data), { headers: { "Content-Type": "application/json", "Cache-Control": "public" } }));
    await expect(fetchSideChatSnapshot("general", "uid", authority, signal)).rejects.toThrow("보호 설정");
  });
  it("never dispatches a late native ticket after its owning connection was aborted", async () => {
    let resolve!: (value: Awaited<ReturnType<typeof requestDesktopSideChatReadTicket>>) => void;
    vi.mocked(requestDesktopSideChatReadTicket).mockReturnValue(new Promise((done) => { resolve = done; }));
    const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
    const controller = new AbortController();
    const request = fetchSideChatSnapshot("general", "uid", { kind: "local" }, controller.signal);
    controller.abort();
    resolve({ http_base_url: "http://127.0.0.1:1234", ticket: "single-use" } as Awaited<ReturnType<typeof requestDesktopSideChatReadTicket>>);
    await expect(request).rejects.toThrow();
    expect(fetch).not.toHaveBeenCalled();
  });
});
