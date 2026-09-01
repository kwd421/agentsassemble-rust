import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  read: vi.fn(),
  write: vi.fn(),
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  requestDesktopMessagePinsReadTicket: bridge.read,
  requestDesktopMessagePinsWriteTicket: bridge.write,
}));

import {
  fetchLobbyMessagePins,
  setLobbyMessagePinned,
} from "./messagePins";

function grant(ticket: string) {
  return {
    ticket,
    ttl_seconds: 30,
    http_base_url: "http://127.0.0.1:49154",
  };
}

function pin(eventId = "event-1") {
  return {
    event_id: eventId,
    channel_id: "lobby",
    pinned_at: "2026-08-29T01:02:03.456Z",
    seq: 7,
    author: "Operator",
    content: "canonical message",
    created_at: "2026-08-29T01:00:00Z",
    attachment_filenames: [],
  };
}

function jsonResponse(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("lobby message-pin HTTP authority", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses distinct local read and write grants on the issued loopback base", async () => {
    bridge.read.mockResolvedValue(grant("a".repeat(64)));
    bridge.write.mockResolvedValue(grant("b".repeat(64)));
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ pins: [pin()] }))
      .mockResolvedValueOnce(jsonResponse({ pinned: false, pins: [] }));
    vi.stubGlobal("fetch", fetchMock);

    const listed = await fetchLobbyMessagePins({
      roomId: "general",
      authority: { kind: "local" },
    });
    const changed = await setLobbyMessagePinned({
      roomId: "general",
      eventId: "event-1",
      pinned: false,
      authority: { kind: "local" },
    });

    expect(listed).toEqual([pin()]);
    expect(changed).toEqual([]);
    expect(bridge.read).toHaveBeenCalledWith("general");
    expect(bridge.write).toHaveBeenCalledWith("general");
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49154/api/room-pins?room_id=general&channel_id=lobby",
      expect.objectContaining({ cache: "no-store", headers: expect.any(Headers) })
    );
    const readHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const writeInit = fetchMock.mock.calls[1]?.[1] as RequestInit;
    expect(readHeaders.get("Authorization")).toBe(`Bearer ${"a".repeat(64)}`);
    expect((writeInit.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"b".repeat(64)}`
    );
    expect(JSON.parse(String(writeInit.body))).toEqual({
      room_id: "general",
      channel_id: "lobby",
      event_id: "event-1",
      pinned: false,
    });
  });

  it("presents a remote session directly to each pin operation", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ pins: [] }))
      .mockResolvedValueOnce(jsonResponse({ pinned: true, pins: [pin()] }));
    vi.stubGlobal("fetch", fetchMock);
    const authority = { kind: "remote", sessionToken: "aas1.session" } as const;

    await fetchLobbyMessagePins({ roomId: "general", authority });
    await setLobbyMessagePinned({
      roomId: "general",
      eventId: "event-1",
      pinned: true,
      authority,
    });

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "/api/room-pins?room_id=general&channel_id=lobby",
      expect.objectContaining({ headers: expect.any(Headers) })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "/api/room-pins",
      expect.objectContaining({ method: "POST", headers: expect.any(Headers) })
    );
    const readHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const writeHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(readHeaders.get("Authorization")).toBe("Bearer aas1.session");
    expect(writeHeaders.get("Authorization")).toBe("Bearer aas1.session");
  });

  it("surfaces a read-only denial from the pin target", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      jsonResponse(
        {
          error: {
            code: "permission_denied",
            message: "This room session cannot modify messages.",
          },
        },
        403
      )
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: "event-1",
        pinned: true,
        authority: { kind: "remote", sessionToken: "aas1.read-only" },
      })
    ).rejects.toThrow("This room session cannot modify messages.");
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/room-pins");
  });

  it("checks local operation currentness after a deferred grant and before dispatch", async () => {
    let resolveGrant: ((value: ReturnType<typeof grant>) => void) | undefined;
    bridge.read.mockReturnValueOnce(
      new Promise<ReturnType<typeof grant>>((resolve) => {
        resolveGrant = resolve;
      })
    );
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    let current = true;
    const request = fetchLobbyMessagePins({
      roomId: "general",
      authority: { kind: "local" },
      beforeDispatch: () => {
        if (!current) throw new Error("retired");
      },
    });

    current = false;
    resolveGrant?.(grant("a".repeat(64)));

    await expect(request).rejects.toThrow("retired");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("checks remote operation currentness immediately before target dispatch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: "event-1",
        pinned: true,
        authority: { kind: "remote", sessionToken: "aas1.session" },
        beforeDispatch: () => {
          throw new Error("retired");
        },
      })
    ).rejects.toThrow("retired");
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects malformed complete-list state instead of projecting defaults", async () => {
    bridge.read.mockResolvedValue(grant("a".repeat(64)));
    const malformed = [
      { pins: [{ ...pin(), channel_id: "side" }] },
      { pins: [{ ...pin(), attachment_filenames: ["not-yet-owned"] }] },
      { pins: [pin(), pin()] },
      { pins: [pin("event-1"), pin("event-2")] },
      { pins: Array.from({ length: 65 }, (_, index) => pin(`event-${index}`)) },
      { pins: [{ ...pin(), content: "" }] },
      { pins: [{ ...pin(), content: " \t\n\u200b" }] },
      { pins: [{ ...pin(), event_id: "event-\ud800" }] },
      { pins: [{ ...pin(), content: "message \udfff" }] },
      { pins: [{ ...pin(), author: "Operator \ud800" }] },
      { pins: [{ ...pin(), pinned_at: "2026-01-01 (\ud800)" }] },
      { pins: [{ ...pin(), created_at: "2026-01-01 (\udfff)" }] },
      { pins: [], ignored: true },
    ];

    for (const payload of malformed) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse(payload)));
      await expect(
        fetchLobbyMessagePins({
          roomId: "general",
          authority: { kind: "local" },
        })
      ).rejects.toThrow();
    }
  });

  it("accepts valid non-BMP Unicode scalars in canonical pin strings", async () => {
    bridge.read.mockResolvedValue(grant("a".repeat(64)));
    const scalarPin = {
      ...pin("event-😀"),
      author: "Operator 😀",
      content: "canonical 😀 message",
    };
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ pins: [scalarPin] }))
    );

    await expect(
      fetchLobbyMessagePins({
        roomId: "general",
        authority: { kind: "local" },
      })
    ).resolves.toEqual([scalarPin]);
  });

  it("rejects invalid identities before consuming a native grant", async () => {
    await expect(
      fetchLobbyMessagePins({
        roomId: " general",
        authority: { kind: "local" },
      })
    ).rejects.toThrow("방 식별자");
    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: `event-${"a".repeat(129)}`,
        pinned: true,
        authority: { kind: "local" },
      })
    ).rejects.toThrow("메시지 식별자");
    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: "é".repeat(65),
        pinned: true,
        authority: { kind: "local" },
      })
    ).rejects.toThrow("메시지 식별자");
    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: "event-\ud800",
        pinned: true,
        authority: { kind: "local" },
      })
    ).rejects.toThrow("메시지 식별자");
    expect(bridge.read).not.toHaveBeenCalled();
    expect(bridge.write).not.toHaveBeenCalled();
  });

  it("rejects a mutation response whose complete list contradicts its result", async () => {
    bridge.write.mockResolvedValue(grant("b".repeat(64)));
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ pinned: true, pins: [] }))
    );

    await expect(
      setLobbyMessagePinned({
        roomId: "general",
        eventId: "event-1",
        pinned: true,
        authority: { kind: "local" },
      })
    ).rejects.toThrow("응답 계약");
  });
});
