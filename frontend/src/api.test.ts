import { afterEach, describe, expect, it, vi } from "vitest";
import { getWsTicket, saveHostToken } from "./api";

describe("standalone ticket transport", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    saveHostToken("");
  });

  it("sends no secret-derived request before the server proves host authority", async () => {
    saveHostToken("private-host-token-0000000000000000");
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
      return new Response(JSON.stringify({
        challenge: "a".repeat(64),
        host_challenge_proof: "b".repeat(64),
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getWsTicket({ kind: "host", meetingId: "general" }))
      .rejects.toThrow("did not prove the server authority");
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0][0]).toBe("/api/host-challenge");
    expect(fetchMock.mock.calls[0][1]).toBeUndefined();
  });

  it("rejects ticket-endpoint replacement after an authentic challenge", async () => {
    saveHostToken("host-token-0000000000000000000000");
    const challenge = "c".repeat(64);
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      if (input === "/api/host-challenge") {
        return new Response(JSON.stringify({
          challenge,
          host_challenge_proof: "663e4232010a2500a2ee27f392029d1f8bdb8c03ab75e34bfdf39cff63d77144",
        }), { status: 200, headers: { "Content-Type": "application/json" } });
      }
      const headers = init?.headers as Record<string, string>;
      expect(headers["X-Host-Token"]).toBeUndefined();
      expect(headers["X-Host-Proof"]).toBe(
        "d09a57843280b3ff939f5aac64629f11a93b2ef4f536972bcc962974ef78152c"
      );
      return new Response(JSON.stringify({
        ticket: "a".repeat(64),
        ttl_seconds: 30,
        server_proof_key: "b".repeat(64),
        host_response_proof: "0".repeat(64),
      }), { status: 200, headers: { "Content-Type": "application/json" } });
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(getWsTicket({ kind: "host", meetingId: "general" }))
      .rejects.toThrow("did not prove the host authority");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
