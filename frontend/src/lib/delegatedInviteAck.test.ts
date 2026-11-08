import { expect, it } from "vitest";
import { delegatedActionAckIsValid } from "./delegatedActionProjection";

const base = {
  invite_id: "inv", invite_token: "t", join_url: "https://h-1.agentsassemble.uk/join?token=t",
  display_name: "AI", invite_scope: "read_write", max_uses: 1, expires_at: "2026-11-08T13:00:00Z",
};
const ack = (invite: Record<string, unknown>) =>
  delegatedActionAckIsValid({ confirmation_id: "c" }, { confirmation_id: "c", state: "succeeded", receipt: {}, invite });

it("accepts an AI invitation link only without a join code", () => {
  expect(ack({ ...base, join_code: "", invitee: "ai" })).toBe(true);
  expect(ack({ ...base, join_code: "code", invitee: "ai" })).toBe(false);
  expect(ack({ ...base, join_code: "", invitee: "robot" })).toBe(false);
});

it("still requires a join code for a person's invite", () => {
  expect(ack({ ...base, join_code: "code" })).toBe(true);
  expect(ack({ ...base, join_code: "" })).toBe(false);
});
