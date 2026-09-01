import { describe, expect, it } from "vitest";
import type { RoomAgentSession, RoomMember } from "../api";
import { agentSessionMemberToLiveAgent } from "./appModel";

describe("agentSessionMemberToLiveAgent", () => {
  it("takes identity from the Agent Session and membership from the room participant", () => {
    const member = {
      meeting_id: "room-1",
      participant_id: "agent-1",
      display_name: "Stale Participant",
      avatar_image_url: "/participant-avatar",
      role: "reviewer",
      participant_type: "local",
      provider_kind: "stale-provider",
      connection_kind: "stale-connection",
      status: "joined",
      source: "agent_session",
      created_at: "",
      updated_at: "",
    } satisfies RoomMember;
    const session = {
      room_id: "room-1",
      session_id: "agent-1",
      participant_id: "agent-1",
      display_name: "Session Makima",
      avatar_image_url: "/session-avatar",
      status: "stopped",
      runtime_status: "stopped",
      enabled: false,
      provider_kind: "codex_live_session",
      runtime_kind: "codex_app_server",
      connection_kind: "native_cli_bridge",
      persona_card_id: "",
      persona_card: null,
    } satisfies RoomAgentSession;

    expect(agentSessionMemberToLiveAgent(member, session)).toMatchObject({
      agent_id: "agent-1",
      display_name: "Session Makima",
      avatar_image_url: "/session-avatar",
      provider_kind: "codex_live_session",
      connection_kind: "native_cli_bridge",
      session_id: "agent-1",
      meeting_id: "room-1",
      status: "joined",
    });
  });
});
