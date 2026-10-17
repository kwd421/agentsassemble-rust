import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { createFreshRoom } from "../lib/roomDockModel";
import { useRailRooms } from "./useRailRooms";
import { useServerOpening } from "./useServerOpening";

// Gate straight-in while the local host is still starting and with an
// own-server mismatch + joined member: the shell opens without a manual 열기.
const gateMocks = vi.hoisted(() => ({
  configured: true,
  loggedOut: false,
  session: null as null | { person: { person_id: string; display_name: string } },
  bootstrap: vi.fn(),
  register: vi.fn(),
  openServer: vi.fn(),
  deviceInfo: vi.fn(),
  bootstrapStatus: vi.fn(),
  fetchRooms: vi.fn(),
}));
vi.mock("../lib/central/identity", () => ({
  hasPendingLocalDemotion: vi.fn(() => false),
  retryPendingLocalDemotion: vi.fn(),
  CENTRAL_SESSION_CHANGED_EVENT: "agentsassemble:central-session-changed",
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => gateMocks.configured,
  centralAccountEntryUrl: () => "https://central.example/",
  isCentralWebEntry: () => false,
  centralSessionLoggedOut: () => gateMocks.loggedOut,
  bootstrapCentral: gateMocks.bootstrap,
  loadCentralSession: () => gateMocks.session,
  loadPendingCentralRecoveryCode: () => "",
  logoutCentral: vi.fn(),
  loginCentralGoogle: vi.fn(),
  openCentralOwnedServer: gateMocks.openServer,
  registerLocalServer: gateMocks.register,
  localHostingState: vi.fn().mockResolvedValue(null),
}));
vi.mock("../lib/desktopBridge", () => ({
  isBundledDesktopWebview: () => true,
  isDesktopWebview: () => true,
  requestDesktopServerRestore: vi.fn().mockResolvedValue(null),
  fetchDesktopOperatorRuntime: gateMocks.fetchRooms,
  initializeDesktopBootstrap: vi.fn(),
  requestDesktopBootstrapStatus: gateMocks.bootstrapStatus,
  requestDesktopHostProductSurface: vi.fn().mockResolvedValue({ revision: 1, digest: "1".repeat(64), commands: [] }),
  requestDesktopHostDeviceInfo: gateMocks.deviceInfo,
}));
vi.mock("../lib/deviceIdentity", () => ({
  rememberGuestProfile: vi.fn(),
  getOrCreateBrowserCredential: () => "device-token",
  getOrCreateClientId: () => "client-1",
}));
vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: vi.fn(),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

import StartupIdentityGate from "../views/components/StartupIdentityGate";

const OWN_ID = "10000000-0000-4000-8000-000000000001";
const FOREIGN_ID = "20000000-0000-4000-8000-000000000002";
const MEMBER_ID = "30000000-0000-4000-8000-000000000003";

function memberServer(id = MEMBER_ID) {
  return {
    server_id: id, relation: "member" as const, alias: "friend", icon: "",
    registration_epoch: "epoch", host_key_fingerprint: "fp",
    host_os: null,
  };
}
function ownedServer(id = OWN_ID, endpoint: unknown = null) {
  return {
    server_id: id, relation: "owner" as const, alias: "mine", icon: "",
    registration_epoch: "epoch", host_key_fingerprint: "fp", host_os: "macos" as const,
    endpoint,
  };
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.clearAllMocks();
  gateMocks.configured = true;
  gateMocks.loggedOut = false;
  gateMocks.session = null;
  gateMocks.deviceInfo.mockReset();
  gateMocks.bootstrap.mockReset();
  gateMocks.bootstrapStatus.mockReset();
  gateMocks.fetchRooms.mockReset();
});

describe("startup goes straight in", () => {
  it("opens the shell while the local host is still starting when a joined room exists", async () => {
    const person = { person_id: "me", display_name: "me", identity_kind: "google" as const, avatar_url: null };
    gateMocks.session = { person };
    gateMocks.bootstrap.mockResolvedValue({
      person, servers: [ownedServer(OWN_ID), memberServer()], server_time: 1,
      live: { person, servers: [], server_time: 1 },
    });
    // The local host is still starting: device info never resolves yet.
    gateMocks.deviceInfo.mockImplementation(() => new Promise(() => undefined));
    gateMocks.bootstrapStatus.mockImplementation(() => new Promise(() => undefined));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device" onComplete={onComplete} />);
    await waitFor(() => expect(onComplete).toHaveBeenCalledOnce(), { timeout: 8000 });
  });

  it("opens the shell without a member 열기 when the own server is offline elsewhere", async () => {
    const person = { person_id: "nel", display_name: "nel", identity_kind: "google" as const, avatar_url: null };
    gateMocks.session = { person };
    gateMocks.bootstrap.mockResolvedValue({
      person,
      servers: [ownedServer(OWN_ID, null), memberServer()],
      server_time: 1,
      live: { person, servers: [], server_time: 1 },
    });
    gateMocks.deviceInfo.mockResolvedValue({
      server_id: FOREIGN_ID, host_name: "mac", host_os: "macos",
      device_kind: "Mac", profile_name: "other", hosting_state: null,
    });
    gateMocks.bootstrapStatus.mockResolvedValue({ phase: "complete" });
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device" onComplete={onComplete} />);
    await waitFor(() => expect(onComplete).toHaveBeenCalledOnce(), { timeout: 8000 });
  });
});

describe("rail keeps only mine plus joined rooms", () => {
  it("hides this computer's foreign room while keeping the joined room", async () => {
    gateMocks.session = {
      person: { person_id: "nel", display_name: "nel" },
      token: "token",
    } as unknown as typeof gateMocks.session;
    window.localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
      token: "token", expires_at: 9_999_999_999, device_id: "device",
      person: { person_id: "nel", identity_kind: "google", display_name: "nel" },
    }));
    const { attachMemberHost } = await import("../lib/central/memberAutoAttach");
    vi.mocked(attachMemberHost).mockImplementation((async () => ({
      admission: { transport: { onClose: vi.fn() }, close: vi.fn(), select: vi.fn() },
      rooms: [{ room_id: "hihi-room", name: "hihi" }],
      anchor: { server_id: MEMBER_ID, registration_epoch: "epoch", endpoint_origin: "https://hihi.test" },
    })) as unknown as typeof attachMemberHost);
    const foreignRoom = {
      ...createFreshRoom(), id: "foreign-1", meetingId: "foreign-1",
      label: "foreign", serverId: FOREIGN_ID, roomOrigin: "local" as const,
    };
    const directory = {
      status: "connected" as const,
      person: { person_id: "nel", display_name: "nel", identity_kind: "google" as const },
      servers: [
        { server_id: OWN_ID, relation: "owner" as const, alias: "nel-pc" },
        { server_id: MEMBER_ID, relation: "member" as const, alias: "hihi" },
      ],
      live: {
        person: { person_id: "nel", display_name: "nel", identity_kind: "google" as const },
        servers: [{
          server_id: MEMBER_ID, registration_epoch: "epoch", relation: "member" as const,
          alias: "hihi", host_key_fingerprint: "fp", host_public_key_jwk: {},
          endpoint: { origin: "https://hihi.test", generation: 1, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
        }],
        server_time: 1,
      },
    };
    const { result } = renderHook(() => useRailRooms({
      directoryRooms: [foreignRoom], startupHostEnabled: true, roomDirectorySyncIssue: null,
      centralDirectory: directory, ownerConnect: null,
      admittedSessionToken: "", guestSession: null, guestExpired: false,
    }));
    await waitFor(
      () => expect(result.current.rooms.some((room) => room.meetingId === "hihi-room")).toBe(true),
      { timeout: 5000 },
    );
    expect(result.current.rooms.some((room) => room.meetingId === "foreign-1")).toBe(false);
  });
});

describe("boundary enters the shell without the chooser", () => {
  it("renders the app shell immediately for a signed-in desktop session", async () => {
    const { default: Boundary } = await import("../views/components/StartupIdentityBoundary");
    gateMocks.session = {
      person: { person_id: "me", display_name: "me" },
      token: "token",
    } as unknown as typeof gateMocks.session;
    render(
      <Boundary>
        {() => <main aria-label="app shell" />}
      </Boundary>,
    );
    expect(await screen.findByRole("main", { name: "app shell" })).toBeTruthy();
  });
});

describe("signed-in account and displayed self never disagree", () => {
  it("treats a foreign local server as not mine and shows the central account", async () => {
    const { isLocalOwnedByAccount, visibleLocalRooms, centralAccountSelf } = await import(
      "../lib/central/localOwnership"
    );
    const signedIn = { person_id: "nel", display_name: "nel", identity_kind: "google" as const };
    const directory = {
      status: "connected" as const,
      person: signedIn,
      servers: [{ server_id: OWN_ID, relation: "owner" as const, alias: "nel-pc" }],
      live: null,
    };
    const foreignRoom = {
      ...createFreshRoom(), id: "foreign-1", meetingId: "foreign-1",
      serverId: FOREIGN_ID, roomOrigin: "local" as const,
    };
    expect(isLocalOwnedByAccount([foreignRoom], directory)).toBe(false);
    expect(visibleLocalRooms([foreignRoom], directory)).toHaveLength(0);
    const self = centralAccountSelf({ display_name: signedIn.display_name, avatar_url: null });
    expect(self?.displayName).toBe(signedIn.display_name);
  });
});

describe("rail retry stays in place for member rooms", () => {
  it("retries an anchored member host instead of opening the owner chooser", async () => {
    const retryHost = vi.fn();
    const refreshCentral = vi.fn();
    const { result } = renderHook(() => useServerOpening({
      refreshCentralDirectory: refreshCentral,
      refreshRoomDirectory: vi.fn(),
      captureRoomDirectoryContinuity: vi.fn(),
      hasMemberAnchor: () => true,
      isMemberRefused: () => false,
      retryMemberHost: retryHost,
    }));
    await result.current.retryRoomConnection({
      ...createFreshRoom(), id: "member-1", meetingId: "hihi-room",
      serverId: MEMBER_ID, roomOrigin: "remote_server",
    });
    expect(retryHost).toHaveBeenCalledWith(MEMBER_ID);
    expect(refreshCentral).not.toHaveBeenCalled();
  });
});
