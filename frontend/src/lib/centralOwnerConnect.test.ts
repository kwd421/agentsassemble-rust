import { beforeEach, describe, expect, it } from "vitest";

import {
  centralOwnerConnectFromUrl,
  centralOwnerServerUrl,
  consumeCentralOwnerConnectFromUrl,
  type CentralOwnerConnect,
} from "./centralOwnerConnect";

const connect: CentralOwnerConnect = {
  grantToken: `aacg1.${"a".repeat(43)}`,
  serverId: "10000000-0000-4000-8000-000000000001",
  generation: 123,
  expiresAt: Math.floor(Date.now() / 1000) + 300,
  hostPublicKeyX: "b".repeat(43),
  hostKeyFingerprint: "c".repeat(43),
};

describe("central owner navigation secret", () => {
  beforeEach(() => {
    window.history.replaceState({}, "", "/app");
  });

  it("round-trips only through an HTTPS fragment and consumes it from history", () => {
    const url = centralOwnerServerUrl("https://home.example.test", connect);
    expect(new URL(url).pathname).toBe("/pair");
    expect(new URL(url).search).toBe("");
    expect(new URL(url).hash).toMatch(/^#central-owner=/);
    expect(centralOwnerConnectFromUrl(url)).toEqual(connect);
    window.history.replaceState({}, "", `/app${new URL(url).hash}`);
    expect(consumeCentralOwnerConnectFromUrl()).toEqual(connect);
    expect(window.location.hash).toBe("");
  });

  it("rejects non-HTTPS origins and malformed or expired fragments", () => {
    expect(() => centralOwnerServerUrl("http://home.example.test", connect)).toThrow(
      "안전하지"
    );
    expect(centralOwnerConnectFromUrl("https://home.example.test/app#central-owner=%%%"))
      .toBeNull();
    const expired = { ...connect, expiresAt: 1 };
    expect(
      centralOwnerConnectFromUrl(
        centralOwnerServerUrl("https://home.example.test", expired)
      )
    ).toBeNull();
  });
});
