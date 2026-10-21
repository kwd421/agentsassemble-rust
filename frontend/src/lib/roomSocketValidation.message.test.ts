import { expect, test } from "vitest";
import { commandAckResultIsValid } from "./roomSocketValidation";
import { event } from "../test/roomSocketHarness";

test("publication ACK binds canonical command identity while preserving vote privacy and randomness", () => {
  const ids = ["ma_" + "a".repeat(32), "ma_" + "b".repeat(32)];
  const payload = { content: "\u0085  hello\r\nworld\0  \u0085", reply_to_event_id: "reply", attachment_ids: ids };
  const delivered = { ...event(1), content: "hello\nworld", reply_to_event_id: "reply", attachments: ids.map(id => ({ id })) };
  const valid = (candidate: unknown, request: Record<string, unknown> = payload, action = "message.send") =>
    commandAckResultIsValid(action, request, { event: candidate, event_seq: 1 }, "general", "operator-local");
  expect(valid(delivered)).toBe(true);
  for (const bad of [
    { ...delivered, actor: { ...delivered.actor, participant_id: "another" } },
    { ...delivered, message_kind: "vote" },
    { ...delivered, content: "different" },
    { ...delivered, reply_to_event_id: "different" },
    { ...delivered, attachments: [...delivered.attachments].reverse() },
    { ...delivered, attachments: [{ id: ids[0] }, { id: "ma_" + "c".repeat(32) }] },
    { ...delivered, attachments: delivered.attachments.slice(0, 1) },
  ]) expect(valid(bad)).toBe(false);
  // A recorded call needs an "@ on" request; a reply to yourself comes back without one.
  const calling = { ...payload, reply_mention: true };
  expect(valid({ ...delivered, reply_mention_participant_id: "author" }, calling)).toBe(true);
  expect(valid(delivered, calling)).toBe(true);
  expect(valid({ ...delivered, reply_mention_participant_id: "author" })).toBe(false);
  expect(valid({ ...event(1), content: "🦀".repeat(12000) }, { content: "🦀".repeat(12001) })).toBe(true);
  const poll = { ...event(1), content: "", message_kind: "vote", vote_question: "Ship?", vote_options: ["Yes", "No"], attachments: delivered.attachments };
  expect(valid(poll, { kind: "vote", vote_question: "Ship?", vote_options: ["Yes", "No"], attachment_ids: ids })).toBe(true);
  for (const kind of ["vote_cast", "vote_withdraw", "vote_close"]) {
    const marker = { ...event(1), actor: { participant_id: "", participant_type: "" }, content: "", message_kind: kind, vote_id: "poll" };
    const command = { kind, vote_id: "poll", ...(kind === "vote_cast" ? { vote_choice: "2" } : {}) };
    expect(valid(marker, command)).toBe(true);
    expect(valid({ ...marker, vote_id: "other" }, command)).toBe(false);
    expect(valid({ ...marker, actor: delivered.actor }, command)).toBe(false);
  }
  expect(valid({ ...event(1), message_kind: "dice", content: "rolled 4" }, { sides: 6 }, "room.random.dice")).toBe(true);
});
