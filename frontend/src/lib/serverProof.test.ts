import { describe, expect, it } from "vitest";
import {
  deriveConnectionNonce,
  digestPermissions,
} from "./serverProof";

describe("subscription proof primitives", () => {
  it("binds each one-use ticket to a distinct connection nonce", async () => {
    await expect(deriveConnectionNonce("a".repeat(64))).resolves.not.toBe(
      await deriveConnectionNonce("b".repeat(64))
    );
  });

  it("rejects non-canonical permission maps and hashes exact booleans", async () => {
    const permissions = {
      "agent.control": true,
      "bridge.publish": false,
      "bridge.report": false,
      "message.modify": true,
      "message.send": true,
      "participant.kick": true,
      "participant.leave": true,
      "participant.mute": true,
      "provider.request.resolve": true,
      "room.delete": true,
      "room.history": true,
      "room.manage": true,
      "room.random": true,
      "room.vote.summary": true,
    };
    const digest = await digestPermissions(permissions);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
    await expect(digestPermissions({ ...permissions, unknown: false })).resolves.toBeNull();
    await expect(
      digestPermissions({ ...permissions, "message.send": false })
    ).resolves.not.toBe(digest);
  });
});
