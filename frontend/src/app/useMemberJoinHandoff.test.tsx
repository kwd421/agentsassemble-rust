import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { useMemberRoomAutoReselect } from "./useMemberRoomAutoReselect";
import type { CentralDirectoryState } from "./useCentralDirectory";
import type { RoomDockItem } from "../lib/roomDockModel";
import { closeRemoteWorkspace, installRemoteWorkspace } from "../lib/remote/remoteWorkspace";
import {
  roomGuestSessionExpired,
  roomGuestSessionFromJoinPayload,
  type RoomGuestSession,
} from "../lib/roomGuestSession";
import { loadMemberAnchor, saveMemberAnchor } from "../lib/central/memberAnchors";
import { clearMemberJoinHandoffs, noteMemberJoinConsent } from "../lib/central/memberJoinHandoff";
import { MemberAnchorMismatchError } from "../lib/central/memberAutoAttach";
import { resetMemberRoomAutoReselectBudgetForTests } from "../lib/central/memberRoomAutoReselectBudget";
import { resetMemberHostOfflineBudgetForTests } from "../lib/central/memberRoomLive";
import { resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const ACCOUNT = "person";
const OTHER_ACCOUNT = "other-person";
const SERVER = "44444444-4444-4444-8444-444444444444";
const OTHER_SERVER = "55555555-5555-4555-8555-555555555555";
const EPOCH = "epoch-1";
const EPOCH2 = "epoch-2";
const ORIGIN = "https://friend.test";
const ORIGIN_NEXT = "https://friend-next.test";
const ROOM_A = "room-a";

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
};

let sessionCounter = 0;
/** The first incarnation's channel: the one the startup attach serves. */
let epoch1: FakeAdmission;

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

function fakeAdmission(origin = ORIGIN): FakeAdmission {
  const listeners = new Set<() => void>();
  const transport = {
    hello: { origin, server_id: SERVER },
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
      return joinFor(roomId) as unknown as Record<string, unknown>;
    }),
    selects: [] as string[],
    backgroundSelects: [] as string[],
  };
  return admission;
}

function anchorFor(origin: string, epoch = EPOCH, serverId = SERVER) {
  return {
    server_id: serverId,
    registration_epoch: epoch,
    host_key_fingerprint: "fingerprint",
    endpoint_origin: origin,
    endpoint_generation: 3,
    rooms: [{ room_id: ROOM_A, name: ROOM_A }],
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

const connectedDirectory = (origin = ORIGIN, epoch = EPOCH): CentralDirectoryState => {
  const server = {
    server_id: SERVER,
    registration_epoch: epoch,
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

function setupHarness(account = ACCOUNT): Harness {
  const hook = renderHook(() => {
    const [directory, setDirectory] = useState<CentralDirectoryState>(() => unconfirmedDirectory());
    const [session, setSession] = useState<RoomGuestSession | null>(null);
    const [selectError, setSelectErrorState] = useState<{ roomId: string; message: string } | null>(null);
    const [clock, setClock] = useState(0);
    void clock;
    const hosts = useMemberHosts({ personId: account, sessionToken: "token", directory });
    const admittedSessionToken = session && !roomGuestSessionExpired(session) ? session.sessionToken : "";
    const live = useMemberRoomLive({
      snapshots: hosts.snapshots,
      admissionFor: hosts.admissionFor,
      activeServerId: SERVER,
      activeMeetingId: ROOM_A,
      accountId: account,
      openSocket: backgroundOpen as never,
    });
    useMemberRoomAutoReselect({
      activeRoom: railRoom(ROOM_A, "rail-a") as never,
      ownerServerId: undefined,
      accountId: account,
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
          setSelectErrorState((current) => (update as (c: typeof current) => typeof current)(current));
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

/** The invite join: the new incarnation's channel goes live and the consent
 *  is noted exactly as the join path notes it. */
function joinThroughInvite(admission: FakeAdmission, account = ACCOUNT, serverId = SERVER) {
  const payload = joinFor(ROOM_A) as never;
  const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
  installRemoteWorkspace({
    transport: admission.transport as never,
    owner: null,
    member,
    deviceToken: "device",
    clientId: "client",
  });
  noteMemberJoinConsent(account, {
    server_id: serverId,
    registration_epoch: EPOCH2,
    host_key_fingerprint: "fingerprint",
    endpoint_origin: ORIGIN_NEXT,
    endpoint_generation: 3,
  }, admission, [{ room_id: ROOM_A, name: ROOM_A }]);
}

describe("member host epoch rotation with a fresh invite re-join", () => {
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
    clearMemberJoinHandoffs(ACCOUNT);
    clearMemberJoinHandoffs(OTHER_ACCOUNT);
    saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
    // The silent re-entry refuses a changed identity, exactly like the real
    // attach does, and serves the first incarnation's channel.
    epoch1 = fakeAdmission();
    attach.mockImplementation(async (server: { server_id: string; registration_epoch: string }) => {
      const stored = loadMemberAnchor(ACCOUNT, server.server_id);
      if (stored && stored.registration_epoch !== server.registration_epoch) throw new MemberAnchorMismatchError();
      saveMemberAnchor(ACCOUNT, anchorFor(ORIGIN));
      return { admission: epoch1, rooms: [{ room_id: ROOM_A, name: ROOM_A }], anchor: anchorFor(ORIGIN) };
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    resetMemberRoomAutoReselectBudgetForTests();
  });

  it("(1) the invite re-join after the epoch change heals the open room without a tap and without a new central call", async () => {
    const epoch2 = fakeAdmission(ORIGIN_NEXT);
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    expect(env.result.current.hosts.admissionFor(SERVER)).toBe(epoch1);

    // The host rotates its epoch: the registry refuses it and drops the dead
    // admission, so the session-loss heal has nothing to heal on.
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT, EPOCH2));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(true));
    expect(env.result.current.hosts.admissionFor(SERVER)).toBeNull();

    // The member accepts a new invite for the rotated host: the new
    // incarnation's channel goes live and the consent is noted.
    joinThroughInvite(epoch2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });

    // The registry takes the consented admission over and lifts the refusal
    // on its own: no tap, no 다시 연결, no new central call.
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(false));
    await waitFor(() => expect(env.result.current.hosts.admissionFor(SERVER)).toBe(epoch2));
    expect(epoch2.transport.active).toBe(true);
    expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected");
    const grantsAfterJoin = attach.mock.calls.length;

    // The hourly session-loss heal now finds the live admission: one free
    // select, no central grant, no stop line.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    const selectsBefore = epoch2.selects.length;
    await act(async () => {
      await env.result.current.expireSessions();
    });
    await waitFor(() => expect(epoch2.selects.length).toBe(selectsBefore + 1));
    await waitFor(() =>
      expect(
        env.result.current.session &&
          !roomGuestSessionExpired(env.result.current.session) &&
          env.result.current.session.meetingId === ROOM_A,
      ).toBe(true),
    );
    expect(env.result.current.selectError).toBeNull();
    expect(attach.mock.calls.length).toBe(grantsAfterJoin);
    env.unmount();
  });

  it("(2) an ordinary room select after the epoch refusal does not lift it", async () => {
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));

    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT, EPOCH2));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(true));

    // A plain room tap is not a new invite: the refusal stays and the host
    // keeps asking for a fresh invite.
    await act(async () => {
      await env.result.current.hosts.selectRoom(SERVER, ROOM_A).catch(() => undefined);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.isRefused(SERVER)).toBe(true);
    expect(env.result.current.hosts.admissionFor(SERVER)).toBeNull();
    env.unmount();
  });

  it("(3) a handoff for another server or another person never lifts this host's refusal", async () => {
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT, EPOCH2));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(true));

    // A consent for another server, and a consent recorded for another
    // person, are both somebody else's.
    const other = fakeAdmission(ORIGIN_NEXT);
    noteMemberJoinConsent(ACCOUNT, {
      server_id: OTHER_SERVER,
      registration_epoch: EPOCH2,
      host_key_fingerprint: "fingerprint",
      endpoint_origin: ORIGIN_NEXT,
      endpoint_generation: 3,
    }, other);
    noteMemberJoinConsent(OTHER_ACCOUNT, {
      server_id: SERVER,
      registration_epoch: EPOCH2,
      host_key_fingerprint: "fingerprint",
      endpoint_origin: ORIGIN_NEXT,
      endpoint_generation: 3,
    }, other);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.isRefused(SERVER)).toBe(true);
    expect(env.result.current.hosts.admissionFor(SERVER)).toBeNull();
    env.unmount();
  });

  it("(4) the re-join before the directory catches up still heals, and the handoff waits for the matching epoch", async () => {
    const epoch2 = fakeAdmission(ORIGIN_NEXT);
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));

    // The join lands first: the directory still lists the old epoch, so the
    // consent must not be spent on the wrong incarnation.
    joinThroughInvite(epoch2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(env.result.current.hosts.admissionFor(SERVER)).not.toBe(epoch2);

    // The directory catches up: now the handoff names the live incarnation
    // and the registry takes the admission over, dropping the dead old-epoch
    // entry that shadowed it.
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT, EPOCH2));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(false));
    await waitFor(() => expect(env.result.current.hosts.admissionFor(SERVER)).toBe(epoch2));
    expect(epoch2.transport.active).toBe(true);
    env.unmount();
  });

  it("(5) control: the epoch change without a re-join stays refused and never heals", async () => {
    const env = setupHarness();
    await waitFor(() => expect(env.result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory());
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.session?.meetingId).toBe(ROOM_A));
    await act(async () => {
      env.result.current.setDirectory(connectedDirectory(ORIGIN_NEXT, EPOCH2));
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(env.result.current.hosts.isRefused(SERVER)).toBe(true));
    expect(env.result.current.hosts.admissionFor(SERVER)).toBeNull();

    const grantsBefore = attach.mock.calls.length;
    const selectsBefore = epoch1.selects.length;
    await act(async () => {
      await env.result.current.expireSessions();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(env.result.current.hosts.isRefused(SERVER)).toBe(true);
    expect(env.result.current.hosts.admissionFor(SERVER)).toBeNull();
    expect(attach.mock.calls.length).toBe(grantsBefore);
    expect(epoch1.selects.length).toBe(selectsBefore);
    env.unmount();
  });
});
