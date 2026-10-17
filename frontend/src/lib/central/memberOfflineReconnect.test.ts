import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP,
  memberHostOfflineBudgetExhausted,
  memberHostReconnectDelay,
  noteMemberHostOfflinePreview,
  resetMemberHostOfflineBudgetForTests,
} from "./memberRoomLive";
import { attachMemberHost } from "./memberAutoAttach";
import { saveMemberAnchor } from "./memberAnchors";

const rooms = [{ room_id: "general", name: "일반" }];

function offlineError() {
  return Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" });
}

describe("offline reconnect budget", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    resetMemberHostOfflineBudgetForTests();
    vi.useRealTimers();
  });

  it("parks a down host within the daily cap over 6 hours", () => {
    vi.useFakeTimers();
    try {
      const start = Date.UTC(2026, 9, 17, 12, 0, 0);
      vi.setSystemTime(start);
      let now = start;
      let attempt = 0;
      let previews = 0;
      const end = start + 6 * 3_600_000;
      // Walk the slow schedule the way useMemberHosts does: each slot spends
      // one preview (409 while down), notes it, then waits the slow delay.
      // Once the budget is spent the host parks until midnight (no preview).
      while (now <= end) {
        if (memberHostOfflineBudgetExhausted("person", "host", now)) break;
        previews += 1;
        noteMemberHostOfflinePreview("person", "host", now);
        now += memberHostReconnectDelay(attempt, "offline", null, () => 0.5);
        attempt += 1;
      }
      expect(previews).toBeLessThanOrEqual(MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP);
      expect(previews).toBe(MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP);
      // 8 previews × 3 units = 24 units = 10% of the 240-unit allowance.
      expect(previews * 3).toBe(24);
      void offlineError();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reconnects on the same origin with zero member-preview calls", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://host.test", endpoint_generation: 3, rooms, hidden: false, updated_at: 1,
    });
    const close = vi.fn();
    const admission = {
      admit: vi.fn().mockResolvedValue(rooms),
      close, select: vi.fn(), transport: { onClose: vi.fn() },
    };
    const preview = vi.fn();
    const open = vi.fn().mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue({
      server_id: "server", host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
    });
    const attached = await attachMemberHost(
      { server_id: "server", registration_epoch: "epoch" },
      "central-token", "person", new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(probe).toHaveBeenCalledTimes(1);
    expect(probe).toHaveBeenCalledWith("https://host.test", expect.objectContaining({ server_id: "server" }));
    expect(preview).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledTimes(1);
    expect(attached.rooms).toEqual(rooms);
  });

  it("spends exactly one preview at the next slot when the host moved", async () => {
    saveMemberAnchor("person", {
      server_id: "server", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://old.test", endpoint_generation: 2, rooms, hidden: false, updated_at: 1,
    });
    const close = vi.fn();
    const admission = {
      admit: vi.fn().mockResolvedValue(rooms),
      close, select: vi.fn(), transport: { onClose: vi.fn() },
    };
    const preview = vi.fn().mockResolvedValue({
      target: {
        server_id: "server", registration_epoch: "epoch", origin: "https://new.test", generation: 4,
        host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
      },
    });
    const open = vi.fn().mockResolvedValue(admission);
    // Old origin answers nothing (down): the slot spends its single preview
    // to learn the new origin.
    const probe = vi.fn().mockResolvedValue(null);
    const attached = await attachMemberHost(
      { server_id: "server", registration_epoch: "epoch" },
      "central-token", "person", new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(probe).toHaveBeenCalledTimes(1);
    expect(preview).toHaveBeenCalledTimes(1);
    expect(open).toHaveBeenCalledTimes(1);
    expect(attached.anchor.endpoint_origin).toBe("https://new.test");
  });
});
