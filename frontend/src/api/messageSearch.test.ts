import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({ read: vi.fn() }));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  requestDesktopMessageSearchReadTicket: bridge.read,
}));

import {
  fetchRoomMessageContext,
  searchRoomMessages,
} from "./messageSearch";

function grant(ticket: string) {
  return {
    ticket,
    ttl_seconds: 30,
    http_base_url: "http://127.0.0.1:49154",
  };
}

function jsonResponse(value: unknown, privateResponse = true) {
  return new Response(JSON.stringify(value), {
    headers: privateResponse
      ? { "Content-Type": "application/json", "Cache-Control": "private, no-store" }
      : { "Content-Type": "application/json" },
  });
}

function result(eventId = "event-1", seq = 7) {
  return {
    event_id: eventId,
    channel_id: "lobby",
    seq,
    created_at: "2026-08-29T01:00:00Z",
    author: "Operator",
    content: "canonical message",
    attachment_filenames: [],
  };
}

function contextEvent(eventId = "event-1", seq = 7) {
  return {
    v: 1,
    id: eventId,
    seq,
    created_at: "2026-08-29T01:00:00Z",
    room_id: "general",
    type: "message_final",
    actor: { participant_id: "operator-local", participant_type: "human" },
    participant_id: "operator-local",
    participant_type: "human",
    actor_id: "operator-local",
    actor_type: "human",
    display_name: "Operator",
    content: "canonical message",
    message_kind: "message",
  };
}

function agentContextEvent(eventId = "event-agent", seq = 8) {
  return {
    ...contextEvent(eventId, seq),
    actor: { participant_id: "agent-1", participant_type: "agent" },
    participant_id: "agent-1",
    participant_type: "agent",
    actor_id: "agent-1",
    actor_type: "agent",
    display_name: "Terra",
    content: "agent answer",
    session_id: "agent-1",
    turn_id: "turn-1",
    source_event_id: "event-1",
    target_agent_id: "agent-1",
    message_source: "room_portal",
  };
}

function roomToolContextEvent(eventId = "event-tool", seq = 9) {
  return {
    ...contextEvent(eventId, seq),
    actor: { participant_id: "room-system", participant_type: "system" },
    participant_id: "room-system",
    participant_type: "system",
    actor_id: "room-system",
    actor_type: "system",
    display_name: "주사위 결과",
    content: "Operator · 2d6+1 → 8 (굴림: 2, 5)",
    message_kind: "system",
    message_source: "room_tool_result",
    room_result_id: `result-${"e".repeat(32)}`,
    room_result_kind: "dice_roll",
    operation: "roll_dice",
    source_turn_id: "turn-1",
    source_participant_id: "operator-local",
    details: { notation: "2d6+1", rolls: [2, 5], modifier: 1, total: 8 },
  };
}

async function localSearchResponse(payload: unknown, privateResponse = true) {
  bridge.read.mockResolvedValueOnce(grant("a".repeat(64)));
  vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(
    jsonResponse(payload, privateResponse)
  ));
  return searchRoomMessages({
    roomId: "general",
    channelId: "lobby",
    query: "canonical",
    authority: { kind: "local" },
  });
}

async function localContextResponse(payload: unknown) {
  bridge.read.mockResolvedValueOnce(grant("a".repeat(64)));
  vi.stubGlobal("fetch", vi.fn().mockResolvedValueOnce(jsonResponse(payload)));
  return fetchRoomMessageContext({
    roomId: "general",
    channelId: "lobby",
    eventId: "event-1",
    authority: { kind: "local" },
  });
}

describe("lobby message-search HTTP authority", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses a fresh local one-use grant for search and context", async () => {
    bridge.read
      .mockResolvedValueOnce(grant("a".repeat(64)))
      .mockResolvedValueOnce(grant("b".repeat(64)));
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ results: [result()], next_cursor: "" }))
      .mockResolvedValueOnce(jsonResponse({
        channel_id: "lobby",
        event_id: "event-agent",
        events: [contextEvent(), agentContextEvent(), roomToolContextEvent()],
      }));
    vi.stubGlobal("fetch", fetchMock);

    const page = await searchRoomMessages({
      roomId: "general",
      channelId: "all",
      query: "canonical",
      authority: { kind: "local" },
    });
    const context = await fetchRoomMessageContext({
      roomId: "general",
      channelId: "lobby",
      eventId: "event-agent",
      authority: { kind: "local" },
    });

    expect(page.results).toEqual([result()]);
    expect(context.events.map((event) => event.id)).toEqual([
      "event-1",
      "event-agent",
      "event-tool",
    ]);
    expect(bridge.read).toHaveBeenNthCalledWith(1, "general");
    expect(bridge.read).toHaveBeenNthCalledWith(2, "general");
    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      "http://127.0.0.1:49154/api/room-search?room_id=general&channel_id=all&q=canonical"
    );
    expect(fetchMock.mock.calls[1]?.[0]).toBe(
      "http://127.0.0.1:49154/api/room-search/context?room_id=general&channel_id=lobby&event_id=event-agent"
    );
    expect((fetchMock.mock.calls[0]?.[1]?.headers as HeadersInit)).toEqual({
      Authorization: `Bearer ${"a".repeat(64)}`,
    });
    expect((fetchMock.mock.calls[1]?.[1]?.headers as HeadersInit)).toEqual({
      Authorization: `Bearer ${"b".repeat(64)}`,
    });
  });

  it("exchanges the remote session for the exact read purpose before every request", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(jsonResponse({ ticket: "c".repeat(64), ttl_seconds: 30 }))
      .mockResolvedValueOnce(jsonResponse({ results: [], next_cursor: "" }))
      .mockResolvedValueOnce(jsonResponse({ ticket: "d".repeat(64), ttl_seconds: 30 }))
      .mockResolvedValueOnce(jsonResponse({
        channel_id: "lobby",
        event_id: "event-1",
        events: [contextEvent()],
      }));
    vi.stubGlobal("fetch", fetchMock);
    const authority = { kind: "remote", sessionToken: "aas1.session" } as const;

    await searchRoomMessages({
      roomId: "general",
      channelId: "lobby",
      query: "missing",
      authority,
    });
    await fetchRoomMessageContext({
      roomId: "general",
      channelId: "lobby",
      eventId: "event-1",
      authority,
    });

    for (const index of [0, 2]) {
      expect(fetchMock).toHaveBeenNthCalledWith(
        index + 1,
        "/api/session-tickets/message-search-read",
        {
          cache: "no-store",
          method: "POST",
          headers: { Authorization: "Bearer aas1.session" },
        }
      );
    }
    expect(JSON.stringify(fetchMock.mock.calls[1]?.[1])).not.toContain("aas1.session");
    expect(JSON.stringify(fetchMock.mock.calls[3]?.[1])).not.toContain("aas1.session");
  });

  it("accepts empty optional provenance emitted by current message producers", async () => {
    const context = await localContextResponse({
      channel_id: "lobby",
      event_id: "event-1",
      events: [
        contextEvent(),
        { ...agentContextEvent(), target_agent_id: "" },
        { ...roomToolContextEvent(), source_turn_id: "" },
      ],
    });

    expect(context.events.map((event) => event.id)).toEqual([
      "event-1",
      "event-agent",
      "event-tool",
    ]);
  });

  it("accepts canonical newest-first nanosecond ordering", async () => {
    const results = [
      { ...result("event-3", 9), created_at: "2026-08-29T01:00:01.000000001Z" },
      { ...result("event-2", 8), created_at: "2026-08-29T01:00:01Z" },
      { ...result("event-1", 7), created_at: "2026-08-29T01:00:00.999999999Z" },
    ];

    await expect(localSearchResponse({ results, next_cursor: "" })).resolves.toEqual({
      results,
      next_cursor: "",
    });
  });

  it("rejects malformed complete pages rather than projecting permissive defaults", async () => {
    const malformed = [
      { results: [result()], next_cursor: "", ignored: true },
      { results: Array.from({ length: 31 }, (_, index) => result(`event-${index}`, index + 1)), next_cursor: "" },
      { results: [result()], next_cursor: "cursor" },
      { results: [result(), result()], next_cursor: "" },
      { results: [result("event-1", 7), result("event-2", 8)], next_cursor: "" },
      {
        results: [
          result("event-1", 8),
          { ...result("event-2", 7), created_at: "2026-08-29T02:00:00Z" },
        ],
        next_cursor: "",
      },
      { results: [{ ...result(), attachment_filenames: ["../unsafe"] }], next_cursor: "" },
      { results: [{ ...result(), content: "\ud800" }], next_cursor: "" },
    ];
    for (const payload of malformed) {
      await expect(localSearchResponse(payload)).rejects.toThrow("응답 계약");
    }
    await expect(localSearchResponse(
      { results: [result()], next_cursor: "" },
      false
    )).rejects.toThrow("응답 계약");
  });

  it("rejects private, crossed, duplicate, and unbounded context before use", async () => {
    const malformed = [
      { channel_id: "lobby", event_id: "event-1", events: [{ ...contextEvent(), provider_turn_id: "private" }] },
      { channel_id: "lobby", event_id: "other", events: [contextEvent()] },
      { channel_id: "lobby", event_id: "event-1", events: [contextEvent(), contextEvent()] },
      { channel_id: "lobby", event_id: "event-1", events: [contextEvent("event-2", 8), contextEvent()] },
      {
        channel_id: "lobby",
        event_id: "event-1",
        events: Array.from({ length: 32 }, (_, index) => contextEvent(index === 0 ? "event-1" : `event-${index}`, index + 1)),
      },
      {
        channel_id: "lobby",
        event_id: "event-1",
        events: Array.from({ length: 31 }, (_, index) => contextEvent(index === 0 ? "event-1" : `event-${index}`, index + 1)),
      },
    ];
    for (const payload of malformed) {
      await expect(localContextResponse(payload)).rejects.toThrow("응답 계약");
    }
  });
});
