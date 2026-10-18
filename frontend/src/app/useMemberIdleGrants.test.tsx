import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { useMemberRoomAutoReselect } from "./useMemberRoomAutoReselect";
import { RemoteTransport } from "../lib/remote/remoteTransport";
import { closeRemoteWorkspace } from "../lib/remote/remoteWorkspace";
import { resetMemberRoomAutoReselectBudgetForTests } from "../lib/central/memberRoomAutoReselectBudget";
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

const joinFor = (roomId: string, requestId: string) => ({
  server_id: SERVER,
  authority_lineage_id: "22222222-2222-4222-8222-222222222222",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  session_token: `session-${roomId}`,
  agent_id: "member-1",
  display_name: "Member",
  meeting_id: roomId,
  invite_scope: "room",
  participant_type: "human",
  client_type: "browser",
  provider_kind: "manual",
  connection_kind: "native_remote_room_client",
  expires_at: "2099-01-01T00:00:00Z",
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
            ...(url.searchParams.get("room_select_channel_bound") === "1"
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
        const channelBound = url.searchParams.get("room_select_channel_bound") === "1";
        // Mirrors the host contract under test: a lapsed challenge selects
        // only with the channel-bound opt-in on the redeeming channel.
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

function previewCalls() {
  return grantStub.mock.calls.filter((call) => String(call[1]).includes("member-preview"));
}

function selectCalls(options: FakeHostOptions) {
  return options.calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
}

type SocketHandlers = { onError?: (error: unknown) => void };

function socketHandlers(openSocket: { mock: { calls: unknown[][] } }): SocketHandlers[] {
  return openSocket.mock.calls.map((args) => args[2] as SocketHandlers);
}

describe("idle member channel costs zero central (fake timers, real hooks)", () => {
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
            mode: "event_secure_v1",
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

  async function setupIdle(options: FakeHostOptions) {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.spyOn(Math, "random").mockReturnValue(0.5);
    const transport = fakeTransport(options);
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    const openSocket = vi.fn(() => ({ close: vi.fn() }));
    const { result: hookResult, unmount } = renderHook(() => {
      const [guestSession, setGuestSession] = useState<{
        meetingId: string;
        serverSurface: { server_id: string };
      } | null>(null);
      const [admittedSessionToken, setAdmittedSessionToken] = useState("");
      const central = useCentralDirectory(true);
      const hosts = useMemberHosts({
        personId: central.directory?.person?.person_id ?? ACCOUNT,
        sessionToken: "token",
        directory: central.directory,
      });
      const live = useMemberRoomLive({
        snapshots: hosts.snapshots,
        admissionFor: hosts.admissionFor,
        activeServerId: SERVER,
        activeMeetingId: ROOM_A,
        accountId: ACCOUNT,
        openSocket: openSocket as never,
      });
      useMemberRoomAutoReselect({
        activeRoom: {
          id: "rail-a",
          meetingId: ROOM_A,
          serverId: SERVER,
          roomOrigin: "remote_server",
        } as never,
        ownerServerId: undefined,
        accountId: ACCOUNT,
        hasAnchor: hosts.hasAnchor,
        isRefused: hosts.isRefused,
        snapshots: hosts.snapshots,
        admissionFor: hosts.admissionFor,
        guestSession: guestSession as never,
        guestExpired: false,
        admittedSessionToken,
        liveSessionFor: live.liveSessionFor,
        selectRoom: hosts.selectRoom,
        acceptSession: (async (payload: { meeting_id: string; session_token: string }) => {
          setGuestSession({
            meetingId: payload.meeting_id,
            serverSurface: { server_id: SERVER },
          });
          setAdmittedSessionToken(payload.session_token);
          return true;
        }) as never,
        setSelectError: (() => undefined) as never,
      });
      return { central, hosts, live, openSocket };
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    await waitFor(() => expect(hookResult.current.hosts.snapshots[0]?.phase).toBe("connected"));
    await waitFor(() => expect(selectCalls(options).length).toBeGreaterThanOrEqual(2));
    return { result: hookResult, unmount, transport, openSocket };
  }

  it("60 idle minutes with foreground+background rooms spend nothing after attach", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const { result, unmount } = await setupIdle(options);
    const baseGrants = grantCalls().length;
    const basePreviews = previewCalls().length;
    expect(baseGrants).toBeGreaterThan(0);
    expect(basePreviews).toBe(1);
    for (let minute = 0; minute < 60; minute += 1) {
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await vi.advanceTimersByTimeAsync(60_000);
      });
    }
    expect(grantCalls().length).toBe(baseGrants);
    expect(previewCalls().length).toBe(basePreviews);
    expect(result.current.hosts.snapshots[0]?.phase).toBe("connected");
    unmount();
  });

  it("re-tapping the open room after the 300s window costs nothing on a live channel", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const { result, unmount } = await setupIdle(options);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(301_000);
    });
    const baseGrants = grantCalls().length;
    const basePreviews = previewCalls().length;
    const { selectMemberRoomOnDemand } = await import("./useMemberRoomAutoReselect");
    await act(async () => {
      await selectMemberRoomOnDemand({
        roomId: "rail-a",
        meetingId: ROOM_A,
        memberServerId: SERVER,
        liveSessionFor: result.current.live.liveSessionFor,
        selectRoom: result.current.hosts.selectRoom,
        acceptSession: (async () => true) as never,
        setSelectError: (() => undefined) as never,
      });
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(grantCalls().length).toBe(baseGrants);
    expect(previewCalls().length).toBe(basePreviews);
    unmount();
  });

  it("background re-auth after the window costs nothing while membership holds", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const { openSocket, unmount } = await setupIdle(options);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(301_000);
    });
    const baseGrants = grantCalls().length;
    const baseSelects = selectCalls(options).length;
    expect(socketHandlers(openSocket).length).toBeGreaterThan(0);
    await act(async () => {
      for (const handlers of socketHandlers(openSocket)) {
        handlers.onError?.(Object.assign(new Error("denied"), { category: "authorization_failed" }));
      }
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(grantCalls().length).toBe(baseGrants);
    expect(selectCalls(options).length).toBeGreaterThan(baseSelects);
    unmount();
  });

  it("a kick during the hour ends the session without spending a grant", async () => {
    const options: FakeHostOptions = { calls: [], kickedRooms: new Set() };
    const { openSocket, unmount } = await setupIdle(options);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(301_000);
    });
    options.kickedRooms.add(ROOM_B);
    const baseGrants = grantCalls().length;
    const baseSelects = selectCalls(options).length;
    await act(async () => {
      for (const handlers of socketHandlers(openSocket)) {
        handlers.onError?.(Object.assign(new Error("denied"), { category: "authorization_failed" }));
      }
      await vi.advanceTimersByTimeAsync(0);
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(grantCalls().length).toBe(baseGrants);
    expect(selectCalls(options).length).toBeGreaterThan(baseSelects);
    // The refused room never recovers on its own: no retry storm.
    const parkedSelects = selectCalls(options).length;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300_000);
    });
    expect(selectCalls(options).length).toBe(parkedSelects);
    expect(grantCalls().length).toBe(baseGrants);
    unmount();
  });

});
