import { afterEach, expect, it, vi } from "vitest";
import { flushPromises, handshakeFrames, openHarness } from "./test/roomSocketHarness";
import { pendingRequest } from "./test/providerRequest";

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("admits all request bodies together only after completion and resets partial reconnect state", async () => {
  vi.useFakeTimers();
  const onRoomSnapshot = vi.fn();
  const { handle, sockets } = openHarness({ onRoomSnapshot });
  await flushPromises(); sockets[0].open();
  const first = handshakeFrames(0, 0);
  sockets[0].receive(first.receipt); sockets[0].receive(first.catalog);
  const body = { ...pendingRequest, request: { ...pendingRequest.request, description: "가".repeat(1_200) } };
  sockets[0].receive({ op: "provider_request_snapshot", request: body });
  expect(handle.ready()).toBe(false); expect(onRoomSnapshot).not.toHaveBeenCalled();
  sockets[0].close(); await vi.advanceTimersByTimeAsync(500); await flushPromises(); sockets[1].open();
  const next = handshakeFrames(0, 0);
  sockets[1].receive(next.receipt); sockets[1].receive(next.catalog);
  // The same ID must not collide with the abandoned partial admission.
  sockets[1].receive({ op: "provider_request_snapshot", request: body });
  sockets[1].receive(next.requestsEnd);
  expect(handle.ready()).toBe(false); expect(onRoomSnapshot).not.toHaveBeenCalled();
  sockets[1].receiveRaw(next.rawSnapshot); await flushPromises();
  expect(handle.ready()).toBe(true);
  expect(onRoomSnapshot).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ provider_requests: [body] }), expect.any(String));
  handle.close();
});

it.each(["missing_end", "invalid", "duplicate", "after_end"])("rejects %s request admission without exposing a partial room", async (kind) => {
  const onRoomSnapshot = vi.fn(); const onError = vi.fn();
  const { handle, sockets } = openHarness({ onRoomSnapshot, onError });
  await flushPromises(); sockets[0].open();
  const frames = handshakeFrames(0, 0);
  sockets[0].receive(frames.receipt); sockets[0].receive(frames.catalog);
  const request = { op: "provider_request_snapshot", request: pendingRequest };
  if (kind === "after_end") sockets[0].receive(frames.requestsEnd);
  sockets[0].receive(kind === "invalid" ? { ...request, request: { ...pendingRequest, private_field: true } } : request);
  if (kind === "duplicate") sockets[0].receive(request);
  if (kind === "missing_end") sockets[0].receiveRaw(frames.rawSnapshot);
  await flushPromises();
  expect(onError).toHaveBeenCalled(); expect(onRoomSnapshot).not.toHaveBeenCalled();
  expect(handle.ready()).toBe(false); handle.close();
});
