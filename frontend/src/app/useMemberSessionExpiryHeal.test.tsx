import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { useMemberRoomAutoReselect } from "./useMemberRoomAutoReselect";
import { useCanonicalRoom } from "../useCanonicalRoom";
import { NO_SURFACE_STREAMS } from "./remoteRoomAuthority";
import type { RoomSocketHandlers } from "../roomSocketTypes";
import { RemoteTransport } from "../lib/remote/remoteTransport";
import { closeRemoteWorkspace } from "../lib/remote/remoteWorkspace";
import {
  roomGuestSessionExpired,
  roomGuestSessionFromJoinPayload,
  type RoomGuestSession,
} from "../lib/roomGuestSession";
import {
  canonicalRoomAuthFor,
  remoteRoomSessionReady,
  serverProductSurfaceFor,
} from "./remoteRoomAuthority";
import { ApiError } from "../lib/apiErrors";
import {
  MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP,
  MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE,
  memberRoomAutoReselectCount,
  resetMemberRoomAutoReselectBudgetForTests,
} from "../lib/central/memberRoomAutoReselectBudget";
import { resetMemberHostOfflineBudgetForTests } from "../lib/central/memberRoomLive";
import { resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const ACCOUNT = "person";
const SERVER = "33333333-3333-4333-8333-333333333333";
const EPOCH = "epoch-grants";
const ORIGIN = "https://friend-grants.test";
const ROOM_A = "room-a";
const ROOM_B = "room-b";

vi.mock("../lib/central/identity", async (original) => {
  const actual = await original<typeof import("../lib/central/identity")>();
  return {
    ...actual,
    bootstrapCentral: (...args: unknown[]) => bootstrap(...args),
    centralIdentityConfigured: () => true,
    loadCentralSession: () => ({
      token: "token",
      person: { person_id: ACCOUNT, display_name: "나", identity_kind: "google" },
      device_id: "device",
      expires_at: 9_999_999_999,
    }),
    isCentralAuthenticationError: () => false,
    signedRequest: (...args: unknown[]) => grantStub(...args),
  };
});

vi.mock("../lib/central/memberAutoAttach", async (original) => {
  const actual = await original<typeof import("../lib/central/memberAutoAttach")>();
  const entry = await import("../lib/central/secureMemberEntry");
  return {
    ...actual,
    attachMemberHost: (server: unknown, token: string, person: string, signal: AbortSignal) =>
      actual.attachMemberHost(server as never, token, person, signal, {
        preview: entry.previewSecureMember,
        open: (e, t, s, sig) => entry.SecureMemberAdmission.open(e, t, s, sig),
        probe: async () => null,
      }),
  };
});

vi.mock("../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: () => "device",
  getOrCreateClientId: () => "client",
}));

const bootstrap = vi.fn();
const grantStub = vi.fn();

type FakeHostOptions = {
  calls: { path: string; body: unknown }[];
  kickedRooms: Set<string>;
  oldHost?: boolean;
};

const challenges = new Map<string, number>();

function challengeFor(id: string) {
  const expires_at = Math.floor(Date.now() / 1000) + 300;
  challenges.set(id, expires_at);
  return {
    challenge_id: id,
    challenge_hash: "a".repeat(43),
    server_id: SERVER,
    registration_epoch: EPOCH,
    expires_at,
  };
}

function grantFromBody(body: Record<string, unknown>) {
  return {
    protocol: "secure_admission_v1",
    client_public_key: body.client_public_key,
    channel_id: body.channel_id,
    server_id: SERVER,
    registration_epoch: EPOCH,
    endpoint_origin: body.origin,
    endpoint_generation: body.generation,
    purpose: "connect",
    grant_token: `aamc1.${"g".repeat(43)}`,
    expires_at: Math.floor(Date.now() / 1000) + 300,
  };
}

// Sessions minted by the fake host carry a REAL one-hour expiry, exactly like
// SESSION_TTL on the server: the bearer lapses by the clock while the host
// session row and the secure channel stay live.
const joinFor = (roomId: string, requestId: string) => ({
  server_id: SERVER,
  authority_lineage_id: "22222222-2222-4222-8222-222222222222",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  session_token: `session-${roomId}-${requestId}`,
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
  request_id: requestId,
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
});

const hello = {
  server_id: SERVER,
  protocol: "secure_admission_v1",
  client_public_key: "client-key",
  channel_id: "channel-1",
  origin: ORIGIN,
  generation: 3,
};

function fakeTransport(options: FakeHostOptions) {
  const state = { active: true };
  const selected = new Map<string, string>();
  const transport = {
    hello,
    get active() {
      return state.active;
    },
    fetch: vi.fn(async (path: string, init?: RequestInit) => {
      const url = new URL(path, ORIGIN);
      const body = init?.body ? JSON.parse(String(init.body)) : {};
      options.calls.push({ path: url.pathname + url.search, body });
      if (url.pathname === "/api/member-connect/challenge") {
        const id = `challenge-${options.calls.length}`;
        return new Response(JSON.stringify(challengeFor(id)));
      }
      if (url.pathname === "/api/member-connect/rooms") {
        const known = challenges.get(String(body.challenge_id));
        if (!known) return new Response("no", { status: 401 });
        const listed = [ROOM_A, ROOM_B].filter((room) => !options.kickedRooms.has(room));
        return new Response(
          JSON.stringify({
            rooms: listed.map((room_id) => ({ room_id, name: room_id })),
            room_select_reuse: true,
            // An old host omits the channel-bound advertisement even when the
            // client opts in via the query string.
            ...(options.oldHost
              ? {}
              : url.searchParams.get("room_select_channel_bound") === "1"
                ? { room_select_channel_bound: true }
                : {}),
          })
        );
      }
      if (url.pathname === "/api/member-connect/select") {
        const roomId = String(body.room_id);
        if (options.kickedRooms.has(roomId) || (roomId !== ROOM_A && roomId !== ROOM_B)) {
          return new Response(
            JSON.stringify({ error: "ended", code: "member_membership_ended" }),
            { status: 403 }
          );
        }
        const known = challenges.get(String(body.challenge_id));
        const live = known !== undefined && known > Date.now() / 1000;
        // An old host ignores the channel-bound opt-in: past the window the
        // lapsed challenge is refused and the caller re-admits with a grant.
        const channelBound =
          !options.oldHost && url.searchParams.get("room_select_channel_bound") === "1";
        if (!live && !channelBound) return new Response("stale", { status: 401 });
        const prior = selected.get(roomId);
        if (prior && prior !== String(body.client_id)) {
          return new Response("foreign", { status: 401 });
        }
        selected.set(roomId, String(body.client_id));
        return new Response(JSON.stringify(joinFor(roomId, `request-${options.calls.length}`)));
      }
      throw new Error(`unexpected host path ${path}`);
    }),
    close: vi.fn(() => {
      state.active = false;
    }),
    onClose: vi.fn(),
  };
  return transport;
}

function grantCalls() {
  return grantStub.mock.calls.filter((call) => String(call[1]).includes("member-connect-grants"));
}

function selectCalls(options: FakeHostOptions) {
  return options.calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
}

const railRoomA = {
  id: "rail-a",
  label: "friend",
  meetingId: ROOM_A,
  serverId: SERVER,
  roomOrigin: "remote_server" as const,
  connectionState: "connected" as const,
  topic: "",
  shortLabel: "F",
  icon: (() => null) as never,
  createdAt: "",
  tone: "fresh" as const,
};

type HealEnv = {
  options: FakeHostOptions;
  result: { current: { session: RoomGuestSession | null; roomId: string; selectError: { roomId: string; message: string } | null } };
  unmount: () => void;
  foregroundHandlers: RoomSocketHandlers[];
  degradeSnapshots: () => Promise<void>;
  expireByTtl: () => Promise<void>;
};

describe("foreground session-loss heal on a live channel (real hooks)", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    bootstrap.mockReset();
    grantStub.mockReset();
    challenges.clear();
    resetCentralRequestGovernorForTests();
    resetMemberHostOfflineBudgetForTests();
    resetMemberRoomAutoReselectBudgetForTests();
    bootstrap.mockImplementation(async () => ({
      person: { person_id: ACCOUNT, display_name: "나", identity_kind: "google" },
      servers: [
        {
          server_id: SERVER,
          registration_epoch: EPOCH,
          relation: "member" as const,
          alias: "친구",
          host_key_fingerprint: "fingerprint",
          host_public_key_jwk: {},
          endpoint: {
            origin: ORIGIN,
            generation: 3,
            status: "published" as const,
            mode: "event_secure_v1" as const,
            protocol: "secure_admission_v1",
          },
        },
      ],
      server_time: 1,
    }));
    grantStub.mockImplementation(
      async (_session: unknown, path: string, _method: string, body?: Record<string, unknown>) => {
        if (String(path).includes("member-preview")) {
          return {
            server_id: SERVER,
            label: "친구",
            endpoint_origin: ORIGIN,
            endpoint_generation: 3,
            protocol: "secure_admission_v1",
            mode: "event_secure_v1",
            registration_epoch: EPOCH,
            host_key_fingerprint: "fingerprint",
            host_public_key_jwk: {},
          };
        }
        return grantFromBody(body ?? {});
      }
    );
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    resetMemberRoomAutoReselectBudgetForTests();
  });

  // The owner stack, faithfully: the joined session object is RETAINED at TTL
  // and only the admitted bearer lapses by the clock (useRoomAdmission derives
  // admittedSessionToken from roomGuestSessionExpired; guestExpired stays
  // false because expireGuestSession never runs). The foreground socket is the
  // real useCanonicalRoom: at SESSION_TTL the server drops the connection with
  // NO error frame (wait_for_session_expiry returns), and the reconnect ticket
  // then 401s while the bearer is already lapsed, so the owner's onUnauthorized
  // (wired only while a token is admitted) is unset and nothing expires the
  // admission. The room view (roomId "") is derived exactly like
  // useAppController (operationally disconnected -> no room/auth/surface).
  async function setupHeal(options: FakeHostOptions): Promise<HealEnv> {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    const transport = fakeTransport(options);
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    const foregroundHandlers: RoomSocketHandlers[] = [];
    const foregroundOpen = vi.fn((_auth: unknown, _streams: unknown, handlers: RoomSocketHandlers) => {
      foregroundHandlers.push(handlers);
      // The socket establishes asynchronously like a real transport.
      Promise.resolve().then(() => handlers.onOpen?.());
      return { close: vi.fn(), resync: vi.fn(), ready: () => true };
    });
    const backgroundOpen = vi.fn(() => ({ close: vi.fn() }));
    const { result, unmount } = renderHook(() => {
      const [session, setSession] = useState<RoomGuestSession | null>(null);
      const [selectError, setSelectErrorState] = useState<{
        roomId: string;
        message: string;
      } | null>(null);
      // Split-brain precondition from the real app: the member host snapshot
      // stops reporting connected while the adopted admission channel stays
      // live. The rail/socket owner then tears down with the session still
      // valid (hasAuth:false, hasSurface:true) and the phase-gated heal parks.
      const [degraded, setDegraded] = useState(false);
      const central = useCentralDirectory(true);
      const hosts = useMemberHosts({
        personId: central.directory?.person?.person_id ?? ACCOUNT,
        sessionToken: "token",
        directory: central.directory,
      });
      const snapshots = degraded ? [] : hosts.snapshots;
      const admittedSessionToken =
        session && !roomGuestSessionExpired(session) ? session.sessionToken : "";
      const guestExpired = false;
      const activeRoomSession =
        session && admittedSessionToken && session.meetingId === ROOM_A &&
        session.serverSurface.server_id === SERVER
          ? session
          : null;
      const sessionReady = remoteRoomSessionReady(railRoomA, {
        guestLocked: Boolean(session),
        sessionToken: admittedSessionToken,
        session: activeRoomSession,
        expired: guestExpired,
      });
      const serverSurface = serverProductSurfaceFor(
        sessionReady, activeRoomSession, session, true, null);
      const operationallyDisconnected = !sessionReady;
      const roomId = operationallyDisconnected ? "" : ROOM_A;
      const auth = serverSurface
        ? canonicalRoomAuthFor(
            railRoomA,
            { operational: operationallyDisconnected, selectError: Boolean(selectError), guestLocked: Boolean(session) },
            admittedSessionToken,
            "device",
          )
        : undefined;
      const canonical = useCanonicalRoom({
        roomId,
        auth,
        streams: serverSurface?.websocket_streams ?? NO_SURFACE_STREAMS,
        serverSurface: serverSurface ?? null,
        viewerParticipantId: session?.agentId || "operator-local",
        openSocket: foregroundOpen as never,
      });
      void canonical.connectionState;
      const live = useMemberRoomLive({
        snapshots,
        admissionFor: hosts.admissionFor,
        activeServerId: SERVER,
        activeMeetingId: ROOM_A,
        accountId: ACCOUNT,
        openSocket: backgroundOpen as never,
      });
      useMemberRoomAutoReselect({
        activeRoom: railRoomA as never,
        ownerServerId: undefined,
        accountId: ACCOUNT,
        hasAnchor: hosts.hasAnchor,
        isRefused: hosts.isRefused,
        snapshots,
        admissionFor: hosts.admissionFor,
        guestSession: session as never,
        guestExpired,
        admittedSessionToken,
        liveSessionFor: live.liveSessionFor,
        selectRoom: hosts.selectRoom,
        acceptSession: (async (payload: never) => {
          const next = { ...roomGuestSessionFromJoinPayload("", payload as never), centralMember: true };
          if (roomGuestSessionExpired(next)) return false;
          setSession(next);
          return true;
        }) as never,
        setSelectError: ((update: unknown) => {
          if (typeof update === "function") {
            setSelectErrorState((current) =>
              (update as (c: typeof current) => typeof current)(current)
            );
          } else {
            setSelectErrorState(update as typeof selectError);
          }
        }) as never,
      });
      return {
        hosts,
        session,
        roomId,
        selectError,
        degrade: () => setDegraded(true),
      };
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(result.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await waitFor(() => expect(selectCalls(options).length).toBeGreaterThanOrEqual(2));
    // The foreground open completes through the same heal path: a live
    // session is now armed for the next loss.
    await waitFor(() => expect(result.current.session?.meetingId).toBe(ROOM_A));
    await waitFor(() => expect(result.current.roomId).toBe(ROOM_A));
    return {
      options,
      result: result as HealEnv["result"],
      unmount,
      foregroundHandlers,
      degradeSnapshots: async () => {
        await act(async () => {
          result.current.degrade();
          await vi.advanceTimersByTimeAsync(0);
        });
      },
      expireByTtl: async () => {
        // SESSION_TTL passes: the bearer lapses by the clock (the component
        // re-renders only when the socket below drops, as in the app).
        await act(async () => {
          await vi.advanceTimersByTimeAsync(3_601_000);
        });
        // The server drops the room connection with no frame; the owner
        // re-renders, the bearer is gone, and the room shows disconnected.
        await act(async () => {
          foregroundHandlers[foregroundHandlers.length - 1]?.onClose?.();
          await vi.advanceTimersByTimeAsync(0);
        });
        // The reconnect ticket 401s on the lapsed bearer. The owner wires
        // onUnauthorized only while a token is admitted, so it is unset and
        // the admission stays joined (session retained, not expired).
        await act(async () => {
          foregroundHandlers[foregroundHandlers.length - 1]?.onError?.(
            new ApiError(401, "denied", "authorization_failed")
          );
          await vi.advanceTimersByTimeAsync(0);
        });
      },
    };
  }

  it("(1) real TTL expiry with a stale host snapshot heals once over the live channel with zero central", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const env = await setupHeal(options);
    await env.degradeSnapshots();
    const baseGrants = grantCalls().length;
    const baseSelects = selectCalls(options).length;
    expect(baseGrants).toBeGreaterThan(0);

    await env.expireByTtl();
    // The loss heals with exactly one automatic channel-bound select,
    // spending no budget (the disconnected roomId "" only flashes between
    // the socket close and the select landing, too fast to assert here).
    await waitFor(() => expect(selectCalls(options).length).toBe(baseSelects + 1));
    await waitFor(() => expect(env.result.current.roomId).toBe(ROOM_A));
    await waitFor(() => expect(
      env.result.current.session && !roomGuestSessionExpired(env.result.current.session)
    ).toBe(true));
    expect(env.result.current.selectError).toBeNull();
    expect(grantCalls().length).toBe(baseGrants);
    // No second attempt on the same loss: the count stays parked.
    const parkedSelects = selectCalls(options).length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_000);
    });
    expect(selectCalls(options).length).toBe(parkedSelects);
    expect(grantCalls().length).toBe(baseGrants);
    env.unmount();
  });

  it("(2) real TTL expiry with a connected snapshot heals over the live channel", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const env = await setupHeal(options);
    const baseGrants = grantCalls().length;
    const baseSelects = selectCalls(options).length;

    await env.expireByTtl();
    await waitFor(() => expect(selectCalls(options).length).toBe(baseSelects + 1));
    await waitFor(() => expect(
      env.result.current.session && !roomGuestSessionExpired(env.result.current.session)
    ).toBe(true));
    expect(env.result.current.selectError).toBeNull();
    expect(grantCalls().length).toBe(baseGrants);
    env.unmount();
  });

  it("(3) a refused re-select (kicked) never retries and parks with the stop line", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const env = await setupHeal(options);
    await env.degradeSnapshots();
    const baseGrants = grantCalls().length;
    const baseSelects = selectCalls(options).length;
    options.kickedRooms.add(ROOM_A);

    await env.expireByTtl();
    await waitFor(() => expect(selectCalls(options).length).toBe(baseSelects + 1));
    await waitFor(() => expect(env.result.current.selectError).not.toBeNull());
    expect(env.result.current.selectError).toEqual({
      roomId: "rail-a",
      message: MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE,
    });
    // The refused select refreshes the host room list once (existing
    // terminal-error path) so the kicked room can leave the rail: at most
    // that single refresh grant, never a retry storm.
    expect(grantCalls().length - baseGrants).toBeLessThanOrEqual(1);
    const grantsAfterRefusal = grantCalls().length;
    // A persistently refused room never loops: advancing time adds no select.
    const parkedSelects = selectCalls(options).length;
    const parkedBudget = memberRoomAutoReselectCount(ACCOUNT, SERVER, ROOM_A);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600_000);
    });
    expect(selectCalls(options).length).toBe(parkedSelects);
    expect(grantCalls().length).toBe(grantsAfterRefusal);
    expect(memberRoomAutoReselectCount(ACCOUNT, SERVER, ROOM_A)).toBe(parkedBudget);
    env.unmount();
  });

  it("(4) 24 hourly expiries on a live channel-bound admission all heal with zero central and no cap", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const env = await setupHeal(options);
    await env.degradeSnapshots();
    const baseGrants = grantCalls().length;
    for (let hour = 0; hour < 24; hour += 1) {
      const before = selectCalls(options).length;
      await env.expireByTtl();
      // Each hourly loss heals with exactly one free channel-bound select.
      await waitFor(() => expect(selectCalls(options).length).toBe(before + 1));
      await waitFor(() => expect(
        env.result.current.session && !roomGuestSessionExpired(env.result.current.session)
      ).toBe(true));
      expect(env.result.current.selectError).toBeNull();
    }
    expect(grantCalls().length).toBe(baseGrants);
    expect(memberRoomAutoReselectCount(ACCOUNT, SERVER, ROOM_A)).toBeLessThan(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP
    );
    expect(memberRoomAutoReselectCount(ACCOUNT, SERVER, ROOM_B)).toBeLessThan(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP
    );
    env.unmount();
  });

  it("(5) paid retries on an old host stay within the daily cap", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set(), oldHost: true };
    const env = await setupHeal(options);
    await env.degradeSnapshots();
    // Move past the 300s reuse window so every heal needs a fresh grant.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(301_000);
    });
    const baseGrants = grantCalls().length;
    let heals = 0;
    for (let attempt = 0; attempt < 8; attempt += 1) {
      await env.expireByTtl();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      if (env.result.current.session &&
          !roomGuestSessionExpired(env.result.current.session) &&
          env.result.current.selectError === null) {
        heals += 1;
      }
      // After the cap parks, the room stays on the disconnected stop line.
      if (env.result.current.selectError !== null) break;
    }
    expect(heals).toBeLessThanOrEqual(MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP);
    expect(memberRoomAutoReselectCount(ACCOUNT, SERVER, ROOM_A)).toBeLessThanOrEqual(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP
    );
    // Paid heals spend at most one retry grant each: bounded by the cap.
    expect(grantCalls().length - baseGrants).toBeLessThanOrEqual(
      MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP
    );
    const parkedSelects = selectCalls(options).length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(600_000);
    });
    expect(selectCalls(options).length).toBe(parkedSelects);
    env.unmount();
  });
});
