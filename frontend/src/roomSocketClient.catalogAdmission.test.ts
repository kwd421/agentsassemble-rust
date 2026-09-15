import { afterEach, expect, it, vi } from "vitest";
import { flushPromises, handshakeFrames, openHarness } from "./test/roomSocketHarness";

afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("admits a complete room only after its catalog, snapshot and catch-up arrive", async () => {
  const onRoomSnapshot = vi.fn();
  const onProviderCatalog = vi.fn();
  const { handle, sockets, opened } = openHarness({ onRoomSnapshot, onProviderCatalog });
  await flushPromises(); sockets[0].open();
  const first = handshakeFrames(0, 0);
  sockets[0].receive(first.receipt);
  expect(handle.ready()).toBe(false);
  expect(onRoomSnapshot).not.toHaveBeenCalled();
  sockets[0].receive(first.catalog); sockets[0].receive(first.requestsEnd);
  expect(handle.ready()).toBe(false);
  expect(onProviderCatalog).not.toHaveBeenCalled();
  expect(onRoomSnapshot).not.toHaveBeenCalled();
  sockets[0].receiveRaw(first.rawSnapshot);
  await opened;
  expect(handle.ready()).toBe(true);
  expect(onRoomSnapshot).toHaveBeenCalledWith(
    expect.objectContaining({ provider_catalog: first.catalog.catalog }), expect.any(String));
  const updated = { ...first.catalog, catalog: { ...first.catalog.catalog, catalog_revision: "cat-2" } };
  sockets[0].receive(updated);
  expect(onProviderCatalog).toHaveBeenCalledWith(updated.catalog);
  handle.close();
});

it.each(["missing", "invalid", "duplicate"])("rejects %s initial catalog without exposing a ready room", async (kind) => {
  const onRoomSnapshot = vi.fn(); const onOpen = vi.fn(); const onError = vi.fn();
  const { handle, sockets } = openHarness({ onRoomSnapshot, onOpen, onError });
  await flushPromises(); sockets[0].open();
  const frames = handshakeFrames(0, 0);
  sockets[0].receive(frames.receipt);
  if (kind === "missing") sockets[0].receiveRaw(frames.rawSnapshot);
  if (kind === "invalid") sockets[0].receive({ ...frames.catalog, private_field: "rejected" });
  if (kind === "duplicate") { sockets[0].receive(frames.catalog); sockets[0].receive(frames.requestsEnd); sockets[0].receive(frames.catalog); sockets[0].receive(frames.requestsEnd); }
  await flushPromises();
  expect(onError).toHaveBeenCalled(); expect(onOpen).not.toHaveBeenCalled();
  expect(onRoomSnapshot).not.toHaveBeenCalled(); expect(handle.ready()).toBe(false);
  handle.close();
});

it("requires the new catalog again on reconnect after a live catalog update", async () => {
  vi.useFakeTimers();
  const onRoomSnapshot = vi.fn();
  const { handle, sockets, opened } = openHarness({ onRoomSnapshot });
  await flushPromises(); sockets[0].open();
  const first = handshakeFrames(0, 0);
  sockets[0].receive(first.receipt); sockets[0].receive(first.catalog); sockets[0].receive(first.requestsEnd); sockets[0].receiveRaw(first.rawSnapshot);
  await opened;
  const updated = { ...first.catalog, catalog: { ...first.catalog.catalog, catalog_revision: "cat-2" } };
  sockets[0].receive(updated); sockets[0].close();
  await vi.advanceTimersByTimeAsync(500); await flushPromises(); sockets[1].open();
  const next = handshakeFrames(0, 0);
  sockets[1].receive(next.receipt);
  expect(handle.ready()).toBe(false);
  sockets[1].receive(updated); sockets[1].receive(next.requestsEnd);
  expect(handle.ready()).toBe(false);
  sockets[1].receiveRaw(next.rawSnapshot); await flushPromises();
  expect(handle.ready()).toBe(true);
  expect(onRoomSnapshot).toHaveBeenLastCalledWith(
    expect.objectContaining({ provider_catalog: updated.catalog }), expect.any(String));
  handle.close();
});
