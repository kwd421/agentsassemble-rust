import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadCentralSession, signedRequest } from "./identity";
import { RemoteTransport } from "../remote/remoteTransport";
import { closeRemoteWorkspace, remoteSessionTransport, remoteWorkspaceFor } from "../remote/remoteWorkspace";
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
  session_token: `background-session-${roomId}`,
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

describe("member background room select", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    vi.mocked(loadCentralSession).mockReturnValue(session as never);
  });

  async function openAdmission(calls: { path: string; body: unknown }[], { reuse = false, channelBound = false } = {}) {
    const fetch = vi.fn(async (path: string, init?: RequestInit) => {
      const body = init?.body ? JSON.parse(String(init.body)) : {};
      calls.push({ path, body });
      if (path === "/api/member-connect/challenge") {
        return new Response(JSON.stringify(challengeFor(`challenge-${calls.length}`)));
      }
      if (path.startsWith("/api/member-connect/rooms")) {
        return new Response(
          JSON.stringify({ rooms: [{ room_id: "general", name: "일반" }, { room_id: "second", name: "둘째" }], ...(reuse ? { room_select_reuse: true } : {}), ...(channelBound ? { room_select_reuse: true, room_select_channel_bound: true } : {}) })
        );
      }
      if (path.startsWith("/api/member-connect/select")) {
        return new Response(JSON.stringify(joinFor(body.room_id, `request-${calls.length}`)));
      }
      throw new Error(`unexpected host path ${path}`);
    });
    const transport = { hello, active: true, fetch, close: vi.fn(), onClose: vi.fn() };
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    vi.mocked(signedRequest).mockResolvedValue(grantFor() as never);
    const admission = await SecureMemberAdmission.open(entry, target as never, "central-token", new AbortController().signal);
    return { admission, transport, fetch };
  }

  it("keeps the connect admit path on member-connect grants after the grant refactor", async () => {
    const calls: { path: string; body: unknown }[] = [];
    const { admission } = await openAdmission(calls);
    const rooms = await admission.admit(new AbortController().signal);
    expect(rooms).toEqual([
      { room_id: "general", name: "일반" },
      { room_id: "second", name: "둘째" },
    ]);
    const grantCall = vi.mocked(signedRequest).mock.calls[0];
    expect(String(grantCall[1])).toContain("member-connect-grants");
    admission.close();
    vi.restoreAllMocks();
  });

  it("selects another joined room over the same channel with its own challenge cycle", async () => {
    const calls: { path: string; body: unknown }[] = [];
    const { admission, transport } = await openAdmission(calls);
    await admission.admit(new AbortController().signal);
    // The new client opts into the capability on the URL so an old host
    // ignores it and keeps serving the exact old shape.
    const redeemPaths = calls.filter((call) => call.path.startsWith("/api/member-connect/rooms")).map((call) => call.path);
    expect(redeemPaths).toHaveLength(1);
    expect(redeemPaths[0]).toContain("room_select_reuse=1");
    const primaryChallenge = (calls.find((call) => call.path.startsWith("/api/member-connect/rooms"))?.body as { challenge_id: string }).challenge_id;

    const parsed = await admission.selectBackgroundRoom("second", new AbortController().signal);
    expect(parsed.meeting_id).toBe("second");
    expect(parsed.session_token).toBe("background-session-second");
    // The background select ran a fresh challenge/grant cycle and bound the
    // room session for socket tickets on the same transport, without
    // installing a workspace (the foreground session keeps that slot).
    const backgroundRooms = calls.filter((call) => call.path.startsWith("/api/member-connect/rooms"));
    expect(backgroundRooms).toHaveLength(2);
    const backgroundChallenge = (backgroundRooms[1].body as { challenge_id: string }).challenge_id;
    expect(backgroundChallenge).not.toBe(primaryChallenge);
    const backgroundSelect = calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
    expect(backgroundSelect).toHaveLength(1);
    expect(backgroundSelect[0].body).toMatchObject({ challenge_id: backgroundChallenge, room_id: "second" });
    expect(remoteSessionTransport("background-session-second")).toBe(transport);
    expect(remoteWorkspaceFor(entry.server_id)).toBeNull();
    // The primary admission challenge is untouched: a later foreground
    // select still goes out on the original challenge.
    await admission.select("general", new AbortController().signal).catch(() => undefined);
    const selects = calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
    expect(selects[selects.length - 1].body).toMatchObject({ challenge_id: primaryChallenge, room_id: "general" });
    admission.close();
    vi.restoreAllMocks();
  });

  it("reuses one redeemed challenge for every room when the host advertises it", async () => {
    const calls: { path: string; body: unknown }[] = [];
    const { admission, transport } = await openAdmission(calls, { reuse: true });
    await admission.admit(new AbortController().signal);
    const primaryChallenge = (calls.find((call) => call.path.startsWith("/api/member-connect/rooms"))?.body as { challenge_id: string }).challenge_id;
    const grantsBefore = vi.mocked(signedRequest).mock.calls.length;

    const parsed = await admission.selectBackgroundRoom("second", new AbortController().signal);
    expect(parsed.meeting_id).toBe("second");
    expect(parsed.session_token).toBe("background-session-second");
    // The reused challenge spends no new central grant: one challenge, one
    // redeem and one grant for the whole admission.
    expect(calls.filter((call) => call.path === "/api/member-connect/challenge")).toHaveLength(1);
    expect(calls.filter((call) => call.path.startsWith("/api/member-connect/rooms"))).toHaveLength(1);
    expect(vi.mocked(signedRequest).mock.calls.length).toBe(grantsBefore);
    const backgroundSelect = calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
    expect(backgroundSelect).toHaveLength(1);
    expect(backgroundSelect[0].body).toMatchObject({ challenge_id: primaryChallenge, room_id: "second" });
    expect(remoteSessionTransport("background-session-second")).toBe(transport);
    expect(remoteWorkspaceFor(entry.server_id)).toBeNull();

    // The foreground select for another joined room reuses it too.
    await admission.select("general", new AbortController().signal).catch(() => undefined);
    const selects = calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
    expect(selects[selects.length - 1].body).toMatchObject({ challenge_id: primaryChallenge, room_id: "general" });
    admission.close();
    vi.restoreAllMocks();
  });

  it("refuses a room the host no longer lists", async () => {
    const calls: { path: string; body: unknown }[] = [];
    const { admission } = await openAdmission(calls);
    await admission.admit(new AbortController().signal);
    await expect(admission.selectBackgroundRoom("gone", new AbortController().signal)).rejects.toThrow(
      "참가 중인 방을 확인하지 못했어요."
    );
    admission.close();
    vi.restoreAllMocks();
  });

  it("keeps the old strict rooms shape working against a host without the opt-in", async () => {
    const { assertExactKeys, strictRecord } = await import("../strictJsonContract");
    // An old client checks exactly ["rooms"]; a new host that did not see the
    // opt-in must serve exactly that shape.
    const legacy = strictRecord({ rooms: [{ room_id: "general", name: "일반" }] }, "참가 중인 방");
    assertExactKeys(legacy, ["rooms"], "참가 중인 방");
    // A new host shape without the opt-in stays identical, so the check passes.
    expect(Object.keys(legacy).sort()).toEqual(["rooms"]);
  });

  it("selects past the 300s window on the live channel without a new grant when the host vouches channel-bound", async () => {
    vi.useFakeTimers();
    try {
      const calls: { path: string; body: unknown }[] = [];
      const { admission } = await openAdmission(calls, { channelBound: true });
      await admission.admit(new AbortController().signal);
      const redeemPaths = calls
        .filter((call) => call.path.startsWith("/api/member-connect/rooms"))
        .map((call) => call.path);
      expect(redeemPaths).toHaveLength(1);
      expect(redeemPaths[0]).toContain("room_select_reuse=1");
      expect(redeemPaths[0]).toContain("room_select_channel_bound=1");
      const grantsBefore = vi.mocked(signedRequest).mock.calls.length;
      const first = await admission.selectBackgroundRoom("general", new AbortController().signal);
      expect(first.meeting_id).toBe("general");
      expect(vi.mocked(signedRequest).mock.calls.length).toBe(grantsBefore);
      // Past the 300-second challenge window the same live channel keeps
      // selecting without a fresh central grant; the select carries the
      // channel-bound opt-in so the host can tell it from an old client.
      await vi.advanceTimersByTimeAsync(301_000);
      const second = await admission.selectBackgroundRoom("second", new AbortController().signal);
      expect(second.meeting_id).toBe("second");
      expect(vi.mocked(signedRequest).mock.calls.length).toBe(grantsBefore);
      const selects = calls.filter((call) => call.path.startsWith("/api/member-connect/select"));
      expect(selects).toHaveLength(2);
      expect(selects[1].path).toContain("room_select_channel_bound=1");
      expect(calls.filter((call) => call.path === "/api/member-connect/challenge")).toHaveLength(1);
      admission.close();
    } finally {
      vi.useRealTimers();
    }
    vi.restoreAllMocks();
  });
});
