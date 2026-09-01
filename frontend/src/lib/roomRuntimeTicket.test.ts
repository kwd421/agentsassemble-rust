import { describe, expect, it } from "vitest";

import {
  parseNativeRoomRuntimeTicket,
  requireAcceptedRoomRuntimeTicket,
} from "./roomRuntimeTicket";

const NATIVE_TICKET = {
  ticket: "a".repeat(64),
  ttl_seconds: 30,
  websocket_base_url: "ws://127.0.0.1:43123",
};

describe("room runtime ticket authority", () => {
  it("derives one immutable display origin from an exact native grant", () => {
    expect(parseNativeRoomRuntimeTicket(NATIVE_TICKET)).toEqual({
      ...NATIVE_TICKET,
      displayResourceBase: "http://127.0.0.1:43123",
    });
  });

  it.each([
    ["extra key", { ...NATIVE_TICKET, extra: true }],
    ["coercible ticket", { ...NATIVE_TICKET, ticket: [NATIVE_TICKET.ticket] }],
    ["zero TTL", { ...NATIVE_TICKET, ttl_seconds: 0 }],
    ["uppercase ticket", { ...NATIVE_TICKET, ticket: "A".repeat(64) }],
    ["TLS socket", { ...NATIVE_TICKET, websocket_base_url: "wss://127.0.0.1:43123" }],
    ["hostname alias", { ...NATIVE_TICKET, websocket_base_url: "ws://localhost:43123" }],
    ["path", { ...NATIVE_TICKET, websocket_base_url: "ws://127.0.0.1:43123/ws" }],
    ["query", { ...NATIVE_TICKET, websocket_base_url: "ws://127.0.0.1:43123?x=1" }],
    ["fragment", { ...NATIVE_TICKET, websocket_base_url: "ws://127.0.0.1:43123#x" }],
    ["credentials", { ...NATIVE_TICKET, websocket_base_url: "ws://u@127.0.0.1:43123" }],
    ["alternate serialization", { ...NATIVE_TICKET, websocket_base_url: "ws://127.0.0.1:43123/" }],
  ])("rejects %s before transport use", (_case, value) => {
    expect(() => parseNativeRoomRuntimeTicket(value)).toThrow();
  });

  it("rejects an accepted ticket whose socket and display origins differ", () => {
    expect(() =>
      requireAcceptedRoomRuntimeTicket({
        ...parseNativeRoomRuntimeTicket(NATIVE_TICKET),
        displayResourceBase: "http://127.0.0.1:43124",
      })
    ).toThrow("do not match");
  });
});
