import { describe, expect, it } from "vitest";
import { handshakeFrames } from "../test/roomSocketHarness";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { verifySubscriptionReceipt } from "./roomSubscriptionContract";

const expected = {
  roomId: "general", participantId: "operator-local", streams: ["room_events"],
  serverSurface: TEST_SERVER_PRODUCT_SURFACE,
};

describe("subscription response expansion", () => {
  it("ignores extra fields and the server-only digest", () => {
    const { receipt } = handshakeFrames(1, 2);
    const expanded: Record<string, unknown> = { ...receipt, extra: { future: true } };
    for (const digest of [undefined, "different", null]) {
      expanded.server_surface_digest = digest;
      if (digest === undefined) delete expanded.server_surface_digest;
      expect(verifySubscriptionReceipt(expanded, expected)).toBe(expanded);
    }
  });

  it("retains every other required field and type", () => {
    const { receipt } = handshakeFrames(1, 2);
    for (const key of Object.keys(receipt).filter((key) => key !== "server_surface_digest")) {
      const changed: Record<string, unknown> = { ...receipt };
      delete changed[key];
      expect(() => verifySubscriptionReceipt(changed, expected), `missing ${key}`).toThrow();
      changed[key] = null;
      expect(() => verifySubscriptionReceipt(changed, expected), `null ${key}`).toThrow();
    }
    expect(() => verifySubscriptionReceipt({ ...receipt, catchup_high_water: 0 }, expected)).toThrow();
    expect(() => verifySubscriptionReceipt({ ...receipt, room_id: "other" }, expected)).toThrow();
    expect(() => verifySubscriptionReceipt({ ...receipt, participant_id: "other" }, expected)).toThrow();
    expect(() => verifySubscriptionReceipt({ ...receipt, streams: undefined }, {
      ...expected, streams: [],
    })).toThrow();
  });
});
