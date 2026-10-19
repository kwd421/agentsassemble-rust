import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { useMemberRoomAutoReselect } from "./useMemberRoomAutoReselect";
import type { CentralDirectoryState } from "./useCentralDirectory";
import type { RoomDockItem } from "../lib/roomDockModel";
import {
  bindRemoteSession,
  closeRemoteWorkspace,
  installRemoteWorkspace,
  remoteWorkspaceFor,
} from "../lib/remote/remoteWorkspace";
import {
  roomGuestSessionExpired,
  roomGuestSessionFromJoinPayload,
  type RoomGuestSession,
} from "../lib/roomGuestSession";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import { resetMemberRoomAutoReselectBudgetForTests } from "../lib/central/memberRoomAutoReselectBudget";
import { resetMemberHostOfflineBudgetForTests } from "../lib/central/memberRoomLive";
import { resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const ACCOUNT = "person";
const SERVER = "44444444-4444-4444-8444-444444444444";
const EPOCH = "epoch-orphan";
const ORIGIN = "https://friend-orphan.test";
const ORIGIN_NEXT = "https://friend-orphan-next.test";
const ROOM_A = "room-a";
const ROOM_B = "room-b";

const attach = vi.fn();
vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

vi.mock("../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: () => "device",
  getOrCreateClientId: () => "client",
}));

type FakeAdmission = {
  transport: {
    hello: { origin: string; server_id: string };
    active: boolean;
    onClose: (listener: () => void) => () => void;
    close: () => void;
  };
  adopted: boolean;
  channelBoundSelectFree: boolean;
  deviceToken: string;
  close: () => void;
  select: (roomId: string) => Promise<Record<string, unknown>>;
  selectBackgroundRoom: (roomId: string) => Promise<Record<string, unknown>>;
  selects: string[];
  backgroundSelects: string[];
  kill: (fireClose: boolean) => void;
};

let sessionCounter = 0;

function joinFor(roomId: string) {
  sessionCounter += 1;
  return {
    server_id: SERVER,
    authority_lineage_id: "22222222-2222-4222-8222-222222222222",
    server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
    session_token: `session-${roomId}-${sessionCounter}`,
    agent_id: "member-1",
    display_name: "Member",
    meeting_id: roomId,
    invite_scope: "room",
    participant_type: "human",
    client_type: "browser",
    provider_kind: "manual",
    connection_kind: "native_remote_room_client",
    expires_at: new Date(Date.now() + 3_600_000).toISOString(),
    room_label: roomId,
    room_topic: "",
    room_created_at: "2026-07-10T00:00:00Z",
    status: "admitted",
    request_id: `request-${sessionCounter}`,
    owner_display_name: "",
    owner_id: "owner-1",
    stable_identity: false,
    operator: false,
    client_id: "client",
    guide: {
      welcome: "Welcome",
      how_to: ["Read and send messages."],
      etiquette: [],
      session: { expires_in_seconds: 3600, rejoin: "Request another invite." },
    },
  };
}

function installMemberWorkspace(admission: FakeAdmission, roomId: string) {
  const payload = joinFor(roomId) as never;
  const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
  installRemoteWorkspace({
    transport: admission.transport as never,
    owner: null,
    member,
    deviceToken: "device",
    clientId: "client",
  });
  return payload as unknown as Record<string, unknown>;
}

function fakeAdmission(): FakeAdmission {
  const listeners = new Set<() => void>();
  const transport = {
    hello: { origin: ORIGIN, server_id: SERVER },
    active: true,
    onClose: vi.fn((listener: () => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    }),
    close: vi.fn(() => {
      transport.active = false;
    }),
  };
  const admission = {
    transport,
    adopted: false,
    channelBoundSelectFree: true,
    deviceToken: "device",
    close: vi.fn(() => {
      // Mirrors SecureMemberAdmission.close: an adopted channel (the room
      // lives on it) is never closed by a registry replacement.
      if (!admission.adopted) transport.close();
    }),
    select: vi.fn(async (roomId: string) => {
      admission.selects.push(roomId);
      const payload = installMemberWorkspace(admission, roomId);
      admission.adopted = true;
      return payload;
    }),
    selectBackgroundRoom: vi.fn(async (roomId: string) => {
      admission.backgroundSelects.push(roomId);
      const payload = joinFor(roomId) as never;
      const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
      bindRemoteSession(member.sessionToken, transport as never, SERVER);
      return payload as unknown as Record<string, unknown>;
    }),
    selects: [] as string[],
    backgroundSelects: [] as string[],
    kill: (fireClose: boolean) => {
      transport.active = false;
      if (fireClose) {
        for (const listener of [...listeners]) listener();
      }
    },
  };
  return admission;
}

function anchorFor(origin: string) {
  return {
    server_id: SERVER,
    registration_epoch: EPOCH,
    host_key_fingerprint: "fingerprint",
    endpoint_origin: origin,
    endpoint_generation: 3,
    rooms: [
      { room_id: ROOM_A, name: ROOM_A },
      { room_id: ROOM_B, name: ROOM_B },
    ],
    hidden: false,
    updated_at: Date.now(),
  };
}

const unconfirmedDirectory = (): CentralDirectoryState => ({
  status: "central-unconfirmed",
  person: { person_id: ACCOUNT, display_name: "나", identity_kind: "google" },
  servers: [],
  live: null,
  error: undefined,
});

const connectedDirectory = (origin = ORIGIN): CentralDirectoryState => {
  const server = {
    server_id: SERVER,
    registration_epoch: EPOCH,
    relation: "member" as const,
    alias: "친구",
    host_key_fingerprint: "fingerprint",
    host_public_key_jwk: {},
    endpoint: {
      origin,
      generation: 3,
      status: "published" as const,
      mode: "event_secure_v1" as const,
      protocol: "secure_admission_v1",
    },
  };
  return {
    status: "connected",
    person: { person_id: ACCOUNT, display_name: "나", identity_kind: "google" },
    servers: [{ ...server, host_key_fingerprint: undefined, host_public_key_jwk: undefined, endpoint: undefined } as never],
    live: {
      person: { person_id: ACCOUNT, display_name: "나", identity_kind: "google" },
      servers: [server],
      server_time: 1,
    },
    error: undefined,
  } as unknown as CentralDirectoryState;
};

function railRoom(meetingId: string, id: string): RoomDockItem {
  return {
    id,
    label: meetingId,
    meetingId,
    serverId: SERVER,
    roomOrigin: "remote_server",
    connectionState: "connected",
    topic: "",
    shortLabel: "F",
    icon: (() => null) as never,
    createdAt: "",
    tone: "fresh",
  } as unknown as RoomDockItem;
}

const backgroundOpen = vi.fn(() => ({ close: vi.fn() }));

type Harness = {
  result: {
    current: {
      hosts: ReturnType<typeof useMemberHosts>;
      session: RoomGuestSession | null;
      selectError: { roomId: string; message: string } | null;
      setDirectory: (directory: CentralDirectoryState) => void;
      expireSessions: () => Promise<void>;
    };
  };
  unmount: () => void;
};

function setupHarness(): Harness {
  const hook = renderHook(() => {
    const [directory, setDirectory] = useState<CentralDirectoryState>(() => unconfirmedDirectory());
    const [session, setSession] = useState<RoomGuestSession | null>(null);
    const [selectError, setSelectErrorState] = useState<{ roomId: string; message: string } | null>(null);
    const [clock, setClock] = useState(0);
    void clock;
    const hosts = useMemberHosts({ personId: ACCOUNT, sessionToken: "token", directory });
    const admittedSessionToken =
      session && !roomGuestSessionExpired(session) ? session.sessionToken : "";
    const live = useMemberRoomLive({
      snapshots: hosts.snapshots,
      admissionFor: hosts.admissionFor,
      activeServerId: SERVER,
      activeMeetingId: ROOM_A,
      accountId: ACCOUNT,
      openSocket: backgroundOpen as never,
    });
    useMemberRoomAutoReselect({
      activeRoom: railRoom(ROOM_A, "rail-a") as never,
      ownerServerId: undefined,
      accountId: ACCOUNT,
      hasAnchor: hosts.hasAnchor,
      isRefused: hosts.isRefused,
      snapshots: hosts.snapshots,
      admissionFor: hosts.admissionFor,
      guestSession: session as never,
      guestExpired: false,
      admittedSessionToken,
      liveSessionFor: live.liveSessionFor,
      selectRoom: hosts.selectRoom,
      acceptSession: (async (payload: never) => {
        const next = { ...roomGuestSessionFromJoinPayload("", payload as never), centralMember: true as const };
        if (roomGuestSessionExpired(next)) return false;
        setSession(next);
        return true;
      }) as never,
      setSelectError: ((update: unknown) => {
        if (typeof update === "function") {
          setSelectErrorState((current) =>
            (update as (c: typeof current) => typeof current)(current));
        } else {
          setSelectErrorState(update as typeof selectError);
        }
      }) as never,
    });
    return {
      hosts,
      session,
      selectError,
      setDirectory,
      expireSessions: async () => {
        await vi.advanceTimersByTimeAsync(3_601_000);
        setClock((value) => value + 1);
      },
    };
  });
  return hook as unknown as Harness;
}

describe("member host channel custody through session expiry", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    backgroundOpen.mockReset();
    backgroundOpen.mockImplementation(() => ({ close: vi.fn() }));
    attach.mockReset();
    sessionCounter = 0;
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    resetCentralRequestGovernorForTests();
    resetMemberHostOfflineBudgetForTests();
    resetMemberRoomAutoReselectBudgetForTests();
    saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    resetMemberRoomAutoReselectBudgetForTests();
  });

  it("startup with anchors creates exactly one member channel and one grant", async () => {
    let resolveAttach!: (attached: {
      admission: FakeAdmission;
      rooms: { room_id: string; name: string }[];
      anchor: ReturnType<typeof anchorFor>;
    }) => void;
    attach.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveAttach = resolve;
        }),
    );
    const env = setupHarness();
    await waitFor(() => expect(attach).toHaveBeenCalledTimes(1));
    // A second reconcile while the first attach is still in flight (anchor
    // save, directory refresh) must not open a second channel.
    await act(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(attach).toHaveBeenCalledTimes(1);
    const admission = fakeAdmission();
    await act(async () => {
      resolveAttach({
        admission,
        rooms: [
          { room_id: ROOM_A, name: ROOM_A },
          { room_id: ROOM_B, name: ROOM_B },
        ],
        anchor: anchorFor(ORIGIN),
      });
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    expect(attach).toHaveBeenCalledTimes(1);
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(admission);
    env.unmount();
  });

  it("explicit reconnect on the live room keeps the live admission registered with no new grant", async () => {
    const admission = fakeAdmission();
    attach.mockImplementation(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      return {
        admission,
        rooms: [
          { room_id: ROOM_A, name: ROOM_A },
          { room_id: ROOM_B, name: ROOM_B },
        ],
        anchor: anchorFor(ORIGIN),
      };
    });
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    // Fresh admission auto re-selects the open room onto the live channel.
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    expect(admission.selects).toEqual([ROOM_A]);
    expect(remoteWorkspaceFor(SERVER)?.transport).toBe(admission.transport);
    const grantsBefore = attach.mock.calls.length;

    // The open room is live on the workspace channel; an explicit reconnect
    // (the same reattach path a timer-driven retry uses) must reconcile on
    // that live channel instead of dropping the registry entry and orphaning
    // the admission the session-expiry heal needs.
    await act(async () => {
      env.result.current.hosts.retryHost(SERVER);
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected");
    expect(attach.mock.calls.length).toBe(grantsBefore);
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(admission);
    expect(admission.transport.active).toBe(true);
    env.unmount();
  });

  it("open-room session TTL expiry heals once over the live admission with zero central", async () => {
    const admission = fakeAdmission();
    attach.mockImplementation(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      return {
        admission,
        rooms: [{ room_id: ROOM_A, name: ROOM_A }],
        anchor: anchorFor(ORIGIN),
      };
    });
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    await act(async () => {
      env.result.current.hosts.retryHost(SERVER);
      await vi.advanceTimersByTimeAsync(0);
    });
    const grantsBefore = attach.mock.calls.length;
    const selectsBefore = admission.selects.length;
    const sessionBefore = env.result.current.session?.sessionToken;
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(admission);

    // SESSION_TTL passes on the live channel: the bearer lapses by the clock
    // while the host session row and the secure channel stay live.
    await act(async () => {
      await env.result.current.expireSessions();
    });
    // Exactly one free channel-bound re-select, no new central grant, and the
    // room is live again with no stop line.
    await waitFor(() => expect(admission.selects.length).toBe(selectsBefore + 1));
    await waitFor(() =>
      expect(
        env.result.current.session &&
          !roomGuestSessionExpired(env.result.current.session) &&
          env.result.current.session.sessionToken !== sessionBefore,
      ).toBe(true),
    );
    expect(env.result.current.selectError).toBeNull();
    expect(attach.mock.calls.length).toBe(grantsBefore);
    const parkedSelects = admission.selects.length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_000);
    });
    expect(admission.selects.length).toBe(parkedSelects);
    expect(attach.mock.calls.length).toBe(grantsBefore);
    env.unmount();
  });

  it("background-room session expiry heals on the same live admission with zero central", async () => {
    const admission = fakeAdmission();
    attach.mockImplementation(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      return {
        admission,
        rooms: [
          { room_id: ROOM_A, name: ROOM_A },
          { room_id: ROOM_B, name: ROOM_B },
        ],
        anchor: anchorFor(ORIGIN),
      };
    });
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    // The background room selected once over the live host channel.
    await waitFor(() => expect(admission.backgroundSelects).toEqual([ROOM_B]));
    const grantsBefore = attach.mock.calls.length;
    await act(async () => {
      env.result.current.hosts.retryHost(SERVER);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(admission);

    // The background room session lapses: the socket reports
    // authorization_failed and the record re-selects once over the same live
    // admission instead of parking.
    const backgroundHandle = backgroundOpen.mock.results
      .map((call) => call.value)
      .find((handle) => handle && typeof handle === "object");
    expect(backgroundHandle).toBeTruthy();
    const handlers = backgroundOpen.mock.calls.map((call) => (call as unknown[])[2] as {
      onError?: (error: Error) => void;
    });
    expect(handlers.length).toBeGreaterThan(0);
    await act(async () => {
      for (const handler of handlers) {
        handler.onError?.(Object.assign(new Error("denied"), { category: "authorization_failed" }));
      }
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(admission.backgroundSelects.length).toBe(2));
    expect(attach.mock.calls.length).toBe(grantsBefore);
    env.unmount();
  });

  it("a republish while live closes the redundant channel instead of clobbering the registry", async () => {
    const first = fakeAdmission();
    const second = fakeAdmission();
    attach.mockImplementationOnce(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      return {
        admission: first,
        rooms: [{ room_id: ROOM_A, name: ROOM_A }],
        anchor: anchorFor(ORIGIN),
      };
    });
    attach.mockImplementationOnce(async () => {
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN_NEXT));
      return {
        admission: second,
        rooms: [{ room_id: ROOM_A, name: ROOM_A }],
        anchor: anchorFor(ORIGIN_NEXT),
      };
    });
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    expect(remoteWorkspaceFor(SERVER)?.transport).toBe(first.transport);

    // The friend host republishes on a fresh endpoint while the room is live
    // on the old channel: the completing second attach must not replace the
    // live admission with an admit-only duplicate that the host reaps 30 s
    // later. The redundant channel closes; the registry keeps serving.
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(attach).toHaveBeenCalledTimes(2));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(first);
    expect(first.transport.active).toBe(true);
    expect(second.transport.active).toBe(false);
    expect(remoteWorkspaceFor(SERVER)?.transport).toBe(first.transport);
    // No churn behind it: the suppressed duplicate never schedules a retry.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(attach).toHaveBeenCalledTimes(2);
    env.unmount();
  });
});
