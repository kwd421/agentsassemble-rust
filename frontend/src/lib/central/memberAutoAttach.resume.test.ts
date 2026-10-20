import { beforeEach, describe, expect, it, vi } from "vitest";
import { attachMemberHost } from "./memberAutoAttach";
import { loadMemberAnchor, saveMemberAnchor } from "./memberAnchors";
import { memberHostOfflineBudgetExhausted, resetMemberHostOfflineBudgetForTests } from "./memberRoomLive";
import { SecureMemberAdmission } from "./secureMemberEntry";

const mocks = vi.hoisted(() => ({
  signed: vi.fn(),
  connect: vi.fn(),
  credential: vi.fn(() => "aad1_fixture"),
  resumeKey: vi.fn(),
  loadResumeKey: vi.fn(),
}));

vi.mock("./identity", () => ({
  loadCentralSession: () => ({ token: "central-token", person: { person_id: "person" } }),
  signedRequest: mocks.signed,
}));
vi.mock("../remote/remoteTransport", () => ({ RemoteTransport: { connect: mocks.connect } }));
vi.mock("../deviceIdentity", () => ({
  getOrCreateBrowserCredential: mocks.credential,
  getOrCreateClientId: () => "client",
}));
vi.mock("./memberResumeKey", () => ({
  loadOrCreateMemberResumeKey: mocks.resumeKey,
  loadMemberResumeKey: mocks.loadResumeKey,
}));

const rooms = [{ room_id: "general", name: "일반" }];
const server = { server_id: "server", registration_epoch: "epoch" };

function anchor(overrides: Record<string, unknown> = {}) {
  return {
    server_id: "server",
    registration_epoch: "epoch",
    host_key_fingerprint: "fingerprint",
    endpoint_origin: "https://host.test",
    endpoint_generation: 3,
    rooms,
    hidden: false,
    updated_at: 1,
    ...overrides,
  };
}

function probeHit(overrides: Record<string, unknown> = {}) {
  return {
    server_id: "server",
    host_key_fingerprint: "fingerprint",
    host_public_key_jwk: {},
    ...overrides,
  };
}

/** The central grant the connect path spends on this host. */
function grant() {
  return {
    grant_token: `aamc1.${"g".repeat(43)}`,
    server_id: "server",
    registration_epoch: "epoch",
    expires_at: Math.floor(Date.now() / 1000) + 300,
    endpoint_origin: "https://host.test",
    endpoint_generation: 3,
    protocol: "secure_admission_v1",
    client_public_key: "cHVibGljLWtleQ",
    channel_id: "Y2hhbm5lbA",
    purpose: "connect",
  };
}

/**
 * The real admission against a host that answers the connect routes: the
 * direct attempt fails (this device's key no longer opens the path) and
 * the central path is the only way in.
 */
async function hostFixture() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign", "verify"],
  );
  const attempts: string[] = [];
  const transport = {
    close: vi.fn(),
    hello: {
      protocol: "secure_admission_v1",
      server_id: "server",
      registration_epoch: "epoch",
      origin: "https://host.test",
      generation: 3,
      purpose: "member_connect",
      client_nonce: "bm9uY2U",
      client_public_key: "cHVibGljLWtleQ",
      channel_id: "Y2hhbm5lbA",
    },
    fetch: vi.fn(async (path: string) => {
      attempts.push(path);
      if (path === "/api/member-connect/challenge") {
        return Response.json({
          challenge_id: "challenge",
          challenge_hash: "h".repeat(43),
          server_id: "server",
          registration_epoch: "epoch",
          expires_at: Math.floor(Date.now() / 1000) + 300,
        });
      }
      if (path.startsWith("/api/member-connect/rooms")) {
        return Response.json({ rooms });
      }
      if (path === "/api/member-connect/resume-key") {
        return Response.json({
          issued_at: Math.floor(Date.now() / 1000),
          expires_at: Math.floor(Date.now() / 1000) + 3600,
        });
      }
      // This host no longer opens the direct path for this device.
      return new Response("", { status: 404 });
    }),
    active: true,
  };
  mocks.connect.mockResolvedValue(transport);
  mocks.resumeKey.mockResolvedValue({ privateKey: pair.privateKey, publicKey: "public-key" });
  mocks.loadResumeKey.mockResolvedValue({ privateKey: pair.privateKey, publicKey: "public-key" });
  const target = {
    server_id: "server",
    registration_epoch: "epoch",
    origin: "https://host.test",
    generation: 3,
    host_key_fingerprint: "fingerprint",
    host_public_key_jwk: {},
  };
  return {
    attempts,
    preview: vi.fn().mockResolvedValue({ target }),
    open: vi.fn(async () =>
      SecureMemberAdmission.open(
        { server_id: "server", registration_epoch: "epoch" },
        target as never,
        "central-token",
        new AbortController().signal,
      ),
    ),
    probe: vi.fn().mockResolvedValue(probeHit()),
  };
}

describe("the central-free member reconnect", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    resetMemberHostOfflineBudgetForTests();
    mocks.signed.mockClear();
  });

  it("reaches the joined host with the resume key and spends no central request", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn();
    const resumeDirect = vi.fn().mockResolvedValue(rooms);
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn();
    const open = vi.fn().mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const fetcher = vi.fn();
    vi.stubGlobal("fetch", fetcher);
    const attached = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(attached.rooms).toEqual(rooms);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
    expect(admit).not.toHaveBeenCalled();
    expect(preview).not.toHaveBeenCalled();
    expect(fetcher).not.toHaveBeenCalled();
    // The daily preview budget of the host does not move either.
    expect(memberHostOfflineBudgetExhausted("person", "server")).toBe(false);
    expect(loadMemberAnchor("person", "server")).toMatchObject({
      endpoint_origin: "https://host.test",
      endpoint_generation: 3,
      rooms,
    });
  });

  it("falls back to one central grant when the stored key is expired", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn().mockResolvedValue(rooms);
    const resumeDirect = vi.fn().mockRejectedValue(new Error("이 기기는 아직 재연결 자격 증명이 없어요."));
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn();
    const open = vi.fn().mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const attached = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(attached.rooms).toEqual(rooms);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(preview).not.toHaveBeenCalled();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("spends one preview and one grant on a generation bump and none on the next blip", async () => {
    saveMemberAnchor("person", anchor());
    const close = vi.fn();
    const admit = vi.fn().mockResolvedValue(rooms);
    const resumeDirect = vi.fn().mockResolvedValue(rooms);
    const admission = { admit, resumeDirect, close, select: vi.fn(), transport: { onClose: vi.fn() } };
    const preview = vi.fn().mockResolvedValue({
      target: {
        server_id: "server",
        registration_epoch: "epoch",
        origin: "https://moved.test",
        generation: 4,
        host_key_fingerprint: "fingerprint",
        host_public_key_jwk: {},
      },
    });
    // The stored generation is stale: the handshake fails and the central
    // path learns the fresh origin.
    const open = vi.fn().mockRejectedValueOnce(new Error("서버 주소가 변경됐어요.")).mockResolvedValue(admission);
    const probe = vi.fn().mockResolvedValue(probeHit());
    const first = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(first.rooms).toEqual(rooms);
    expect(preview).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(loadMemberAnchor("person", "server")?.endpoint_origin).toBe("https://moved.test");
    // The next blip on the updated anchor resumes without any central call.
    const moved = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: preview as never, open: open as never, probe: probe as never },
    );
    expect(moved.rooms).toEqual(rooms);
    expect(preview).toHaveBeenCalledTimes(1);
    expect(admit).toHaveBeenCalledTimes(1);
    expect(resumeDirect).toHaveBeenCalledTimes(1);
  });

  it("keeps the refusal when the stored incarnation no longer matches", async () => {
    saveMemberAnchor("person", anchor());
    const preview = vi.fn();
    const open = vi.fn();
    const probe = vi.fn().mockResolvedValue(probeHit());
    await expect(
      attachMemberHost(
        { server_id: "server", registration_epoch: "other-epoch" },
        "central-token",
        "person",
        new AbortController().signal,
        { preview: preview as never, open: open as never, probe: probe as never },
      ),
    ).rejects.toThrow("참가 서버 정보가 바뀌었어요.");
    expect(preview).not.toHaveBeenCalled();
    expect(open).not.toHaveBeenCalled();
    expect(loadMemberAnchor("person", "server")).not.toBeNull();
  });

  it("spends one central grant and writes the resume key when the grant succeeded", async () => {
    saveMemberAnchor("person", anchor());
    const host = await hostFixture();
    mocks.signed.mockResolvedValue(grant());
    const attached = await attachMemberHost(
      server,
      "central-token",
      "person",
      new AbortController().signal,
      { preview: host.preview as never, open: host.open as never, probe: host.probe as never },
    );
    expect(attached.rooms).toEqual(rooms);
    // The key of the expired device is not enough: the central path spends
    // exactly one grant, and only that grant lets the host store a new key.
    expect(mocks.signed).toHaveBeenCalledTimes(1);
    expect(host.attempts).toContain("/api/member-connect/resume-nonce");
    expect(host.attempts).toContain("/api/member-connect/resume-key");
  });

  it("writes no resume key when the central grant failed", async () => {
    saveMemberAnchor("person", anchor());
    const host = await hostFixture();
    mocks.signed.mockRejectedValue(new Error("central grant refused"));
    await expect(
      attachMemberHost(
        server,
        "central-token",
        "person",
        new AbortController().signal,
        { preview: host.preview as never, open: host.open as never, probe: host.probe as never },
      ),
    ).rejects.toThrow();
    // The direct attempt was tried and failed, the central grant was
    // spent, and the failed grant wrote no key on the host.
    expect(host.attempts).toContain("/api/member-connect/resume-nonce");
    expect(mocks.signed).toHaveBeenCalled();
    expect(host.attempts).not.toContain("/api/member-connect/resume-key");
  });
});
