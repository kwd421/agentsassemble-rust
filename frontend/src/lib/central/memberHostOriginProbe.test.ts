import { describe, expect, it, vi } from "vitest";
import { probeMemberHostOrigin } from "./memberHostOriginProbe";

const expected = { server_id: "server", host_key_fingerprint: "fingerprint" };

function response(value: unknown, ok = true) {
  return new Response(JSON.stringify(value), { status: ok ? 200 : 404 });
}

describe("probeMemberHostOrigin", () => {
  it("returns the host identity only on a full match", async () => {
    const fetchFn = vi.fn().mockResolvedValue(response({
      server_id: "server", host_key_fingerprint: "fingerprint", host_public_key_jwk: { kty: "OKP" },
    }));
    const hit = await probeMemberHostOrigin("https://host.test", expected, fetchFn as never, 50);
    expect(fetchFn).toHaveBeenCalledTimes(1);
    expect(String(fetchFn.mock.calls[0][0])).toBe("https://host.test/api/server-info");
    expect(hit).toMatchObject({ server_id: "server", host_key_fingerprint: "fingerprint" });
  });

  it("treats a foreign host as still down", async () => {
    const fetchFn = vi.fn().mockResolvedValue(response({
      server_id: "other", host_key_fingerprint: "fingerprint", host_public_key_jwk: {},
    }));
    await expect(probeMemberHostOrigin("https://host.test", expected, fetchFn as never, 50)).resolves.toBeNull();
    const wrongKey = vi.fn().mockResolvedValue(response({
      server_id: "server", host_key_fingerprint: "other", host_public_key_jwk: {},
    }));
    await expect(probeMemberHostOrigin("https://host.test", expected, wrongKey as never, 50)).resolves.toBeNull();
  });

  it("never spends central on network or payload failures", async () => {
    const failing = vi.fn().mockRejectedValue(new Error("down"));
    await expect(probeMemberHostOrigin("https://host.test", expected, failing as never, 50)).resolves.toBeNull();
    const badStatus = vi.fn().mockResolvedValue(response({}, false));
    await expect(probeMemberHostOrigin("https://host.test", expected, badStatus as never, 50)).resolves.toBeNull();
    const badJson = vi.fn().mockResolvedValue(new Response("not json", { status: 200 }));
    await expect(probeMemberHostOrigin("https://host.test", expected, badJson as never, 50)).resolves.toBeNull();
    await expect(probeMemberHostOrigin("http://host.test", expected, failing as never, 50)).resolves.toBeNull();
    await expect(probeMemberHostOrigin("https://evil.test/extra", expected, failing as never, 50)).resolves.toBeNull();
  });
});
