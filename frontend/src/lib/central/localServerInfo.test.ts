import { describe, expect, it } from "vitest";
import { hostPublishQuotaNotice } from "./centralRequestGovernor";
import { parseLocalServerInfo } from "./identity";

const base = {
  server_id: "server",
  host_public_key_jwk: {},
  host_key_fingerprint: "fingerprint",
  protocol_version: 1,
  status: "ready",
};

describe("parseLocalServerInfo", () => {
  it("accepts hosts before the quota-park field", () => {
    const parsed = parseLocalServerInfo({ ...base });
    expect(parsed.server_id).toBe("server");
    expect(parsed.central_directory).toBeUndefined();
  });

  it("reads the parked-until projection and tolerates newer keys", () => {
    const parsed = parseLocalServerInfo({
      ...base,
      central_directory: {
        enabled: true,
        registered_origin: "",
        last_success_at: 0,
        last_error: "central directory daily request allowance is exhausted",
        name_sync_error: "",
        quota_parked_until: 1_760_736_060,
        future_key: "ignored",
      },
      another_future_key: 1,
    });
    expect(parsed.central_directory?.quota_parked_until).toBe(1_760_736_060);
  });

  it("defaults a partial directory block instead of throwing", () => {
    const parsed = parseLocalServerInfo({ ...base, central_directory: {} });
    expect(parsed.central_directory?.quota_parked_until).toBe(0);
  });
});

describe("hostPublishQuotaNotice", () => {
  it("shows one Korean line with the local reset time while parked", () => {
    const now = new Date(2026, 9, 17, 12, 0).getTime();
    const parkedUntil = Math.floor(now / 1000) + 3600;
    const notice = hostPublishQuotaNotice(parkedUntil, now);
    expect(notice).toContain("오늘 주소 등록 한도를 다 써서 친구들은 ");
    expect(notice).toContain("이후에 들어올 수 있어요.");
    expect(notice).not.toMatch(/[a-zA-Z]{3,}/);
  });

  it("stays empty once the park has expired", () => {
    const now = Date.now();
    expect(hostPublishQuotaNotice(0, now)).toBeNull();
    expect(hostPublishQuotaNotice(Math.floor(now / 1000) - 1, now)).toBeNull();
  });
});
