import { describe, expect, it } from "vitest";
import { commandAckResultIsValid } from "./roomSocketValidation";

const event = (seq: number, overrides: Record<string, unknown> = {}) => ({
  v: 1,
  id: `event-${seq}`,
  room_id: "general",
  seq,
  type: "message_final",
  content: `message ${seq}`,
  ...overrides,
});

const valid = (
  beforeSeq: number,
  events: Array<Record<string, unknown>>,
  lastSeq: number,
  hasMoreBefore: boolean,
) =>
  commandAckResultIsValid(
    "room.history",
    { before_seq: beforeSeq, limit: 3 },
    {
      events,
      oldest_seq: events[0]?.seq || 0,
      last_seq: lastSeq,
      has_more_before: hasMoreBefore,
    },
    "general",
    "operator-local",
  );

describe("canonical room history ACK validation", () => {
  it("accepts exact newest, frame-fitted, older, and terminal-empty pages", () => {
    expect(valid(0, [event(1), event(2), event(3)], 3, false)).toBe(true);
    expect(valid(0, [event(2), event(3)], 3, true)).toBe(true);
    expect(valid(4, [event(2), event(3)], 8, true)).toBe(true);
    expect(valid(1, [], 8, false)).toBe(true);
  });

  it.each([
    ["more events than requested", { before_seq: 0, limit: 2 }, {
      events: [event(1), event(2), event(3)], oldest_seq: 1, last_seq: 3,
      has_more_before: false,
    }],
    ["a noncanonical limit", { before_seq: 0, limit: 201 }, {
      events: [event(1)], oldest_seq: 1, last_seq: 1, has_more_before: false,
    }],
    ["a wrong room", { before_seq: 0, limit: 3 }, {
      events: [event(1, { room_id: "other" })], oldest_seq: 1, last_seq: 1,
      has_more_before: false,
    }],
    ["a noncontiguous sequence", { before_seq: 0, limit: 3 }, {
      events: [event(1), event(3)], oldest_seq: 1, last_seq: 3,
      has_more_before: false,
    }],
    ["an event at the exclusive cursor", { before_seq: 3, limit: 3 }, {
      events: [event(3)], oldest_seq: 3, last_seq: 3, has_more_before: true,
    }],
    ["a mismatched oldest sequence", { before_seq: 0, limit: 3 }, {
      events: [event(1)], oldest_seq: 0, last_seq: 1, has_more_before: false,
    }],
    ["a page detached from the high water", { before_seq: 0, limit: 3 }, {
      events: [event(1), event(2)], oldest_seq: 1, last_seq: 3,
      has_more_before: false,
    }],
    ["an impossible has-more flag", { before_seq: 0, limit: 3 }, {
      events: [event(1)], oldest_seq: 1, last_seq: 1, has_more_before: true,
    }],
    ["an invalid public projection", { before_seq: 0, limit: 3 }, {
      events: [event(1, {
        type: "participant_updated", participant_id: "agent-one", role: "host",
      })],
      oldest_seq: 1, last_seq: 1, has_more_before: false,
    }],
    ["an empty newest page above zero high water", { before_seq: 0, limit: 3 }, {
      events: [], oldest_seq: 0, last_seq: 8, has_more_before: false,
    }],
  ])("rejects %s", (_case, payload, result) => {
    expect(commandAckResultIsValid(
      "room.history",
      payload,
      result,
      "general",
      "operator-local",
    )).toBe(false);
  });
});
