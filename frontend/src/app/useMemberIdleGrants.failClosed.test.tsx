import { afterEach, describe, expect, it, vi } from "vitest";
import {
  bindRemoteSession,
  closeRemoteWorkspace,
  installRemoteWorkspace,
  remoteSessionTransport,
  unbindRemoteSessionsForServer,
} from "../lib/remote/remoteWorkspace";
import { fetchRoomSettings, getWsTicket, saveRoomSettings } from "../api/room";

const FRIEND_ORIGIN = "https://friend-same-origin.test";
const SERVER = "server-same-origin";

function fakeTransport() {
  const listeners = new Set<() => void>();
  const state = { active: true };
  const transport = {
    hello: { origin: FRIEND_ORIGIN, server_id: SERVER },
    get active() {
      return state.active;
    },
    fetch: vi.fn(async () => new Response("unexpected")),
    onClose: (listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    close: vi.fn(() => {
      state.active = false;
      for (const listener of [...listeners]) listener();
      listeners.clear();
    }),
  };
  return transport;
}

describe("member transport close and same-origin re-attach never leaks to the local origin", () => {
  afterEach(() => {
    closeRemoteWorkspace();
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("stale friend bearer fails closed across a same-origin re-attach (web)", async () => {
    closeRemoteWorkspace();
    const localOrigin = window.location.origin;
    // Live friend channel with a bound room bearer (the open room).
    const oldChannel = fakeTransport();
    installRemoteWorkspace({
      transport: oldChannel as never,
      owner: { sessionToken: "owner-old", serverId: SERVER } as never,
      deviceToken: "device",
      clientId: "client",
    });
    bindRemoteSession("friend-room-old", oldChannel as never, SERVER);
    expect(remoteSessionTransport("friend-room-old")).toBe(oldChannel);

    // The secure channel transport dies while idle (the tunnel close the
    // keepalive now prevents): the binding keeps the host origin but loses
    // its live transport, then the member host drops the stale workspace
    // sessions exactly like useMemberHosts does on re-attach.
    oldChannel.close();
    expect(remoteSessionTransport("friend-room-old")).toBeUndefined();
    closeRemoteWorkspace(SERVER);
    unbindRemoteSessionsForServer(SERVER);

    // Re-attach on the SAME origin with a fresh channel and a fresh bearer;
    // the open room still holds the stale token until its re-select lands.
    const freshChannel = fakeTransport();
    installRemoteWorkspace({
      transport: freshChannel as never,
      owner: { sessionToken: "owner-fresh", serverId: SERVER } as never,
      deviceToken: "device",
      clientId: "client",
    });
    bindRemoteSession("friend-room-fresh", freshChannel as never, SERVER);

    const fetchMock = vi.fn(async () => new Response("local-leak"));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      fetchRoomSettings("room-a", { sessionToken: "friend-room-old" })
    ).rejects.toThrow("연결이 끝났어요");
    await expect(
      saveRoomSettings({
        roomId: "room-a",
        roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
        appearance: { notifications: "mute" },
        identity: { sessionToken: "friend-room-old" },
      })
    ).rejects.toThrow("연결이 끝났어요");
    await expect(
      getWsTicket({ kind: "session", sessionToken: "friend-room-old" })
    ).rejects.toThrow("연결이 끝났어요");

    // No attempt at the local origin at any point: every failure above
    // rejected before a local fetch, and no request targeted this origin.
    for (const call of fetchMock.mock.calls as unknown[][]) {
      const url = String(call[0]);
      const absolute = new URL(url, localOrigin).href;
      expect(absolute.startsWith(localOrigin)).toBe(false);
    }
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("stale friend bearer fails closed across a same-origin re-attach (desktop)", async () => {
    closeRemoteWorkspace();
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    const oldChannel = fakeTransport();
    installRemoteWorkspace({
      transport: oldChannel as never,
      owner: { sessionToken: "owner-old", serverId: SERVER } as never,
      deviceToken: "device",
      clientId: "client",
    });
    bindRemoteSession("friend-room-old", oldChannel as never, SERVER);

    oldChannel.close();
    closeRemoteWorkspace(SERVER);
    unbindRemoteSessionsForServer(SERVER);

    const freshChannel = fakeTransport();
    installRemoteWorkspace({
      transport: freshChannel as never,
      owner: { sessionToken: "owner-fresh", serverId: SERVER } as never,
      deviceToken: "device",
      clientId: "client",
    });
    bindRemoteSession("friend-room-fresh", freshChannel as never, SERVER);

    const fetchMock = vi.fn(async () => new Response("local-leak"));
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      fetchRoomSettings("room-a", { sessionToken: "friend-room-old" })
    ).rejects.toThrow("연결이 끝났어요");
    await expect(
      saveRoomSettings({
        roomId: "room-a",
        roomUid: "a53a3f5c-0e7b-4de1-a70c-8f548e03e90c",
        appearance: { notifications: "mute" },
        identity: { sessionToken: "friend-room-old" },
      })
    ).rejects.toThrow("연결이 끝났어요");
    await expect(
      getWsTicket({ kind: "session", sessionToken: "friend-room-old" })
    ).rejects.toThrow("연결이 끝났어요");

    // Desktop never reaches the local host ticket or a relative fetch for
    // a presented session: no runtime ticket and no fetch at all.
    expect(invoke).not.toHaveBeenCalled();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
