import { afterEach, expect, it, vi } from "vitest";

afterEach(() => {
  vi.resetModules();
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
  localStorage.clear();
  sessionStorage.clear();
});

it.each(["actor_quota_exhausted", "temporary_capacity_exhausted"])(
  "parks the account until 00:00 UTC on central 429 %s",
  async (code) => {
    vi.resetModules();
    vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
    const pair = await crypto.subtle.generateKey({ name: "ECDSA", namedCurve: "P-256" }, false, ["sign", "verify"]);
    const device = {
      deviceId: "fixture-device",
      privateKey: pair.privateKey,
      publicJwk: await crypto.subtle.exportKey("jwk", pair.publicKey),
    };
    vi.stubGlobal("indexedDB", {
      open: () => {
        const request = {
          result: {
            close: vi.fn(),
            transaction: () => ({
              objectStore: () => ({
                get: () => {
                  const read = { result: device, onsuccess: () => {} };
                  queueMicrotask(() => read.onsuccess());
                  return read;
                },
              }),
            }),
          },
          onsuccess: () => {},
        };
        queueMicrotask(() => request.onsuccess());
        return request;
      },
    });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        Response.json({ error: { code, message: "Daily temporary storage capacity reached. Retry after 00:00 UTC." } }, { status: 429 }),
      ),
    );
    localStorage.setItem(
      "agentsassemble.centralSession.v1",
      JSON.stringify({
        token: "fixture-session",
        expires_at: 9_999_999_999,
        device_id: device.deviceId,
        person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" },
      }),
    );
    const { signedRequest, loadCentralSession } = await import("./identity");
    const {
      acquireCentralPermit,
      centralDailyQuotaActionNotice,
      centralDailyQuotaNotice,
      centralDailyQuotaWaitMs,
      resetCentralRequestGovernorForTests,
    } = await import("./centralRequestGovernor");
    resetCentralRequestGovernorForTests();
    const session = loadCentralSession()!;
    const error = await signedRequest(session, "/v1/servers", "POST", {}).catch((e) => e);
    expect(error).toMatchObject({ status: 429, code });
    expect(centralDailyQuotaWaitMs("fixture-person")).toBeGreaterThan(0);
    // The whole account is parked: every path family is governor-blocked.
    expect(() => acquireCentralPermit("/v1/bootstrap", "fixture-person")).toThrowError();
    // User-facing lines stay as they are for both codes.
    expect(centralDailyQuotaNotice("fixture-person")).toContain("이후에 다시 연결돼요.");
    expect(centralDailyQuotaActionNotice("fixture-person")).toContain("이후에 다시 해 주세요.");
    resetCentralRequestGovernorForTests();
  },
);
