import { describe, expect, it } from "vitest";

import { commandAckResultIsValid } from "./roomSocketValidation";

const payload: Record<string, unknown> = { vote_id: "vote-1" };
const summary = {
  vote_id: "vote-1",
  question: "Ship it?",
  options: ["Yes", "No"],
  vote_duration_seconds: 60,
  vote_deadline_at: "2026-08-31T00:01:00Z",
  created_by: "Operator",
  created_at: "2026-08-31T00:00:00Z",
  tallies: { Yes: 1, No: 0 },
  own_choice: "Yes",
  total_votes: 1,
  closed: true,
  closed_at: "2026-08-31T00:00:30Z",
  close_reason: "deadline",
};

function valid(candidate: unknown, request = payload) {
  return commandAckResultIsValid(
    "room.vote.summary",
    request,
    candidate,
    "general",
    "operator-local"
  );
}

describe("room vote summary ACK contract", () => {
  it("accepts the exact canonical summary including an earlier manual close timestamp", () => {
    expect(valid(summary)).toBe(true);
  });

  it("rejects ambiguous shapes and inconsistent anonymous totals", () => {
    for (const candidate of [
      { ...summary, extra: true },
      { ...summary, vote_id: "other" },
      { ...summary, tallies: { Yes: 1 } },
      { ...summary, tallies: { Yes: 1, No: -1 } },
      { ...summary, total_votes: 2 },
      { ...summary, own_choice: "Maybe" },
      { ...summary, vote_deadline_at: "2026-08-31T00:02:00Z" },
      { ...summary, closed: false },
    ]) {
      expect(valid(candidate), JSON.stringify(candidate)).toBe(false);
    }
    expect(valid(summary, { ...payload, room_id: "general" })).toBe(false);
  });
});
