import { beforeEach, describe, expect, it, vi } from "vitest";
import { attachMemberHost } from "./memberAutoAttach";
import { loadMemberAnchor, saveMemberAnchor } from "./memberAnchors";
import { memberHostOfflineBudgetExhausted, resetMemberHostOfflineBudgetForTests } from "./memberRoomLive";

const rooms = [{ room_id: "general", name: "일반" }];
const server = { server_id: "server", registration_epoch: "epoch" };

function anchor(overrides: Record<string, unknown> = {}) {
  return {
    server_id: "server",
    registration_epoch: "epoch",
    host_key_fingerprint: "fingerprint",
    endpoint_origin: "https://host.test",
    endpoint_generation: 3,
    rooms,
    hidden: false,
    updated_at: 1,
    ...overrides,
  };
}

function probeHit(overrides: Record<string, unknown> = {}) {
  return {
    server_id: "server",
    host_key_fingerprint: "fingerprint",
    host_public_key_jwk: {},
    ...overrides,
  };
}

describe("the central-free member reconnect", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    resetMemberHostOfflineBudgetForTests();
  });

  it("reaches the joined host with the resume key and spends no central request", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn();
    const resumeDirect = vi.fn().mockResolvedValue(rooms);
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn();
    const open = vi.fn().mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    const attached = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(attached.rooms).toEqual(rooms);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
    expect(admit).not.toHaveBeenCalled();
    expect(preview).not.toHaveBeenCalled();
    expect(fetcher).not.toHaveBeenCalled();
    // The daily preview budget of the host does not move either.
    expect(memberHostOfflineBudgetExhausted("person", "server")).toBe(false);
    expect(loadMemberAnchor("person", "server")).toMatchObject({
      endpoint_origin: "https://host.test",
      endpoint_generation: 3,
      rooms,
    });
  });

  it("falls back to one central grant when the stored key is expired", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn().mockResolvedValue(rooms);
    const resumeDirect = vi.fn().mockRejectedValue(new Error("이 기기는 아직 재연결 자격 증명이 없어요."));
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn();
    const open = vi.fn().mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const attached = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(attached.rooms).toEqual(rooms);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(preview).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("spends one preview and one grant on a generation bump and none on the next blip", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn().mockResolvedValue(rooms);
    const resumeDirect = vi.fn().mockResolvedValue(rooms);
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn().mockResolvedValue({
      target: {
        server_id: "server",
        registration_epoch: "epoch",
        origin: "https://moved.test",
        generation: 4,
        host_key_fingerprint: "fingerprint",
        host_public_key_jwk: {},
      },
    });
    // The stored generation is stale: the handshake fails and the central
    // path learns the fresh origin.
    const open = vi.fn().mockRejectedValueOnce(new Error("서버 주소가 변경됐어요.")).mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const first = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(first.rooms).toEqual(rooms);
    expect(preview).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(loadMemberAnchor("person", "server")?.endpoint_origin).toBe("https://moved.test");
    // The next blip on the updated anchor resumes without any central call.
    const moved = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(moved.rooms).toEqual(rooms);
    expect(preview).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
  });

  it("keeps the refusal when the stored incarnation no longer matches", async () => {
    saveMemberAnchor("person", anchor());
    const preview = vi.fn();
    const open = vi.fn();
    const probe = vi.fn().mockResolvedValue(probeHit());
    await expect(
      attachMemberHost(
        { server_id: "server", registration_epoch: "other-epoch" },
        "central-token",
        "person",
        new AbortController().signal,
        { preview: preview as never, open: open as never, probe: probe as never },
      ),
    ).rejects.toThrow("참가 서버 정보가 바뀌었어요.");
    expect(preview).not.toHaveBeenCalled();
    expect(open).not.toHaveBeenCalled();
    expect(loadMemberAnchor("person", "server")).not.toBeNull();
  });
});
