import { describe, expect, it } from "vitest";
import { agentSessionFixture } from "../test/agentSession";
import { participantFixture } from "../test/participant";
import { roomTypingIndicators, roomTypingNames } from "./roomTypingIndicators";

const member = participantFixture({
  room_id: "room-a",
  participant_id: "agent-a",
  display_name: "Agent A",
  role: "agent",
  participant_type: "agent",
  owner_id: "operator-local",
});
const session = agentSessionFixture({
  room_id: "room-a",
  session_id: "agent-a",
  participant_id: "agent-a",
  display_name: "Agent A",
  runtime_status: "busy",
  active_turn_id: "turn-a",
  provider_kind: "codex",
});
const progress = {
  participantId: "agent-a",
  displayName: "Agent A",
  message: "",
  turnId: "turn-a",
  activity: "typing" as const,
};

describe("roomTypingNames", () => {
  it("derives one typing signal from the busy Agent Session", () => {
    expect(
      roomTypingNames({ members: [member], sessions: [session], progress: null })
    ).toEqual(["Agent A"]);
  });

  it("keeps typing visible whether detailed activity is shown or hidden", () => {
    const options = { members: [member], sessions: [session], progress };

    expect(roomTypingNames(options)).toEqual(["Agent A"]);
    expect(
      roomTypingNames({
        ...options,
        progress: { ...progress, message: "검토 중" },
      })
    ).toEqual(["Agent A"]);
  });

  it("links a typing indicator to the current Agent Session turn", () => {
    expect(
      roomTypingIndicators({ members: [member], sessions: [session], progress })
    ).toEqual([
      {
        participantId: "agent-a",
        displayName: "Agent A",
        providerKind: "codex",
        turnId: "turn-a",
        activity: "typing",
        role: "agent",
      },
    ]);
  });

  it("replaces the generic typing state while the provider compacts context", () => {
    expect(
      roomTypingIndicators({
        members: [{ ...member, role: "director" }],
        sessions: [session],
        progress: { ...progress, activity: "compacting" },
      })
    ).toMatchObject([
      {
        participantId: "agent-a",
        activity: "compacting",
        role: "director",
      },
    ]);
  });

  it.each([
    { ...session, runtime_status: "stopped" as const, active_turn_id: "" },
    { ...session, recovery_required: true },
  ])("does not revive an inactive or quarantined session from stale turn progress", (inactive) => {
    expect(
      roomTypingNames({
        members: [member],
        sessions: [inactive],
        progress,
      })
    ).toEqual([]);
  });

  it("uses Agent Session identity before participant or progress labels", () => {
    expect(
      roomTypingNames({
        members: [{ ...member, display_name: "Makima" }],
        sessions: [{ ...session, display_name: "Antigravity CLI" }],
        progress: { ...progress, displayName: "Antigravity CLI" },
      })
    ).toEqual(["Antigravity CLI"]);
  });
});
