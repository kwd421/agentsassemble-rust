import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { memberRoomRemovedWhileLive, useMemberHosts } from "./useMemberHosts";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { saveMemberAnchor } from "../lib/central/memberAnchors";

const attach = vi.fn();
vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

const admissionFor = (rooms: { room_id: string; name: string }[]) => ({
  transport: { onClose: vi.fn() },
  close: vi.fn(),
  select: vi.fn(),
  rooms,
});

const liveServer = (overrides = {}) => ({
  server_id: "friend", registration_epoch: "epoch", relation: "member" as const, alias: "친구",
  host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
  endpoint: { origin: "https://friend.test", generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
  ...overrides,
});

const unconfirmedDirectory = (): CentralDirectoryState => ({
  status: "central-unconfirmed",
  person: { person_id: "person", display_name: "나", identity_kind: "google" },
  servers: [],
  live: null,
  error: undefined,
});

const directoryFor = (servers: ReturnType<typeof liveServer>[]) => ({
  status: "connected" as const,
  person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
  servers: servers.map(({ host_key_fingerprint: _key, host_public_key_jwk: _jwk, endpoint, ...rest }) => ({ ...rest })),
  live: { person: { person_id: "person", display_name: "나", identity_kind: "google" as const }, servers, server_time: 1 },
  error: undefined,
});

describe("useMemberHosts", () => {
  beforeEach(() => {
    // Unmount the previous hook first: otherwise its per-host reconnect
    // timer would fire into the next test's attach mock.
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    attach.mockReset();
  });

  it("attaches a listed member server on startup and merges its rooms", async () => {
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledOnce();
    expect(result.current.snapshots[0]).toMatchObject({ serverId: "friend", phase: "connected" });
    expect(result.current.hostDirs).toEqual([{
      serverId: "friend", serverOrigin: "https://friend.test",
      rooms: [{ room_id: "general", label: "일반" }], state: "connected",
    }]);
    expect(result.current.hostStates).toEqual([{ serverId: "friend", state: "connected" }]);
  });

  it("shows cached rooms disconnected when the host is offline", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    attach.mockRejectedValue(new Error("꺼져 있어요."));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("disconnected"), { timeout: 5000 });
    expect(result.current.snapshots[0]).toMatchObject({ serverId: "friend", phase: "disconnected" });
    expect(result.current.hostDirs[0]).toMatchObject({
      serverId: "friend", rooms: [{ room_id: "general", label: "일반" }], state: "disconnected",
    });
  });

  it("shows nothing for an offline host with no known rooms", async () => {
    attach.mockRejectedValue(new Error("꺼져 있어요."));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("disconnected"), { timeout: 5000 });
    expect(result.current.hostDirs[0]).toMatchObject({ serverId: "friend", rooms: null, state: "disconnected" });
  });

  it("keeps refused rooms visible as disconnected with a re-join reason and retains the anchor", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "old", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    const { result, rerender } = renderHook(({ directory }) => useMemberHosts({
      personId: "person", sessionToken: "token", directory,
    }), { initialProps: { directory: directoryFor([liveServer()]) } });
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("disconnected"), { timeout: 5000 });
    expect(attach).not.toHaveBeenCalled();
    expect(result.current.snapshots[0]).toMatchObject({ serverId: "friend", phase: "disconnected" });
    expect(result.current.snapshots[0]?.detail).toMatch(/새 초대/);
    expect(result.current.hostDirs[0]).toMatchObject({
      serverId: "friend", rooms: [{ room_id: "general", label: "일반" }], state: "disconnected",
    });
    // The stale anchor is kept as the refusal baseline; a retry still
    // refuses instead of silently adopting the new identity.
    expect(window.localStorage.getItem("agentsassemble.memberAnchors.v1:person")).not.toBeNull();
    rerender({ directory: directoryFor([liveServer()]) });
    await act(async () => {});
    expect(attach).not.toHaveBeenCalled();
    expect(result.current.snapshots[0]?.phase).toBe("disconnected");
  });

  it("re-attaches silently when only the endpoint origin changed", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://old.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledOnce();
    expect(result.current.hostDirs[0]).toMatchObject({ serverId: "friend", state: "connected" });
    expect(result.current.snapshots[0]?.detail).toBeNull();
  });

  it("removes rooms and the anchor when a live directory drops the membership", async () => {
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2, rooms: [], hidden: false, updated_at: 1,
    });
    const { result, rerender } = renderHook(({ directory }) => useMemberHosts({
      personId: "person", sessionToken: "token", directory,
    }), { initialProps: { directory: directoryFor([liveServer()]) } });
    await waitFor(() => expect(result.current.snapshots).toHaveLength(1), { timeout: 5000 });
    rerender({ directory: directoryFor([]) });
    await waitFor(() => expect(result.current.snapshots).toEqual([]), { timeout: 5000 });
    expect(window.localStorage.getItem("agentsassemble.memberAnchors.v1:person")).toBeNull();
    expect(result.current.hostDirs).toEqual([]);
  });

  it("selects from the saved anchor while central is unconfirmed", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    const admission = admissionFor([{ room_id: "general", name: "일반" }]);
    admission.select.mockResolvedValue({ meeting_id: "general" });
    attach.mockImplementation(async () => ({
      admission,
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: unconfirmedDirectory(),
    }));
    let selected: unknown;
    await act(async () => { selected = await result.current.selectRoom("friend", "general"); });
    // A rail click never waits on a central refresh: the anchor carries the
    // registration epoch while central is unconfirmed.
    expect(attach).toHaveBeenCalled();
    expect(admission.select).toHaveBeenCalledWith("general", expect.anything());
    expect(selected).toEqual({ meeting_id: "general" });
  });

  it("keeps a freshly attached host connected when central turns unconfirmed", async () => {
    let resolveAttach!: (attached: {
      admission: ReturnType<typeof admissionFor>;
      rooms: { room_id: string; name: string }[];
      anchor: { server_id: string; registration_epoch: string };
    }) => void;
    attach.mockImplementation(() => new Promise((resolve) => { resolveAttach = resolve; }));
    const { result, rerender } = renderHook(({ directory }: { directory: CentralDirectoryState }) => useMemberHosts({
      personId: "person", sessionToken: "token", directory,
    }), { initialProps: { directory: directoryFor([liveServer()]) as CentralDirectoryState } });
    await waitFor(() => expect(attach).toHaveBeenCalled(), { timeout: 5000 });
    rerender({ directory: unconfirmedDirectory() });
    const admission = admissionFor([{ room_id: "general", name: "일반" }]);
    await act(async () => {
      resolveAttach({
        admission,
        rooms: [{ room_id: "general", name: "일반" }],
        anchor: { server_id: "friend", registration_epoch: "epoch" },
      });
    });
    // Joined rooms stay from the last-known list with their own state; a
    // transient central blip never drops the host.
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(result.current.hostDirs[0]).toMatchObject({
      serverId: "friend", state: "connected", rooms: [{ room_id: "general", label: "일반" }],
    });
  });

  it("retries an offline host on explicit retry", async () => {
    attach.mockRejectedValueOnce(new Error("꺼져 있어요."));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("disconnected"), { timeout: 5000 });
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    act(() => { result.current.retryHost("friend"); });
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledTimes(2);
  });

  it("reconnects an offline host automatically with backoff", async () => {
    vi.useFakeTimers();
    try {
      attach.mockRejectedValue(new Error("꺼져 있어요."));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
      }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      const calls = attach.mock.calls.length;
      // No central polling: the retry re-admits on the host's own backoff
      // (transient first step: 1 s ± jitter, so 2 s always covers it).
      await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
      expect(attach.mock.calls.length).toBeGreaterThan(calls);
    } finally {
      vi.useRealTimers();
    }
  });

  it("reconnects after the host channel closes without a second transport", async () => {
    vi.useFakeTimers();
    try {
      const onClose = vi.fn();
      const admission = { ...admissionFor([{ room_id: "general", name: "일반" }]), transport: { onClose, active: true } };
      attach.mockImplementation(async () => ({
        admission,
        rooms: [{ room_id: "general", name: "일반" }],
        anchor: { server_id: "friend", registration_epoch: "epoch" },
      }));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
      }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("connected");
      expect(onClose).toHaveBeenCalledOnce();
      const closeHost = onClose.mock.calls[0][0] as () => void;
      await act(async () => { closeHost(); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      const calls = attach.mock.calls.length;
      // A live channel drop waits out the fast origin-lookup slot (15-30 s)
      // so a restarting host has time to publish its new origin; nothing
      // fires inside the first 14 s.
      await act(async () => { await vi.advanceTimersByTimeAsync(14_000); });
      expect(attach.mock.calls.length).toBe(calls);
      await act(async () => { await vi.advanceTimersByTimeAsync(17_000); });
      expect(attach.mock.calls.length).toBeGreaterThan(calls);
    } finally {
      vi.useRealTimers();
    }
  });

  it("holds a revoked endpoint to a slow cadence instead of storming central", async () => {
    vi.useFakeTimers();
    try {
      // Central revoked the endpoint row (host turned external access off):
      // every preview answers 409 incarnation_conflict.
      attach.mockRejectedValue(Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" }));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
      }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(attach).toHaveBeenCalledTimes(1);
      // No per-second retry: the slow cadence never fires within 5 s.
      await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });
      expect(attach).toHaveBeenCalledTimes(1);
      // Ten revoked minutes cost at most 5 central previews (slow doubling
      // 60 s → 5 min cap with jitter), never a per-second storm.
      for (let minute = 0; minute < 10; minute += 1) {
        await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      }
      expect(attach.mock.calls.length).toBeLessThanOrEqual(5);
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-attaches promptly when the host republishes on a fresh endpoint", async () => {
    vi.useFakeTimers();
    try {
      saveMemberAnchor("person", {
        server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://old.test", endpoint_generation: 2,
        rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
      });
      const oldServer = () => liveServer({
        endpoint: { origin: "https://old.test", generation: 2, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
      });
      const newServer = () => liveServer({
        endpoint: { origin: "https://new.test", generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
      });
      attach.mockRejectedValue(Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" }));
      const { result, rerender } = renderHook(({ directory }: { directory: CentralDirectoryState }) => useMemberHosts({
        personId: "person", sessionToken: "token", directory,
      }), { initialProps: { directory: directoryFor([oldServer()]) as CentralDirectoryState } });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(attach).toHaveBeenCalledTimes(1);
      // The host turned external access back on with a new random origin:
      // same identity, fresh endpoint tuple.
      attach.mockImplementation(async () => ({
        admission: admissionFor([{ room_id: "general", name: "일반" }]),
        rooms: [{ room_id: "general", name: "일반" }],
        anchor: { server_id: "friend", registration_epoch: "epoch" },
      }));
      rerender({ directory: directoryFor([newServer()]) });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      // Prompt reconnect on the refresh — no slow-timer wait, and the
      // superseded slow timer never fires a duplicate behind it.
      expect(attach).toHaveBeenCalledTimes(2);
      expect(result.current.snapshots[0]?.phase).toBe("connected");
      await act(async () => { await vi.advanceTimersByTimeAsync(120_000); });
      expect(attach).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("backs off for the server's Retry-After on central rate limiting", async () => {
    vi.useFakeTimers();
    try {
      attach.mockRejectedValue(Object.assign(new Error("limited"), { status: 429, code: "rate_limited", retryAfterMs: 120_000 }));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
      }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(attach).toHaveBeenCalledTimes(1);
      // The server asked for 120 s: 60 s is still quiet, 130 s retries.
      await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      expect(attach).toHaveBeenCalledTimes(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(70_000); });
      expect(attach).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("rests a long default on a 429 without a Retry-After header", async () => {
    vi.useFakeTimers();
    try {
      attach.mockRejectedValue(Object.assign(new Error("limited"), { status: 429, code: "rate_limited" }));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
      }));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(attach).toHaveBeenCalledTimes(1);
      // Default rest is 5 min ± jitter: nothing within the first minute.
      await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      expect(attach).toHaveBeenCalledTimes(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(340_000); });
      expect(attach.mock.calls.length).toBeGreaterThan(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("force-closes the adopted transport and reports confirmed loss instead of re-attaching", async () => {
    const transportClose = vi.fn();
    const admission = {
      transport: { onClose: vi.fn(), close: transportClose, active: true },
      close: vi.fn(),
      select: vi.fn(),
    };
    attach.mockImplementation(async () => ({
      admission,
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const onConfirmedMembershipLost = vi.fn();
    const { result, rerender } = renderHook(({ directory }: { directory: CentralDirectoryState }) => useMemberHosts({
      personId: "person", sessionToken: "token", directory, onConfirmedMembershipLost,
    }), { initialProps: { directory: directoryFor([liveServer()]) as CentralDirectoryState } });
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledTimes(1);
    // Central connectedly confirms the membership is gone (kick/ban/revoke):
    // the adopted channel closes outright (a plain detach would leave it
    // open), the loss is reported once, and no silent re-attach follows.
    rerender({ directory: directoryFor([]) });
    await waitFor(() => expect(result.current.snapshots).toEqual([]), { timeout: 5000 });
    expect(transportClose).toHaveBeenCalled();
    expect(onConfirmedMembershipLost).toHaveBeenCalledTimes(1);
    expect(onConfirmedMembershipLost).toHaveBeenCalledWith("friend");
    await act(async () => {});
    expect(attach).toHaveBeenCalledTimes(1);
  });

  it("drops the previous account's snapshots on account switch", async () => {
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "hihi-room", name: "Hihi room" }]),
      rooms: [{ room_id: "hihi-room", name: "Hihi room" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const directoryA: CentralDirectoryState = {
      status: "connected",
      person: { person_id: "person-a", display_name: "A", identity_kind: "google" },
      servers: [{ server_id: "friend", relation: "member", alias: "Hihi" }],
      live: {
        person: { person_id: "person-a", display_name: "A", identity_kind: "google" },
        servers: [liveServer()], server_time: 1,
      },
    };
    const { result, rerender } = renderHook(({ personId }: { personId: string }) => useMemberHosts({
      personId, sessionToken: `${personId}-token`, directory: personId === "person-a" ? directoryA : {
        status: "connected" as const,
        person: { person_id: "person-b", display_name: "B", identity_kind: "google" as const },
        servers: [], live: { person: { person_id: "person-b", display_name: "B", identity_kind: "google" as const }, servers: [], server_time: 1 },
      },
    }), { initialProps: { personId: "person-a" } });
    await waitFor(() => expect(result.current.snapshots).toHaveLength(1), { timeout: 5000 });
    expect(result.current.snapshots[0]).toMatchObject({ serverId: "friend" });
    rerender({ personId: "person-b" });
    await waitFor(() => expect(result.current.snapshots).toEqual([]), { timeout: 5000 });
  });

  it("attaches a previously joined host on sign-in without manual retry", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const { result, rerender } = renderHook(
      ({ personId, sessionToken, directory }: {
        personId: string; sessionToken: string; directory: CentralDirectoryState | null;
      }) => useMemberHosts({ personId, sessionToken, directory }),
      { initialProps: { personId: "", sessionToken: "", directory: null as CentralDirectoryState | null } },
    );
    // Signed-out shell: no anchor read, no attach attempt.
    expect(result.current.snapshots).toEqual([]);
    expect(attach).not.toHaveBeenCalled();
    // Sign in as the account that previously joined the friend room: the
    // session/directory change alone drives the attach, no retryHost call.
    rerender({ personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]) });
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledTimes(1);
    expect(result.current.hostDirs[0]).toMatchObject({ serverId: "friend", state: "connected" });
  });

  it("re-attaches on its own after a transient failure without manual retry", async () => {
    saveMemberAnchor("person", {
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    });
    attach.mockRejectedValueOnce(new Error("참가 서버의 응답이 없어요. 다시 시도해 주세요."));
    attach.mockImplementation(async () => ({
      admission: admissionFor([{ room_id: "general", name: "일반" }]),
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch" },
    }));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person", sessionToken: "token", directory: directoryFor([liveServer()]),
    }));
    // The first automatic attempt fails: the room reads disconnected with
    // its last-known rooms and a scheduled re-attach parks behind it.
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("disconnected"), { timeout: 5000 });
    // The per-host backoff re-attaches on its own within a few seconds when
    // the host is up — no explicit retry needed.
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(attach).toHaveBeenCalledTimes(2);
  });

  it("confirms a member removal only from a live host that lists without the room", () => {
    const room = { roomOrigin: "remote_server" as const, serverId: "friend", meetingId: "general" };
    const snapshot = (phase: "connecting" | "connected" | "disconnected", rooms: { room_id: string; name: string }[] | null) =>
      ({ serverId: "friend", phase, origin: "https://friend.test", rooms, detail: null });
    // A live host that lists its rooms without this one removed it: fall back.
    expect(memberRoomRemovedWhileLive(room, [snapshot("connected", [{ room_id: "other", name: "다른" }])])).toBe(true);
    // Every other state is transient and retains the selection.
    expect(memberRoomRemovedWhileLive(room, [snapshot("connected", [{ room_id: "general", name: "일반" }])])).toBe(false);
    expect(memberRoomRemovedWhileLive(room, [snapshot("connected", null)])).toBe(false);
    expect(memberRoomRemovedWhileLive(room, [snapshot("connecting", [{ room_id: "general", name: "일반" }])])).toBe(false);
    expect(memberRoomRemovedWhileLive(room, [snapshot("connecting", null)])).toBe(false);
    expect(memberRoomRemovedWhileLive(room, [snapshot("disconnected", [{ room_id: "general", name: "일반" }])])).toBe(false);
    expect(memberRoomRemovedWhileLive(room, [])).toBe(false);
    expect(memberRoomRemovedWhileLive(
      { roomOrigin: "local" as const, meetingId: "general" }, [snapshot("connected", [])])).toBe(false);
  });
});
