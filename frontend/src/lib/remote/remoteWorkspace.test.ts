import { afterEach, expect, it, vi } from "vitest";
import { bindRemoteSession, closeRemoteWorkspace, fetchProductTransport, installRemoteWorkspace } from "./remoteWorkspace";
import type { RemoteTransport } from "./remoteTransport";

afterEach(() => { closeRemoteWorkspace(); vi.unstubAllGlobals(); });
it("routes only the admitted credential and never downgrades it after channel/workspace close", async () => {
  const plain = vi.fn(); vi.stubGlobal("fetch", plain);
  const listeners = new Set<() => void>();
  const remote = { active: true, fetch: vi.fn().mockResolvedValue(new Response("encrypted")),
    onClose: (fn: () => void) => { listeners.add(fn); return () => listeners.delete(fn); },
    close: () => { remote.active = false; for (const listener of listeners) listener(); },
  };
  const transport = remote as unknown as RemoteTransport;
  installRemoteWorkspace({ transport, owner: { sessionToken: "owner" } as never, deviceToken: "device", clientId: "client" });
  bindRemoteSession("room", transport);
  await fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer room" } });
  expect(remote.fetch).toHaveBeenCalledOnce();
  await expect(fetchProductTransport("/api/rooms")).rejects.toThrow();
  await expect(fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer other" } })).rejects.toThrow();
  remote.close(); closeRemoteWorkspace();
  await expect(fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer room" } })).rejects.toThrow();
  expect(plain).not.toHaveBeenCalled();
});
