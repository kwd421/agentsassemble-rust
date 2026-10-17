import { beforeEach, describe, expect, it, vi } from "vitest";
import { attachMemberHost, detachMemberHost, MemberAnchorMismatchError } from "./memberAutoAttach";
import { loadMemberAnchor, saveMemberAnchor } from "./memberAnchors";

const server = { server_id: "server", registration_epoch: "epoch" };
const target = {
  server_id: "server", registration_epoch: "epoch", origin: "https://host.test", generation: 3,
  host_public_key_jwk: {}, host_key_fingerprint: "fingerprint",
};
const rooms = [{ room_id: "general", name: "일반" }];

function depsWith(outcome: { rooms?: { room_id: string; name: string }[] | null; error?: unknown }) {
  const close = vi.fn();
  const admission = { admit: vi.fn().mockImplementation(async () => {
    if (outcome.error) throw outcome.error;
    return outcome.rooms ?? rooms;
  }), close, select: vi.fn(), transport: { onClose: vi.fn() } };
  return {
    close,
    deps: {
      preview: vi.fn().mockResolvedValue({ target, label: "Server", endpoint_origin: target.origin, endpoint_generation: 3, server_id: "server" }),
      open: vi.fn().mockResolvedValue(admission),
    },
  };
}

describe("member auto-attach", () => {
  beforeEach(() => { window.localStorage.clear(); window.sessionStorage.clear(); });

  it("re-admits with a fresh grant and persists only the display anchor", async () => {
    const { deps } = depsWith({});
    const attached = await attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never);
    expect(attached.rooms).toEqual(rooms);
    // The select deadline owns the signal: it reaches the central preview.
    expect(deps.preview).toHaveBeenCalledWith(server, expect.any(AbortSignal));
    const anchor = loadMemberAnchor("person", "server");
    expect(anchor).toMatchObject({
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://host.test", endpoint_generation: 3, rooms, hidden: false,
    });
    const raw = window.localStorage.getItem("agentsassemble.memberAnchors.v1:person") || "";
    expect(raw).not.toMatch(/grant|session_token|private|aad1_/i);
    for (const storage of [window.localStorage, window.sessionStorage]) {
      for (let index = 0; index < storage.length; index += 1) {
        const value = storage.getItem(storage.key(index) || "") || "";
        expect(value).not.toMatch(/aam[cg]1\./);
      }
    }
    detachMemberHost(attached.admission);
  });

  it("re-attaches silently when only the endpoint origin/generation changed", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://old.test", endpoint_generation: 2, rooms, hidden: false, updated_at: 1,
    });
    const { deps } = depsWith({});
    const attached = await attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never);
    expect(attached.rooms).toEqual(rooms);
    // The anchor pins identity only; the fresh origin/generation is adopted.
    expect(loadMemberAnchor("person", "server")).toMatchObject({
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://host.test", endpoint_generation: 3,
    });
    detachMemberHost(attached.admission);
  });

  it("refuses a key mismatch and keeps the stale anchor for the disconnected view", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "old-key",
      endpoint_origin: "https://host.test", endpoint_generation: 2, rooms, hidden: false, updated_at: 1,
    });
    const { deps } = depsWith({});
    await expect(attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never))
      .rejects.toBeInstanceOf(MemberAnchorMismatchError);
    expect(loadMemberAnchor("person", "server")).not.toBeNull();
    expect(loadMemberAnchor("person", "server")?.rooms).toEqual(rooms);
  });

  it("refuses an epoch mismatch before any preview and keeps the stale anchor", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "old", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://host.test", endpoint_generation: 2, rooms, hidden: false, updated_at: 1,
    });
    const { deps } = depsWith({});
    await expect(attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never))
      .rejects.toBeInstanceOf(MemberAnchorMismatchError);
    expect(deps.preview).not.toHaveBeenCalled();
    expect(loadMemberAnchor("person", "server")).not.toBeNull();
    expect(loadMemberAnchor("person", "server")?.rooms).toEqual(rooms);
  });

  it("closes the transport when admission fails without persisting rooms", async () => {
    const { close, deps } = depsWith({ error: new Error("꺼져 있어요.") });
    await expect(attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never))
      .rejects.toThrow("꺼져 있어요.");
    expect(close).toHaveBeenCalledOnce();
    expect(loadMemberAnchor("person", "server")).toBeNull();
  });

  it("aborts a hung central preview instead of stranding the attach", async () => {
    const { deps } = depsWith({});
    deps.preview.mockImplementation((_entry: unknown, signal?: AbortSignal) =>
      new Promise((_resolve, reject) => {
        signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
      }));
    const controller = new AbortController();
    const pending = attachMemberHost(server, "central-token", "person", controller.signal, deps as never);
    controller.abort(new Error("참가 서버의 응답이 없어요. 다시 시도해 주세요."));
    await expect(pending).rejects.toThrow("참가 서버의 응답이 없어요. 다시 시도해 주세요.");
    expect(deps.open).not.toHaveBeenCalled();
    expect(loadMemberAnchor("person", "server")).toBeNull();
  });

  it("keeps the hidden flag of an existing anchor", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://host.test", endpoint_generation: 2, rooms: [], hidden: true, updated_at: 1,
    });
    const { deps } = depsWith({});
    await attachMemberHost(server, "central-token", "person", new AbortController().signal, deps as never);
    expect(loadMemberAnchor("person", "server")?.hidden).toBe(true);
    expect(loadMemberAnchor("person", "server")?.rooms).toEqual(rooms);
  });
});
