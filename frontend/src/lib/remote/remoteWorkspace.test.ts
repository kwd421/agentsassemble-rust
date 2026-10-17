import { afterEach, describe, expect, it, vi } from "vitest";
import { bindRemoteSession, closeRemoteWorkspace, fetchProductTransport, installRemoteWorkspace, remoteSessionTransport, remoteWorkspaceFor, remoteWorkspacesSnapshot, unbindRemoteSessionsForServer } from "./remoteWorkspace";
import type { RemoteTransport } from "./remoteTransport";

afterEach(() => { closeRemoteWorkspace(); vi.unstubAllGlobals(); });
it("routes only the admitted credential and never downgrades it after channel/workspace close", async () => {
  const plain = vi.fn(); vi.stubGlobal("fetch", plain);
  const listeners = new Set<() => void>();
  const remote = { hello: { origin: "https://host.test" }, active: true, fetch: vi.fn().mockResolvedValue(new Response("encrypted")),
    onClose: (fn: () => void) => { listeners.add(fn); return () => listeners.delete(fn); },
    close: () => { remote.active = false; for (const listener of listeners) listener(); },
  };
  const transport = remote as unknown as RemoteTransport;
  installRemoteWorkspace({ transport, owner: { sessionToken: "owner", serverId: "host" } as never, deviceToken: "device", clientId: "client" });
  bindRemoteSession("room", transport);
  await fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer room" } });
  expect(remote.fetch).toHaveBeenCalledOnce();
  await expect(fetchProductTransport("/api/rooms")).rejects.toThrow();
  await expect(fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer other" } })).rejects.toThrow();
  remote.close(); closeRemoteWorkspace();
  await expect(fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer room" } })).rejects.toThrow();
  expect(plain).not.toHaveBeenCalled();
});

function hostTransport(serverId: string) {
  const listeners = new Set<() => void>();
  const remote = { hello: { origin: `https://${serverId}.test`, server_id: serverId }, active: true,
    fetch: vi.fn().mockResolvedValue(new Response("encrypted")),
    onClose: (fn: () => void) => { listeners.add(fn); return () => listeners.delete(fn); },
    close: () => { remote.active = false; for (const listener of listeners) listener(); },
  };
  return remote;
}

function installHost(serverId: string, token: string) {
  const remote = hostTransport(serverId);
  installRemoteWorkspace({ transport: remote as unknown as RemoteTransport,
    owner: { sessionToken: token, serverId } as never, deviceToken: "device", clientId: "client" });
  return remote;
}

describe("per-host session registry", () => {
  it("keeps two hosts live and never sends one host's bearer to the other", async () => {
    const plain = vi.fn(); vi.stubGlobal("fetch", plain);
    const hostA = installHost("server-a", "bearer-a");
    const hostB = installHost("server-b", "bearer-b");
    expect(remoteWorkspacesSnapshot()).toHaveLength(2);
    expect(remoteWorkspaceFor("server-a")?.owner?.sessionToken).toBe("bearer-a");

    await fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer bearer-a" } });
    await fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer bearer-b" } });
    expect(hostA.fetch).toHaveBeenCalledOnce();
    expect(hostB.fetch).toHaveBeenCalledOnce();
    await expect(fetchProductTransport("/api/rooms", { headers: { Authorization: "Bearer unknown" } })).rejects.toThrow();
    expect(plain).not.toHaveBeenCalled();
  });

  it("closing one host fails only its bearer closed while the other stays live", async () => {
    const hostA = installHost("server-a", "bearer-a");
    installHost("server-b", "bearer-b");
    closeRemoteWorkspace("server-a");
    expect(hostA.active).toBe(false);
    expect(remoteWorkspaceFor("server-a")).toBeNull();
    await expect(fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer bearer-a" } })).rejects.toThrow();
    await fetchProductTransport("/api/room-pins", { headers: { Authorization: "Bearer bearer-b" } });
  });

  it("reinstalling a host closes only that host's previous transport", () => {
    const oldA = installHost("server-a", "bearer-a");
    const hostB = installHost("server-b", "bearer-b");
    const newA = installHost("server-a", "bearer-a2");
    expect(oldA.active).toBe(false);
    expect(hostB.active).toBe(true);
    expect(newA.active).toBe(true);
    expect(remoteWorkspacesSnapshot()).toHaveLength(2);
  });

  it("refuses an install whose transport advertises another server, or no server at all", () => {
    const foreign = hostTransport("server-foreign");
    expect(() => installRemoteWorkspace({ transport: foreign as unknown as RemoteTransport,
      owner: { sessionToken: "bearer-a", serverId: "server-a" } as never, deviceToken: "device", clientId: "client" }))
      .toThrow("선택한 서버와 연결이 일치하지 않습니다.");
    expect(remoteWorkspacesSnapshot()).toHaveLength(0);
    const vague = hostTransport("");
    expect(() => installRemoteWorkspace({ transport: vague as unknown as RemoteTransport,
      owner: null, deviceToken: "device", clientId: "client" }))
      .toThrow("서버를 확인하지 못했어요");
  });

  it("unbinding one host drops only its bearers", () => {
    installHost("server-a", "bearer-a");
    installHost("server-b", "bearer-b");
    unbindRemoteSessionsForServer("server-a");
    expect(remoteSessionTransport("bearer-a")).toBeUndefined();
    expect(remoteSessionTransport("bearer-b")).toBeTruthy();
  });
});
