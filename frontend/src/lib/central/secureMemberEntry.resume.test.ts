import { beforeEach, expect, it, vi } from "vitest";
import { SecureMemberAdmission } from "./secureMemberEntry";
import { memberResumeTranscript } from "./memberResume";

const mocks = vi.hoisted(() => ({
  signed: vi.fn(),
  connect: vi.fn(),
  credential: vi.fn(() => "aad1_fixture"),
  resumeKey: vi.fn(),
  loadResumeKey: vi.fn(),
}));

vi.mock("./identity", () => ({
  loadCentralSession: () => ({ token: "live", person: { person_id: "person" } }),
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

const entry = { server_id: "server", registration_epoch: "epoch" };
const target = {
  server_id: "server",
  registration_epoch: "epoch",
  origin: "https://host.test",
  generation: 3,
  host_key_fingerprint: "fingerprint",
  host_public_key_jwk: {},
};
const hello = {
  protocol: "secure_admission_v1",
  server_id: "server",
  registration_epoch: "epoch",
  origin: "https://host.test",
  generation: 3,
  purpose: "member_connect",
  client_nonce: "bm9uY2U",
  client_public_key: "cHVibGljLWtleQ",
  channel_id: "Y2hhbm5lbA",
};

function now() {
  return Math.floor(Date.now() / 1000);
}

// Every test counts the central grants it spends on its own.
beforeEach(() => {
  mocks.signed.mockClear();
});

async function fixture() {
  const pair = await crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    false,
    ["sign", "verify"],
  );
  const posted: { path: string; body: Record<string, unknown> }[] = [];
  const fetchMock = vi.fn(async (path: string, init: RequestInit) => {
    const body = JSON.parse(String(init.body)) as Record<string, unknown>;
    posted.push({ path, body });
    if (path === "/api/member-connect/challenge") {
      return Response.json({
        challenge_id: "challenge",
        challenge_hash: "h".repeat(43),
        server_id: "server",
        registration_epoch: "epoch",
        expires_at: now() + 300,
      });
    }
    if (path.startsWith("/api/member-connect/rooms")) {
      return Response.json({ rooms: [{ room_id: "general", name: "일반" }] });
    }
    if (path === "/api/member-connect/resume-key") {
      return Response.json({ issued_at: now(), expires_at: now() + 3600 });
    }
    if (path === "/api/member-connect/resume-nonce") {
      return Response.json({ nonce: "bm9uY2U", expires_at: now() + 300 });
    }
    if (path === "/api/member-connect/resume") {
      return Response.json({
        rooms: [{ room_id: "general", name: "일반" }],
        challenge_id: "resumed-challenge",
        challenge_expires_at: now() + 300,
      });
    }
    return Response.json({});
  });
  const transport = { close: vi.fn(), hello, fetch: fetchMock, active: true };
  mocks.connect.mockResolvedValue(transport);
  mocks.signed.mockResolvedValue({
    grant_token: `aamc1.${"g".repeat(43)}`,
    server_id: "server",
    registration_epoch: "epoch",
    expires_at: now() + 300,
    endpoint_origin: "https://host.test",
    endpoint_generation: 3,
    protocol: "secure_admission_v1",
    client_public_key: hello.client_public_key,
    channel_id: hello.channel_id,
    purpose: "connect",
  });
  mocks.resumeKey.mockResolvedValue({ privateKey: pair.privateKey, publicKey: "public-key" });
  mocks.loadResumeKey.mockResolvedValue({ privateKey: pair.privateKey, publicKey: "public-key" });
  const admission = await SecureMemberAdmission.open(
    entry,
    target as never,
    "live",
    new AbortController().signal,
  );
  return { admission, posted, pair, fetchMock, transport };
}

it("keeps the resume key on the host after a central admission and resumes with it", async () => {
  const { admission, posted, pair } = await fixture();
  const rooms = await admission.admit();
  expect(rooms).toEqual([{ room_id: "general", name: "일반" }]);
  const issued = posted.find((call) => call.path === "/api/member-connect/resume-key");
  expect(issued?.body).toEqual({ client_public_key: "public-key" });

  const resumed = await admission.resumeDirect();
  expect(resumed).toEqual([{ room_id: "general", name: "일반" }]);
  const attempt = posted.find((call) => call.path === "/api/member-connect/resume");
  expect(attempt?.body.purpose).toBe("resume");
  expect(attempt?.body.nonce).toBe("bm9uY2U");
  expect(attempt?.body.server_id).toBe("server");
  expect(attempt?.body.registration_epoch).toBe("epoch");
  expect(attempt?.body.origin).toBe("https://host.test");
  expect(attempt?.body.generation).toBe(3);
  expect(attempt?.body.host_key_fingerprint).toBe("fingerprint");
  expect(attempt?.body.client_public_key).toBe(hello.client_public_key);
  expect(attempt?.body.channel_id).toBe(hello.channel_id);
  // The device's own resume key travels beside the channel binding: it selects
  // the stored row the host verifies.
  expect(attempt?.body.resume_public_key).toBe("public-key");
  // The signature covers the client-built tuple and nothing else.
  const signature = String(attempt?.body.signature);
  const bytes = Uint8Array.from(atob(signature.replace(/-/g, "+").replace(/_/g, "/")), (character) =>
    character.charCodeAt(0),
  );
  expect(
    await crypto.subtle.verify(
      { name: "ECDSA", hash: "SHA-256" },
      pair.publicKey,
      bytes,
      memberResumeTranscript({
        server_id: "server",
        registration_epoch: "epoch",
        origin: "https://host.test",
        generation: 3,
        host_key_fingerprint: "fingerprint",
        browser_credential_fingerprint: String(attempt?.body.browser_credential_fingerprint),
        client_public_key: hello.client_public_key,
        channel_id: hello.channel_id,
        nonce: "bm9uY2U",
      }),
    ),
  ).toBe(true);
});

it("falls back to exactly one central grant when the host has no resume route", async () => {
  const { admission, transport } = await fixture();
  const host = transport.fetch;
  const attempts: string[] = [];
  transport.fetch = vi.fn(async (path: string, init: RequestInit) => {
    attempts.push(path);
    // An old host answers the resume route with 404.
    if (path === "/api/member-connect/resume-nonce" || path === "/api/member-connect/resume") {
      return new Response("", { status: 404 });
    }
    return host(path, init);
  });
  // The direct attempt cannot open the path on this host.
  await expect(admission.resumeDirect()).rejects.toThrow();
  expect(attempts).toContain("/api/member-connect/resume-nonce");
  // The client falls back to today's central path, and it spends exactly
  // one central grant. No resume body ever reaches this host.
  const rooms = await admission.admit();
  expect(rooms).toEqual([{ room_id: "general", name: "일반" }]);
  expect(mocks.signed).toHaveBeenCalledTimes(1);
  expect(attempts).not.toContain("/api/member-connect/resume");
});
