import { expect, it } from "vitest";
import { roomMessagePayload } from "./roomMessagePayload";

it("sends reply_mention only for a reply that turns it on", () => {
  expect(roomMessagePayload({ message: "hi", replyToEventId: "e", replyMention: true }))
    .toEqual({ content: "hi", reply_to_event_id: "e", reply_mention: true });
  expect(roomMessagePayload({ message: "hi", replyToEventId: "e", replyMention: false }))
    .toEqual({ content: "hi", reply_to_event_id: "e" });
  expect(roomMessagePayload({ message: "hi", replyMention: true })).toEqual({ content: "hi" });
});
