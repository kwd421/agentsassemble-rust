import { beforeEach, describe, expect, it, vi } from "vitest";
import { RemoteTransport } from "../remote/remoteTransport";
import { closeRemoteWorkspace } from "../remote/remoteWorkspace";
import { loadMemberAnchor, saveMemberAnchor } from "./memberAnchors";
import {
  MEMBER_JOIN_HANDOFF_TTL_MS,
  clearMemberJoinHandoffs,
  noteMemberJoinConsent,
  recordMemberJoinHandoff,
  takeMemberJoinHandoff,
} from "./memberJoinHandoff";
import { SecureMemberAdmission } from "./secureMemberEntry";
import { loadCentralSession, signedRequest } from "./identity";
import { TEST_SERVER_PRODUCT_SURFACE } from "../../test/serverProductSurface";

vi.mock("./identity", () => ({ loadCentralSession: vi.fn(), signedRequest: vi.fn() }));
vi.mock("../deviceIdentity", () => ({
  getOrCreateBrowserCredential: () => "device",
  getOrCreateClientId: () => "client",
}));

const PERSON = "person";
const OTHER_PERSON = "other-person";
const SERVER = "11111111-1111-4111-8111-111111111111";
const OTHER_SERVER = "22222222-2222-4222-8222-222222222222";
const EPOCH = "epoch-2";
const KEY = "host-key-2";
const OLD_EPOCH = "epoch-1";

const identity = {
  server_id: SERVER,
  registration_epoch: EPOCH,
  host_key_fingerprint: KEY,
  endpoint_origin: "https://host.test",
  endpoint_generation: 4,
};

const target = {
  server_id: SERVER,
  registration_epoch: EPOCH,
  origin: "https://host.test",
  generation: 4,
  host_public_key_jwk: {},
  host_key_fingerprint: KEY,
};

const oldAnchor = {
  server_id: SERVER,
  registration_epoch: OLD_EPOCH,
  host_key_fingerprint: "host-key-1",
  endpoint_origin: "https://host.test",
  endpoint_generation: 3,
  rooms: [{ room_id: "room-a", name: "방" }],
  hidden: false,
  updated_at: Date.now(),
};

describe("member join handoff store", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    clearMemberJoinHandoffs(PERSON);
    clearMemberJoinHandoffs(OTHER_PERSON);
  });

  it("hands the consented admission over once, and only for the exact person, server, epoch and key", () => {
    const admission = { transport: { active: true } };
    recordMemberJoinHandoff(PERSON, identity, admission);

    // Another server, another person and another epoch never consume it.
    expect(takeMemberJoinHandoff(PERSON, OTHER_SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY })).toBeNull();
    expect(takeMemberJoinHandoff(OTHER_PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY })).toBeNull();
    expect(takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: OLD_EPOCH, host_key_fingerprint: KEY })).toBeNull();
    expect(takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: "other-key" })).toBeNull();

    // The matching incarnation takes the live admission, and the consent is
    // spent: a second take finds nothing.
    const taken = takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY });
    expect(taken?.admission).toBe(admission);
    expect(takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY })).toBeNull();
  });

  it("an expired handoff does nothing", () => {
    vi.useFakeTimers();
    try {
      recordMemberJoinHandoff(PERSON, identity, { transport: { active: true } });
      vi.advanceTimersByTime(MEMBER_JOIN_HANDOFF_TTL_MS + 1);
      expect(takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("the consent refreshes the anchor to the consented incarnation and keeps the known rooms", () => {
    saveMemberAnchor(PERSON, oldAnchor);
    noteMemberJoinConsent(PERSON, identity, { transport: { active: true } }, [{ room_id: "room-b", name: "둘째" }]);
    const anchor = loadMemberAnchor(PERSON, SERVER);
    expect(anchor?.registration_epoch).toBe(EPOCH);
    expect(anchor?.host_key_fingerprint).toBe(KEY);
    expect(anchor?.endpoint_generation).toBe(4);
    expect(anchor?.rooms).toEqual([{ room_id: "room-b", name: "둘째" }, { room_id: "room-a", name: "방" }]);
  });
});

const session = { token: "central-token", person: { person_id: PERSON, display_name: "나" } };
const hello = {
  server_id: SERVER,
  protocol: "secure_admission_v1",
  client_public_key: "client-key",
  channel_id: "channel-1",
  origin: "https://host.test",
  generation: 4,
};

const challenge = {
  challenge_id: "challenge-1",
  challenge_hash: "a".repeat(43),
  server_id: SERVER,
  registration_epoch: EPOCH,
  expires_at: Math.floor(Date.now() / 1000) + 300,
};

const grant = {
  protocol: hello.protocol,
  client_public_key: hello.client_public_key,
  channel_id: hello.channel_id,
  server_id: SERVER,
  registration_epoch: EPOCH,
  endpoint_origin: hello.origin,
  endpoint_generation: hello.generation,
  purpose: "admission",
  grant_token: `aamg1.${"g".repeat(43)}`,
  expires_at: Math.floor(Date.now() / 1000) + 300,
};

const preflight = {
  status: "existing_member",
  can_auto_join: true,
  room_id: "room-a",
  room_label: "방",
  invite_scope: "room",
  participant: { participant_id: "participant-1", display_name: "친구", avatar_image_url: "" },
  operator: false,
};

const joinFor = (roomId: string, requestId: string) => ({
  server_id: SERVER,
  authority_lineage_id: "33333333-3333-4333-8333-333333333333",
  server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  session_token: "member-session",
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

function fakeTransport() {
  const state = { active: true };
  const transport = {
    hello,
    get active() { return state.active; },
    close: vi.fn(() => { state.active = false; }),
    onClose: vi.fn(() => () => undefined),
    fetch: vi.fn(async (path: string, init?: RequestInit) => {
      const body = init?.body ? JSON.parse(String(init.body)) as Record<string, string> : {};
      if (path === "/api/room-invite/member-challenge") return new Response(JSON.stringify(challenge));
      if (path === "/api/room-invite/admission") return new Response(JSON.stringify(preflight));
      if (path === "/api/room-invite/member-join") return new Response(JSON.stringify(joinFor(body.room_id || "room-a", body.request_id || "request-1")));
      if (path === "/api/member-connect/challenge") return new Response(JSON.stringify(challenge));
      if (path.startsWith("/api/member-connect/rooms")) return new Response(JSON.stringify({ rooms: [{ room_id: "room-a", name: "방" }] }));
      if (path.startsWith("/api/member-connect/select")) return new Response(JSON.stringify(joinFor(body.room_id || "room-a", body.request_id || "request-1")));
      throw new Error(`unexpected host path ${path}`);
    }),
  };
  return { transport, state };
}

describe("the invite join path", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    vi.clearAllMocks();
    clearMemberJoinHandoffs(PERSON);
    vi.mocked(loadCentralSession).mockReturnValue(session as never);
    vi.mocked(signedRequest).mockResolvedValue(grant as never);
  });

  it("an invite join consents for the joined incarnation: the anchor follows it and the registry gets the handoff", async () => {
    const { transport } = fakeTransport();
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    const admission = await SecureMemberAdmission.open(
      { server_id: SERVER, registration_epoch: EPOCH, inviteToken: "aaj1.invite" },
      target as never,
      "central-token",
      new AbortController().signal,
    );
    await admission.admit(new AbortController().signal);

    // The anchor names the consented incarnation, so the registry no longer
    // sees an identity change against it.
    const anchor = loadMemberAnchor(PERSON, SERVER);
    expect(anchor?.registration_epoch).toBe(EPOCH);
    expect(anchor?.host_key_fingerprint).toBe(KEY);
    expect(anchor?.rooms).toEqual([{ room_id: "room-a", name: "room-a" }]);

    // The registry can take the live admission over through the handoff.
    const taken = takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY });
    expect(taken?.admission).toBe(admission);
    expect(taken?.handoff.expires_at).toBeGreaterThan(Date.now());
    vi.restoreAllMocks();
  });

  it("an ordinary room select consents to nothing", async () => {
    const { transport } = fakeTransport();
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    vi.mocked(signedRequest).mockResolvedValue({ ...grant, purpose: "connect", grant_token: `aamc1.${"g".repeat(43)}` } as never);
    const admission = await SecureMemberAdmission.open(
      { server_id: SERVER, registration_epoch: EPOCH },
      target as never,
      "central-token",
      new AbortController().signal,
    );
    await admission.admit(new AbortController().signal);
    await admission.select("room-a", new AbortController().signal);

    // A select on a cached bearer installs a workspace like any other room
    // open: it never records a consent, so it can never lift a refusal.
    expect(takeMemberJoinHandoff(PERSON, SERVER, { registration_epoch: EPOCH, host_key_fingerprint: KEY })).toBeNull();
    expect(loadMemberAnchor(PERSON, SERVER)).toBeNull();
    vi.restoreAllMocks();
  });
});
