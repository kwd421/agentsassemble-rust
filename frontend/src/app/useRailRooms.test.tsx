import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useRailRooms } from "./useRailRooms";
import { createFreshRoom } from "../lib/roomDockModel";

const attach = vi.fn();
vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

const liveServer = () => ({
  server_id: "friend", registration_epoch: "epoch", relation: "member" as const, alias: "친구",
  host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
  endpoint: { origin: "https://friend.test", generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
});
const directoryFor = (servers: ReturnType<typeof liveServer>[]) => ({
  status: "connected" as const,
  person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
  servers: servers.map(({ server_id, registration_epoch, relation, alias }) => ({ server_id, registration_epoch, relation, alias })),
  live: { person: { person_id: "person", display_name: "나", identity_kind: "google" as const }, servers, server_time: 1 },
  error: undefined,
});

describe("useRailRooms", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    window.localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
      token: "token", expires_at: 9_999_999_999, device_id: "device",
      person: { person_id: "person", identity_kind: "google", display_name: "나" },
    }));
    vi.clearAllMocks();
    attach.mockReset();
  });

  it("merges auto-attached member rooms with local rooms staying usable", async () => {
    attach.mockImplementation(async () => ({
      admission: { transport: { onClose: vi.fn() }, close: vi.fn(), select: vi.fn() },
      rooms: [{ room_id: "general", name: "일반" }],
      anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: "https://friend.test" },
    }));
    const localRoom = { ...createFreshRoom(), id: "local-1", meetingId: "local-1", label: "내 방" };
    const { result } = renderHook(() => useRailRooms({
      directoryRooms: [localRoom], startupHostEnabled: true, roomDirectorySyncIssue: null,
      centralDirectory: directoryFor([liveServer()]),
      ownerConnect: null, admittedSessionToken: "", guestSession: null, guestExpired: false,
    }));
    await waitFor(() => expect(result.current.rooms.some((room) => room.meetingId === "general")).toBe(true), { timeout: 5000 });
    const byMeeting = Object.fromEntries(result.current.rooms.map((room) => [room.meetingId, room]));
    expect(byMeeting["local-1"]).toMatchObject({ label: "내 방", connectionState: "local" });
    expect(byMeeting["general"]).toMatchObject({
      label: "일반", roomOrigin: "remote_server", serverId: "friend", connectionState: "connected",
    });
  });

  it("keeps own rooms working while an offline host shows its known rooms disconnected", async () => {
    window.localStorage.setItem("agentsassemble.memberAnchors.v1:person", JSON.stringify({ anchors: [{
      server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
      endpoint_origin: "https://friend.test", endpoint_generation: 2,
      rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
    }] }));
    attach.mockRejectedValue(new Error("꺼져 있어요."));
    const localRoom = { ...createFreshRoom(), id: "local-1", meetingId: "local-1", label: "내 방" };
    let phase = "";
    const { result } = renderHook(() => {
      const rail = useRailRooms({
        directoryRooms: [localRoom], startupHostEnabled: true, roomDirectorySyncIssue: null,
        centralDirectory: directoryFor([liveServer()]),
        ownerConnect: null, admittedSessionToken: "", guestSession: null, guestExpired: false,
      });
      phase = rail.memberHosts.snapshots[0]?.phase ?? "";
      return rail;
    });
    await waitFor(() => expect(phase).toBe("disconnected"), { timeout: 5000 });
    await waitFor(() => expect(result.current.rooms.some((room) => room.meetingId === "general")).toBe(true), { timeout: 5000 });
    const byMeeting = Object.fromEntries(result.current.rooms.map((room) => [room.meetingId, room]));
    expect(byMeeting["local-1"]?.connectionState).toBe("local");
    expect(byMeeting["general"]).toMatchObject({ connectionState: "disconnected", roomOrigin: "remote_server" });
  });
});
