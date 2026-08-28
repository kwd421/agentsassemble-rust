import { act, renderHook, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PublicInviteStatus, RoomFriend } from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomInviteController } from "./useRoomInviteController";

const apiMocks = vi.hoisted(() => ({
  createOperatorPairing: vi.fn(),
  createRoomInvite: vi.fn(),
  fetchPublicInviteStatus: vi.fn(),
  startPublicInviteTunnel: vi.fn(),
  stopPublicInviteTunnel: vi.fn(),
}));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  ...apiMocks,
}));

const publicStatus: PublicInviteStatus = {
  mode: "managed",
  public_url: "https://room.example.com",
  stable_url: "",
  tunnel: {
    available: true,
    running: true,
    phase: "running",
    public_url: "https://room.example.com",
    local_url: "http://127.0.0.1:43123",
    stable_phase: "unconfigured",
  },
};

function publicStatusAt(publicUrl: string): PublicInviteStatus {
  return {
    ...publicStatus,
    public_url: publicUrl,
    tunnel: { ...publicStatus.tunnel, public_url: publicUrl },
  };
}

const stoppedStatus: PublicInviteStatus = {
  ...publicStatus,
  public_url: "",
  tunnel: {
    ...publicStatus.tunnel,
    running: false,
    phase: "stopped",
    public_url: "",
  },
};

const startingStatus: PublicInviteStatus = {
  ...publicStatus,
  public_url: "",
  tunnel: {
    ...publicStatus.tunnel,
    phase: "starting",
    public_url: "",
  },
};

const room: RoomDockItem = {
  id: "room-1",
  label: "Room One",
  meetingId: "room-1",
  topic: "",
  shortLabel: "R1",
  icon: Hash,
  createdAt: "2026-07-12T00:00:00Z",
  tone: "fresh",
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}

function renderInviteController(localOperatorEligible = true) {
  return renderHook(() =>
    useRoomInviteController({
      localOperatorEligible,
    })
  );
}

describe("useRoomInviteController", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMocks.fetchPublicInviteStatus.mockResolvedValue(publicStatus);
  });

  it("ignores a stale public-invite status after switching modal rooms", async () => {
    const firstStatus = deferred<PublicInviteStatus>();
    const secondStatus = deferred<PublicInviteStatus>();
    apiMocks.fetchPublicInviteStatus
      .mockReturnValueOnce(firstStatus.promise)
      .mockReturnValueOnce(secondStatus.promise);
    const hook = renderInviteController();

    act(() => hook.result.current.open("room-1"));
    await waitFor(() => expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(1));
    act(() => hook.result.current.open("room-2"));
    await waitFor(() => expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(2));

    await act(async () => {
      firstStatus.resolve(publicStatusAt("https://stale.example.com"));
      await firstStatus.promise;
    });
    expect(hook.result.current.publicInviteStatus).toBeNull();

    await act(async () => {
      secondStatus.resolve(publicStatusAt("https://current.example.com"));
      await secondStatus.promise;
    });
    expect(hook.result.current.publicInviteStatus?.public_url).toBe("https://current.example.com");
  });

  it("invites an active AI friend and clears room-scoped state for the next modal", async () => {
    const friend: RoomFriend = {
      friend_id: "friend:codex",
      display_name: "Codex Friend",
      handle: "codex",
      participant_type: "subscription_ai",
      provider_kind: "codex",
      connection_kind: "agent_session",
      source_agent_id: "codex-friend",
      last_meeting_id: "",
      status: "online",
      source: "test",
      created_at: "2026-07-12T00:00:00Z",
      updated_at: "2026-07-12T00:00:00Z",
    };
    apiMocks.createRoomInvite.mockResolvedValue({
      invite_id: "invite-friend",
      invite_token: "token-friend",
      meeting_id: room.meetingId,
      agent_id: friend.source_agent_id,
      display_name: friend.display_name,
      invite_scope: "room",
      expires_at: "2026-07-13T00:00:00Z",
      room_url: "https://room.example.com",
      join_url: "https://room.example.com/join?token=token-friend",
      remote_client_packet: { attend: { room: room.meetingId } },
    });
    const hook = renderInviteController();
    act(() => hook.result.current.open("room-1"));

    await act(async () => {
      await hook.result.current.inviteFriend({ friend, room });
    });

    expect(hook.result.current.friendStatuses[friend.friend_id]).toBe("입장 패킷 생성됨");
    expect(hook.result.current.remoteClientPacket.preview).toContain('"attend"');

    act(() => hook.result.current.open("room-2"));

    expect(hook.result.current.remoteClientPacket).toEqual({ friendName: "", preview: "" });
    expect(hook.result.current.friendStatuses).toEqual({});
    expect(hook.result.current.secureInviteUrl).toBe("");
    expect(hook.result.current.agentInviteUrl).toBe("");
    expect(hook.result.current.operatorPairingUrl).toBe("");
  });

  it("creates a dedicated short-lived operator pairing link", async () => {
    apiMocks.createOperatorPairing.mockResolvedValue({
      status: "created",
      pairing_id: "pair-1",
      room_id: room.meetingId,
      target_origin: "https://room.example.com",
      expires_at: "2026-07-15T12:02:00Z",
      pairing_url: "https://room.example.com/pair?token=aap1_secret",
    });
    const hook = renderInviteController();

    await act(async () => {
      await hook.result.current.generateOperatorPairing(room);
    });

    expect(apiMocks.createOperatorPairing).toHaveBeenCalledWith({
      meetingId: room.meetingId,
      sessionToken: "",
    });
    expect(hook.result.current.operatorPairingUrl).toBe(
      "https://room.example.com/pair?token=aap1_secret"
    );
    expect(hook.result.current.copyStatus).toContain("2분");
  });

  it("creates a current-session agent invite without selecting or launching a provider", async () => {
    apiMocks.createRoomInvite.mockResolvedValue({
      invite_id: "invite-current-session",
      invite_token: "token-current-session",
      meeting_id: room.meetingId,
      agent_id: "external-agent",
      display_name: "External Agent",
      invite_scope: "room",
      expires_at: "2026-07-13T00:00:00Z",
      room_url: "https://room.example.com",
      join_url: "https://room.example.com/join?token=token-current-session",
    });
    const hook = renderInviteController();

    await act(async () => {
      await hook.result.current.generateAgentInvite(room);
    });

    expect(apiMocks.createRoomInvite).toHaveBeenCalledWith({
      meetingId: room.meetingId,
      agentId: "external-agent",
      displayName: "External Agent",
      inviteScope: "room",
      ttlSeconds: 3600,
      clientType: "browser",
      providerKind: "manual",
      participantType: "agent",
      maxUses: 1,
      sessionToken: "",
    });
    expect(hook.result.current.agentInviteUrl).toBe(
      "https://room.example.com/join?token=token-current-session"
    );
  });

  it("does not expose the local server when invite generation lacks explicit consent", async () => {
    apiMocks.fetchPublicInviteStatus.mockResolvedValue({
      ...stoppedStatus,
    });
    const hook = renderInviteController();

    await act(async () => {
      await hook.result.current.generateAgentInvite(room);
    });

    expect(apiMocks.startPublicInviteTunnel).not.toHaveBeenCalled();
    expect(apiMocks.createRoomInvite).not.toHaveBeenCalled();
    expect(hook.result.current.copyStatus).toContain("외부 접속");
  });

  it("makes no ingress or timer call for an ineligible surface", async () => {
    const hook = renderInviteController(false);
    const timerSpy = vi.spyOn(window, "setTimeout");

    act(() => hook.result.current.open("room-1"));
    await act(async () => {
      await hook.result.current.startTunnel();
      await hook.result.current.stopTunnel();
    });

    expect(apiMocks.fetchPublicInviteStatus).not.toHaveBeenCalled();
    expect(apiMocks.startPublicInviteTunnel).not.toHaveBeenCalled();
    expect(apiMocks.stopPublicInviteTunnel).not.toHaveBeenCalled();
    expect(timerSpy).not.toHaveBeenCalled();
    expect(hook.result.current.copyStatus).toContain("로컬 운영자");
    timerSpy.mockRestore();
  });

  it("publishes the exact stopped status returned by the operator route", async () => {
    apiMocks.stopPublicInviteTunnel.mockResolvedValue(stoppedStatus);
    const hook = renderInviteController();

    await act(async () => {
      await hook.result.current.stopTunnel();
    });

    expect(hook.result.current.publicAccessTransition).toBe("idle");
    expect(hook.result.current.publicInviteStatus?.public_url).toBe("");
    expect(apiMocks.fetchPublicInviteStatus).not.toHaveBeenCalled();
    expect(hook.result.current.copyStatus).toContain("이 컴퓨터에서 계속 작동");
  });

  it("retires readiness polling when Stop supersedes Start", async () => {
    vi.useFakeTimers();
    try {
      apiMocks.startPublicInviteTunnel.mockResolvedValue(startingStatus);
      apiMocks.fetchPublicInviteStatus.mockResolvedValue(startingStatus);
      apiMocks.stopPublicInviteTunnel.mockResolvedValue(stoppedStatus);
      const hook = renderInviteController();
      let startPromise!: Promise<void>;

      await act(async () => {
        startPromise = hook.result.current.startTunnel();
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(1);
      expect(vi.getTimerCount()).toBe(1);

      await act(async () => {
        await hook.result.current.stopTunnel();
        await startPromise;
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });

      expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(1);
      expect(hook.result.current.publicInviteStatus).toEqual(stoppedStatus);
      expect(hook.result.current.publicAccessTransition).toBe("idle");
      expect(hook.result.current.copyStatus).toContain("이 컴퓨터에서 계속 작동");
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not dispatch Start after Stop retires a delayed ticket", async () => {
    const ticket = deferred<void>();
    let startDispatched = false;
    apiMocks.startPublicInviteTunnel.mockImplementation(
      async (beforeDispatch?: () => void) => {
        await ticket.promise;
        beforeDispatch?.();
        startDispatched = true;
        return startingStatus;
      }
    );
    apiMocks.stopPublicInviteTunnel.mockResolvedValue(stoppedStatus);
    const hook = renderInviteController();
    let startPromise!: Promise<void>;

    act(() => {
      startPromise = hook.result.current.startTunnel();
    });
    await waitFor(() => expect(apiMocks.startPublicInviteTunnel).toHaveBeenCalledOnce());
    expect(apiMocks.startPublicInviteTunnel).toHaveBeenCalledWith(expect.any(Function));

    await act(async () => {
      await hook.result.current.stopTunnel();
      ticket.resolve();
      await startPromise;
    });

    expect(startDispatched).toBe(false);
    expect(hook.result.current.publicInviteStatus).toEqual(stoppedStatus);
    expect(hook.result.current.publicAccessTransition).toBe("idle");
  });

  it("gives a superseding invite operation ownership of the access state", async () => {
    vi.useFakeTimers();
    try {
      apiMocks.startPublicInviteTunnel.mockResolvedValue(startingStatus);
      apiMocks.fetchPublicInviteStatus.mockResolvedValue(startingStatus);
      apiMocks.createOperatorPairing.mockResolvedValue({
        status: "created",
        pairing_id: "pair-superseding",
        room_id: room.meetingId,
        target_origin: publicStatus.public_url,
        expires_at: "2026-07-15T12:02:00Z",
        pairing_url: `${publicStatus.public_url}/pair?token=aap1_superseding`,
      });
      const hook = renderInviteController();
      let startPromise!: Promise<void>;

      await act(async () => {
        startPromise = hook.result.current.startTunnel();
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(vi.getTimerCount()).toBe(1);

      apiMocks.fetchPublicInviteStatus.mockResolvedValue(publicStatus);
      await act(async () => {
        await hook.result.current.generateOperatorPairing(room);
        await startPromise;
      });

      expect(vi.getTimerCount()).toBe(0);
      expect(hook.result.current.publicAccessTransition).toBe("idle");
      expect(hook.result.current.operatorPairingUrl).toContain("aap1_superseding");
    } finally {
      vi.useRealTimers();
    }
  });
});
