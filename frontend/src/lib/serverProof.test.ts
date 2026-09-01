import { describe, expect, it } from "vitest";
import { deriveConnectionNonce } from "./serverProof";

describe("subscription proof primitives", () => {
  it("binds each one-use ticket to a distinct connection nonce", async () => {
    await expect(deriveConnectionNonce("a".repeat(64))).resolves.not.toBe(
      await deriveConnectionNonce("b".repeat(64))
    );
  });
});
