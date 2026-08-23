import { describe, expect, it } from "vitest";
import { createServerChallenge, verifyServerProof } from "./serverProof";

describe("desktop runtime server proof", () => {
  it("verifies the protocol HMAC and rejects a changed challenge", async () => {
    const key = "a".repeat(64);
    const challenge = "b".repeat(64);
    const proof = "1487f06e7937ce6a3b94b947bc3f1141e73662cb9851fc5a59381facc497782c";
    await expect(verifyServerProof(key, challenge, proof)).resolves.toBe(true);
    await expect(verifyServerProof(key, "c".repeat(64), proof)).resolves.toBe(false);
  });

  it("generates a fresh 32-byte hexadecimal challenge", () => {
    const first = createServerChallenge();
    const second = createServerChallenge();
    expect(first).toMatch(/^[0-9a-f]{64}$/);
    expect(second).toMatch(/^[0-9a-f]{64}$/);
    expect(second).not.toBe(first);
  });
});
