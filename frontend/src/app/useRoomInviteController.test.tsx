import { act, renderHook, waitFor } from "@testing-library/react";
import { Hash } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  HumanInviteDispatchError,
  type ManagedHumanInviteCustody,
  type PublicInviteStatus,
  type RoomFriend,
} from "../api";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomInviteController } from "./useRoomInviteController";

const apiMocks = vi.hoisted(() => ({
  createManagedHumanInvite: vi.fn(),
  createOperatorPairing: vi.fn(),
  createRoomInvite: vi.fn(),
  fetchPublicInviteStatus: vi.fn(),
  revokeManagedHumanInvite: vi.fn(),
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
const managerAuthority: DesktopManagerRoomAuthority = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002",
  room_id: room.meetingId,
  room_uid: "30000000-0000-4000-8000-000000000003",
};

function managedCustody(
  inviteId: string,
  expiresAt = Date.now() + 86_400_000
): ManagedHumanInviteCustody {
  const expires = new Date(expiresAt);
  const isoExpiry = expires.toISOString();
  const exactExpiry = expires.getUTCMilliseconds()
    ? isoExpiry.replace(/\.(\d{3})Z$/, ".$1000+00:00")
    : isoExpiry.replace(".000Z", "+00:00");
  return Object.freeze({
    authority: Object.freeze({ ...managerAuthority }),
    inviteId,
    joinUrl: `${publicStatus.public_url}/join?token=aaj1_${inviteId.repeat(2)}`,
    responseOrigin: publicStatus.public_url,
    expiresAt: Object.freeze({
      exact: exactExpiry,
      epochMilliseconds: expiresAt,
    }),
  });
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function renderInviteController(
  localOperatorEligible = true,
  resolveManagerRoomAuthority = (roomDockId: string) => {
    if (roomDockId !== room.id) throw new Error("room manager authority unavailable");
    return managerAuthority;
  }
) {
  return renderHook(() =>
    useRoomInviteController({
      localOperatorEligible,
      resolveManagerRoomAuthority,
    })
  );
}

describe("useRoomInviteController", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    apiMocks.fetchPublicInviteStatus.mockResolvedValue(publicStatus);
  });

  it("creates human invites through exact directory authority and retains accepted custody", async () => {
    const custody = managedCustody("0123456789abcdef");
    apiMocks.createManagedHumanInvite.mockImplementation(
      async (_intent, beforeDispatch?: () => void) => {
        beforeDispatch?.();
        return custody;
      }
    );
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));

    await act(async () => {
      await hook.result.current.generateSecureInvite(
        room,
        "room",
        { maxUses: 5, ttlSeconds: 604800 },
        false
      );
    });

    expect(apiMocks.createManagedHumanInvite).toHaveBeenCalledWith(
      {
        authority: managerAuthority,
        displayName: "Guest",
        inviteScope: "room",
        ttlSeconds: 604800,
        maxUses: 5,
      },
      expect.any(Function)
    );
    expect(apiMocks.createRoomInvite).not.toHaveBeenCalled();
    expect(hook.result.current.secureInviteUrl).toBe(custody.joinUrl);
    expect(hook.result.current.humanInvites).toEqual([
      expect.objectContaining({
        key: expect.any(String),
        maxUses: 5,
        ttlSeconds: 604800,
        retired: false,
        revocation: "idle",
        copyUrl: custody.joinUrl,
      }),
    ]);
  });

  it("retains a post-dispatch accepted invite as revoke-only when its operation is superseded", async () => {
    const accepted = deferred<ManagedHumanInviteCustody>();
    apiMocks.createManagedHumanInvite.mockReturnValue(accepted.promise);
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    let create!: Promise<void>;

    act(() => {
      create = hook.result.current.generateSecureInvite(room, "room");
    });
    await waitFor(() => expect(apiMocks.createManagedHumanInvite).toHaveBeenCalledOnce());
    act(() => hook.result.current.open("room-2"));
    await act(async () => {
      accepted.resolve(managedCustody("1111111111111111"));
      await create;
    });
    act(() => hook.result.current.open(room.id));

    await waitFor(() => expect(hook.result.current.humanInvites).toHaveLength(1));
    expect(hook.result.current.humanInvites[0]).toEqual(
      expect.objectContaining({ retired: true, revocation: "idle", copyUrl: "" })
    );
  });

  it("retires prior custody and preserves explicit revoke uncertainty and retry", async () => {
    apiMocks.createManagedHumanInvite
      .mockResolvedValueOnce(managedCustody("2222222222222222"))
      .mockResolvedValueOnce(managedCustody("3333333333333333"));
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));

    await act(async () => {
      await hook.result.current.generateSecureInvite(room, "room");
      await hook.result.current.generateSecureInvite(room, "room");
    });
    expect(hook.result.current.humanInvites).toHaveLength(2);
    expect(hook.result.current.humanInvites.map(({ retired }) => retired)).toEqual([
      false,
      true,
    ]);

    const currentKey = hook.result.current.humanInvites[0].key;
    apiMocks.revokeManagedHumanInvite
      .mockRejectedValueOnce(new HumanInviteDispatchError("proven_not_dispatched"))
      .mockRejectedValueOnce(new HumanInviteDispatchError("outcome_unknown"))
      .mockResolvedValueOnce("revoked");

    await act(async () => hook.result.current.revokeHumanInvite(currentKey));
    expect(hook.result.current.humanInvites[0].revocation).toBe("idle");
    await act(async () => hook.result.current.revokeHumanInvite(currentKey));
    expect(hook.result.current.humanInvites[0].revocation).toBe("unknown");
    await act(async () => hook.result.current.revokeHumanInvite(currentKey));
    expect(hook.result.current.humanInvites[0].revocation).toBe("dead");
    await act(async () => hook.result.current.revokeHumanInvite(currentKey));
    expect(apiMocks.revokeManagedHumanInvite).toHaveBeenCalledTimes(3);
  });

  it("uses the nearest expiry timer only to remove copy eligibility", async () => {
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));
    vi.useFakeTimers();
    try {
      apiMocks.createManagedHumanInvite.mockResolvedValue(
        managedCustody("4444444444444444", Date.now() + 1_000)
      );

      await act(async () => {
        await hook.result.current.generateSecureInvite(room, "room");
      });
      expect(hook.result.current.secureInviteUrl).not.toBe("");
      expect(vi.getTimerCount()).toBe(1);

      await act(async () => vi.advanceTimersByTimeAsync(1_000));

      expect(hook.result.current.secureInviteUrl).toBe("");
      expect(hook.result.current.humanInvites[0]).toEqual(
        expect.objectContaining({ expired: true, revocation: "idle", copyUrl: "" })
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("refreshes strict ingress status before authorizing a clipboard write", async () => {
    apiMocks.createManagedHumanInvite.mockResolvedValue(
      managedCustody("5555555555555555")
    );
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));
    await act(async () => hook.result.current.generateSecureInvite(room, "room"));
    const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    apiMocks.fetchPublicInviteStatus.mockResolvedValue(stoppedStatus);

    try {
      await act(async () => hook.result.current.copySecureInvite());
      expect(writeText).not.toHaveBeenCalled();
      expect(hook.result.current.publicInviteStatus).toEqual(stoppedStatus);
      expect(hook.result.current.secureInviteUrl).toBe("");
    } finally {
      if (clipboardDescriptor) {
        Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
    }
  });

  it("revalidates after an awaited clipboard rejection before fallback copy", async () => {
    apiMocks.createManagedHumanInvite.mockResolvedValue(
      managedCustody("6666666666666666")
    );
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));
    await act(async () => hook.result.current.generateSecureInvite(room, "room"));
    const inviteKey = hook.result.current.humanInvites[0].key;
    const clipboardWrite = deferred<void>();
    const revokeResult = deferred<"revoked">();
    apiMocks.revokeManagedHumanInvite.mockReturnValue(revokeResult.promise);
    const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    const execDescriptor = Object.getOwnPropertyDescriptor(document, "execCommand");
    const writeText = vi.fn(() => clipboardWrite.promise);
    const execCommand = vi.fn(() => true);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    let copy!: Promise<void>;
    let revoke!: Promise<void>;

    try {
      act(() => {
        copy = hook.result.current.copySecureInvite();
      });
      await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
      act(() => {
        revoke = hook.result.current.revokeHumanInvite(inviteKey);
      });
      await waitFor(() =>
        expect(hook.result.current.humanInvites[0].revocation).toBe("in_flight")
      );

      await act(async () => {
        clipboardWrite.reject(new Error("clipboard permission changed"));
        await copy;
      });

      expect(execCommand).not.toHaveBeenCalled();
      expect(hook.result.current.copyStatus).toBe("사람 초대 폐기 중...");
      await act(async () => {
        revokeResult.resolve("revoked");
        await revoke;
      });
    } finally {
      if (clipboardDescriptor) {
        Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
      if (execDescriptor) {
        Object.defineProperty(document, "execCommand", execDescriptor);
      } else {
        Reflect.deleteProperty(document, "execCommand");
      }
    }
  });

  it("does not revive clipboard fallback after the initiating modal closes", async () => {
    apiMocks.createManagedHumanInvite.mockResolvedValue(
      managedCustody("7777777777777777")
    );
    const hook = renderInviteController();
    act(() => hook.result.current.open(room.id));
    await waitFor(() => expect(hook.result.current.publicInviteStatus).toEqual(publicStatus));
    await act(async () => hook.result.current.generateSecureInvite(room, "room"));
    const clipboardWrite = deferred<void>();
    const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    const execDescriptor = Object.getOwnPropertyDescriptor(document, "execCommand");
    const writeText = vi.fn(() => clipboardWrite.promise);
    const execCommand = vi.fn(() => true);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText },
    });
    Object.defineProperty(document, "execCommand", {
      configurable: true,
      value: execCommand,
    });
    let copy!: Promise<void>;

    try {
      const statusFetchesBeforeCopy = apiMocks.fetchPublicInviteStatus.mock.calls.length;
      act(() => {
        copy = hook.result.current.copySecureInvite();
      });
      await waitFor(() => expect(writeText).toHaveBeenCalledOnce());
      expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(
        statusFetchesBeforeCopy + 1
      );
      act(() => hook.result.current.close());
      const statusAtClose = hook.result.current.copyStatus;

      await act(async () => {
        clipboardWrite.reject(new Error("clipboard permission changed"));
        await copy;
      });

      expect(apiMocks.fetchPublicInviteStatus).toHaveBeenCalledTimes(
        statusFetchesBeforeCopy + 1
      );
      expect(execCommand).not.toHaveBeenCalled();
      expect(document.querySelector("textarea")).toBeNull();
      expect(hook.result.current.copyStatus).toBe(statusAtClose);
    } finally {
      if (clipboardDescriptor) {
        Object.defineProperty(navigator, "clipboard", clipboardDescriptor);
      } else {
        Reflect.deleteProperty(navigator, "clipboard");
      }
      if (execDescriptor) {
        Object.defineProperty(document, "execCommand", execDescriptor);
      } else {
        Reflect.deleteProperty(document, "execCommand");
      }
    }
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
