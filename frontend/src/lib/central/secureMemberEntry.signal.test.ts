import { beforeEach, describe, expect, it, vi } from "vitest";
import { loadCentralSession, signedRequest } from "./identity";
import { RemoteTransport } from "../remote/remoteTransport";
import { previewSecureMember, SecureMemberAdmission } from "./secureMemberEntry";

vi.mock("./identity", () => ({ loadCentralSession: vi.fn(), signedRequest: vi.fn() }));

const session = { token: "central-token" };
const entry = { server_id: "server", registration_epoch: "epoch" };
const hello = {
  protocol: "secure_admission_v1", client_public_key: "client-key", channel_id: "channel-1",
  origin: "https://host.test", generation: 3,
};
const target = {
  server_id: "server", registration_epoch: "epoch", origin: "https://host.test", generation: 3,
  host_public_key_jwk: {}, host_key_fingerprint: "fingerprint",
};
const previewValue = {
  server_id: "server", label: "Server", endpoint_origin: "https://host.test", endpoint_generation: 3,
  protocol: "secure_admission_v1", mode: "event_secure_v1", registration_epoch: "epoch",
  host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
};
const challenge = {
  challenge_id: "challenge-1", challenge_hash: "A".repeat(43),
  server_id: "server", registration_epoch: "epoch",
  expires_at: Math.floor(Date.now() / 1000) + 300,
};

function hangImpl(
  _session: unknown,
  _path: unknown,
  _method: unknown,
  _body: unknown,
  signal?: AbortSignal,
): Promise<never> {
  return new Promise((_resolve, reject) => {
    // An already-fired deadline rejects like a real fetch would.
    if (signal?.aborted) {
      reject(signal.reason);
      return;
    }
    signal?.addEventListener("abort", () => reject(signal.reason), { once: true });
  });
}

function hangOnSignal() {
  vi.mocked(signedRequest).mockImplementation(hangImpl as never);
}

describe("member select deadline on the central legs", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    vi.mocked(loadCentralSession).mockReturnValue(session as never);
  });

  it("sends the deadline signal with the central preview", async () => {
    vi.mocked(signedRequest).mockResolvedValue(previewValue as never);
    const controller = new AbortController();
    await previewSecureMember(entry, controller.signal);
    expect(vi.mocked(signedRequest).mock.calls[0][4]).toBe(controller.signal);
  });

  it("aborts hung central grants instead of stranding the select", async () => {
    const fetch = vi.fn(async (path: string) => {
      if (path === "/api/member-connect/challenge") {
        return new Response(JSON.stringify(challenge));
      }
      return new Response(JSON.stringify({ rooms: [{ room_id: "general", name: "일반" }] }));
    });
    const transport = { hello, active: true, fetch, close: vi.fn(), onClose: vi.fn() };
    vi.spyOn(RemoteTransport, "connect").mockResolvedValue(transport as never);
    hangOnSignal();
    const admission = await SecureMemberAdmission.open(entry, target as never, "central-token", new AbortController().signal);
    const controller = new AbortController();
    // The deadline fires while central grants hangs: the admit must settle
    // with the timeout instead of stranding the rail selection.
    vi.mocked(signedRequest).mockImplementationOnce(
      (session, path, method, body, signal) => {
        controller.abort(new Error("참가 서버의 응답이 없어요. 다시 시도해 주세요."));
        return hangImpl(session, path, method, body, signal) as never;
      },
    );
    await expect(admission.admit(controller.signal)).rejects.toThrow("참가 서버의 응답이 없어요. 다시 시도해 주세요.");
    expect(vi.mocked(signedRequest).mock.calls[0][4]).toBe(controller.signal);
    admission.close();
    vi.restoreAllMocks();
  });
});
