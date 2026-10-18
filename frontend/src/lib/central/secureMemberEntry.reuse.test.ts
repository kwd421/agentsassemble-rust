import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadCentralSession, signedRequest } from "./identity";
import { RemoteTransport } from "../remote/remoteTransport";
import {
  closeRemoteWorkspace,
  installRemoteWorkspace,
  remoteSessionTransport,
  remoteWorkspaceFor,
} from "../remote/remoteWorkspace";
import { TEST_SERVER_PRODUCT_SURFACE } from "../../test/serverProductSurface";
import { SecureMemberAdmission } from "./secureMemberEntry";

vi.mock("./identity", () => ({ loadCentralSession: vi.fn(), signedRequest: vi.fn() }));
vi.mock("../deviceIdentity", () => ({
  getOrCreateBrowserCredential: () => "device",
  getOrCreateClientId: () => "client",
}));

const session = { token: "central-token" };
const entry = { server_id: "11111111-1111-4111-8111-111111111111", registration_epoch: "epoch" };
const hello = {
  server_id: "11111111-1111-4111-8111-111111111111",
  protocol: "secure_admission_v1",
  client_public_key: "client-key",
  channel_id: "channel-1",
  origin: "https://host.test",
  generation: 3,
};
const target = {
  server_id: entry.server_id,
  registration_epoch: "epoch",
  origin: "https://host.test",
  generation: 3,
  host_public_key_jwk: {},
  host_key_fingerprint: "fingerprint",
};

const challengeFor = (id: string) => ({
  challenge_id: id,
  challenge_hash: "a".repeat(43),
  server_id: entry.server_id,
  registration_epoch: "epoch",
  expires_at: Math.floor(Date.now() / 1000) + 300,
});

const grantFor = () => ({
  protocol: hello.protocol,
  client_public_key: hello.client_public_key,
  channel_id: hello.channel_id,
  server_id: entry.server_id,
  registration_epoch: "epoch",
  endpoint_origin: hello.origin,
  endpoint_generation: hello.generation,
  purpose: "connect",
  grant_token: `aamc1.${"g".repeat(43)}`,
  expires_at: Math.floor(Date.now() / 1000) + 300,
});

const joinFor = (roomId: string, requestId: string) => ({
  server_id: entry.server_id,
  authority_lineage_id: "22222222-2222-4222-8222-222222222222",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  session_token: `foreground-session-${roomId}`,
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

function liveTransport() {
  const state = { active: true };
  const transport = {
    hello,
    get active() { return state.active; },
    fetch: vi.fn(),
    close: vi.fn(() => { state.active = false; }),
    onClose: vi.fn(),
  };
  return { transport, state };
}

async function openAdmissionWithRooms(
  calls: { path: string; body: unknown }[],
  rooms: { room_id: string; name: string }[],
  transport: { hello: unknown; active: boolean; fetch: ReturnType<typeof vi.fn>; close: ReturnType<typeof vi.fn>; onClose: ReturnType<typeof vi.fn> },
) {
  transport.fetch.mockImplementation(async (path: string, init?: RequestInit) => {
    const body = init?.body ? JSON.parse(String(init.body)) : {};
    calls.push({ path, body });
    if (path === "/api/member-connect/challenge") {
      return new Response(JSON.stringify(challengeFor(`challenge-${calls.length}`)));
    }
    if (path.startsWith("/api/member-connect/rooms")) {
      return new Response(JSON.stringify({ rooms, room_select_reuse: true }));
    }
    if (path === "/api/member-connect/select") {
      return new Response(JSON.stringify(joinFor(body.room_id, `request-${calls.length}`)));
    }
    throw new Error(`unexpected host path ${path}`);
  });
  vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
  vi.mocked(signedRequest).mockResolvedValue(grantFor() as never);
  const admission = await SecureMemberAdmission.open(entry, target as never, "central-token", new AbortController().signal);
  return admission;
}

describe("foreground select on a live reused admission", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    vi.mocked(loadCentralSession).mockReturnValue(session as never);
    vi.restoreAllMocks();
    vi.mocked(loadCentralSession).mockReturnValue(session as never);
  });

  it("reuses the live channel for room B: no close, A stays usable, B binds, no extra grants", async () => {
    const calls: { path: string; body: unknown }[] = [];
    const { transport } = liveTransport();
    const admission = await openAdmissionWithRooms(
      calls,
      [{ room_id: "general", name: "일반" }, { room_id: "second", name: "둘째" }],
      transport as never,
    );
    await admission.admit(new AbortController().signal);
    const grantsAfterAdmit = vi.mocked(signedRequest).mock.calls.length;
    expect(grantsAfterAdmit).toBeGreaterThan(0);

    // Foreground select of room A installs the workspace on the live channel.
    const first = await admission.select("general", new AbortController().signal);
    expect(first.meeting_id).toBe("general");
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(transport);
    const workspaceSessionA = remoteWorkspaceFor(entry.server_id)?.member?.sessionToken;
    expect(workspaceSessionA).toBe("foreground-session-general");
    expect(transport.close).not.toHaveBeenCalled();

    const grantsAfterFirst = vi.mocked(signedRequest).mock.calls.length;

    // Foreground select of room B with no background session reuses the same
    // live challenge: it must not re-install (and therefore never close) the
    // transport, only bind the new room session.
    const second = await admission.select("second", new AbortController().signal);
    expect(second.meeting_id).toBe("second");
    expect(transport.close).not.toHaveBeenCalled();
    expect(transport.active).toBe(true);
    // Room A session still usable on the same live channel.
    expect(remoteSessionTransport("foreground-session-general")).toBe(transport);
    // Room B bound on the same live channel.
    expect(remoteSessionTransport("foreground-session-second")).toBe(transport);
    // No extra central preview/connect-grant calls for the reused select.
    expect(vi.mocked(signedRequest).mock.calls.length).toBe(grantsAfterFirst);
    // The workspace keeps the live transport (foreground session stays A;
    // B works through its own bound session).
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(transport);
    admission.close();
    vi.restoreAllMocks();
  });

  it("a select on a genuinely new transport replaces and closes the old one", async () => {
    const callsA: { path: string; body: unknown }[] = [];
    const first = liveTransport();
    const admissionA = await openAdmissionWithRooms(
      callsA,
      [{ room_id: "general", name: "일반" }],
      first.transport as never,
    );
    await admissionA.admit(new AbortController().signal);
    await admissionA.select("general", new AbortController().signal);
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(first.transport);

    // Stale challenge recovery on a fresh channel: a new admission installs
    // its own transport and closes the superseded one.
    const callsB: { path: string; body: unknown }[] = [];
    const second = liveTransport();
    const admissionB = await openAdmissionWithRooms(
      callsB,
      [{ room_id: "general", name: "일반" }],
      second.transport as never,
    );
    await admissionB.admit(new AbortController().signal);
    await admissionB.select("general", new AbortController().signal);
    expect(first.transport.close).toHaveBeenCalledTimes(1);
    expect(second.transport.close).not.toHaveBeenCalled();
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(second.transport);
    admissionA.close();
    admissionB.close();
    vi.restoreAllMocks();
  });

  it("installing the same transport object never closes it; a different one does", async () => {
    const first = liveTransport();
    (first.transport as { hello: typeof hello }).hello = { ...hello };
    installRemoteWorkspace({
      transport: first.transport as never,
      owner: null,
      member: { sessionToken: "token-a", serverSurface: { server_id: entry.server_id } } as never,
      deviceToken: "device",
      clientId: "client",
    });
    installRemoteWorkspace({
      transport: first.transport as never,
      owner: null,
      member: { sessionToken: "token-b", serverSurface: { server_id: entry.server_id } } as never,
      deviceToken: "device",
      clientId: "client",
    });
    expect(first.transport.close).not.toHaveBeenCalled();
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(first.transport);
    expect(remoteSessionTransport("token-b")).toBe(first.transport);

    const second = liveTransport();
    (second.transport as { hello: typeof hello }).hello = { ...hello };
    installRemoteWorkspace({
      transport: second.transport as never,
      owner: null,
      member: { sessionToken: "token-c", serverSurface: { server_id: entry.server_id } } as never,
      deviceToken: "device",
      clientId: "client",
    });
    expect(first.transport.close).toHaveBeenCalledTimes(1);
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(second.transport);
  });

  it("9 joined rooms: selecting the 9th without a background session keeps the channel", async () => {
    const rooms = Array.from({ length: 9 }, (_, i) => ({ room_id: `room-${i + 1}`, name: `방 ${i + 1}` }));
    const calls: { path: string; body: unknown }[] = [];
    const { transport } = liveTransport();
    const admission = await openAdmissionWithRooms(calls, rooms, transport as never);
    await admission.admit(new AbortController().signal);
    // Only the first 8 rooms get background sockets; the 9th has no live
    // background session, so the foreground tap selects on demand.
    await admission.select("room-1", new AbortController().signal);
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(transport);
    const grantsBeforeNinth = vi.mocked(signedRequest).mock.calls.length;
    const ninth = await admission.select("room-9", new AbortController().signal);
    expect(ninth.meeting_id).toBe("room-9");
    expect(transport.close).not.toHaveBeenCalled();
    expect(transport.active).toBe(true);
    expect(remoteSessionTransport("foreground-session-room-1")).toBe(transport);
    expect(remoteSessionTransport("foreground-session-room-9")).toBe(transport);
    expect(vi.mocked(signedRequest).mock.calls.length).toBe(grantsBeforeNinth);
    expect(remoteWorkspaceFor(entry.server_id)?.transport).toBe(transport);
    admission.close();
    vi.restoreAllMocks();
  });
});
