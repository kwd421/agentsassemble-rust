import { act, renderHook, waitFor } from "@testing-library/react";
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
