import { describe, expect, it } from "vitest";
import { handshakeFrames } from "../test/roomSocketHarness";
import { channelMessageCursorsAreValid, snapshotValidationError } from "./roomSocketValidation";

function snapshotWith(cursors?: unknown) {
  const frames = handshakeFrames(1, 1);
  const snapshot = {
    ...JSON.parse(frames.rawSnapshot),
    provider_catalog: frames.catalog.catalog,
    delegated_action_requests: [],
    provider_requests: [],
  };
  if (cursors !== undefined) snapshot.channel_message_cursors = cursors;
  return snapshot;
}

function validationError(snapshot: unknown) {
  return snapshotValidationError(snapshot, { expectedRoomId: "general", currentLastSeq: 0 });
}

describe("snapshot channel message cursors", () => {
  it("accepts the old-host shape without cursors", () => {
    expect(validationError(snapshotWith())).toBeNull();
  });

  it("accepts valid cursors", () => {
    const cursors = [
      { channel_id: "lobby", last_seq: 1 },
      { channel_id: "c0123456789ab", last_seq: 1 },
    ];
    expect(channelMessageCursorsAreValid(cursors)).toBe(true);
    expect(validationError(snapshotWith(cursors))).toBeNull();
  });

  it("rejects malformed cursors while still rejecting unknown snapshot keys", () => {
    for (const bad of [
      [{ channel_id: "lobby", last_seq: 1, extra: 1 }],
      [{ channel_id: "nope", last_seq: 1 }],
      [{ channel_id: "C0123456789AB", last_seq: 1 }],
      [{ channel_id: "lobby", last_seq: 1 }, { channel_id: "lobby", last_seq: 2 }],
      [{ channel_id: "lobby", last_seq: -1 }],
      [{ channel_id: "lobby", last_seq: 1.5 }],
      "cursors",
    ]) {
      expect(channelMessageCursorsAreValid(bad)).toBe(false);
      expect(validationError(snapshotWith(bad))).not.toBeNull();
    }
    expect(validationError({ ...snapshotWith(), unexpected: 1 })).not.toBeNull();
  });
});
