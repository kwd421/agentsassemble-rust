import { expect, it } from "vitest";
import { agentSessionFixture } from "../test/agentSession";
import { agentSessionIsValid } from "./participantEventContract";

it("accepts omitted or closed OS metadata and rejects host names and arbitrary values", () => {
  const session = agentSessionFixture();
  expect(agentSessionIsValid(session)).toBe(true);
  for (const execution_os of ['macos', 'windows', 'linux', 'other']) {
    expect(agentSessionIsValid({ ...session, execution_os })).toBe(true);
  }
  for (const execution_os of ['personal-hostname', 'Mac', null, 1, { name: 'private' }]) {
    expect(agentSessionIsValid({ ...session, execution_os })).toBe(false);
  }
});
