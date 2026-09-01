import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../api";
import { DEFAULT_ROOM_APPEARANCE } from "../../lib/roomAppearance";
import MobileRoomInfoPanel from "./MobileRoomInfoPanel";

const AGENT: LiveAgent = {
  agent_id: "agent-1",
  display_name: "Agent One",
  owner_id: "operator-local",
  status: "offline",
  provider_kind: "codex",
  connection_kind: "agent_session",
  engagement_mode: "agent_session",
  meeting_id: "room-1",
  last_seen_at: "",
  last_reply_at: "",
  sandbox_enforcement: "read-only",
  capabilities: [],
};

const SESSION: RoomAgentSession = {
  room_id: "room-1",
  session_id: "session-1",
  participant_id: "agent-1",
  display_name: "Agent One",
  status: "stopped",
  runtime_status: "stopped",
  enabled: true,
  provider_kind: "codex",
  runtime_kind: "codex_app_server",
  connection_kind: "agent_session",
  persona_card_id: "",
  persona_card: null,
};

const STALE_MEMBER: RoomMember = {
  meeting_id: "room-1",
  participant_id: "agent-1",
  display_name: "Stale Participant",
  avatar_image_url: "/api/attachments/participant-avatar?view=1",
  role: "reviewer",
  participant_type: "local",
  provider_kind: "antigravity_live_session",
  connection_kind: "agent_session",
  status: "joined",
  source: "agent_session",
  created_at: "",
  updated_at: "",
};

describe("MobileRoomInfoPanel", () => {
  it("does not expose Agent Session controls without the room capability", () => {
    const view = render(
      <MobileRoomInfoPanel
        room={{ id: "room-1", label: "Room One", meetingId: "room-1", topic: "" }}
        appearance={DEFAULT_ROOM_APPEARANCE}
        channelLabel="general"
        agents={[{
          ...AGENT,
          avatar_image_url: "/api/attachments/agent-avatar?view=1",
        }]}
        members={[STALE_MEMBER]}
        displayResourceBase="http://127.0.0.1:43123"
        agentSessions={[SESSION]}
        onClose={vi.fn()}
        onAgentControl={vi.fn()}
      />
    );

    expect(
      view.container.querySelector(".dc-member-avatar-image")?.getAttribute("src")
    ).toBe("http://127.0.0.1:43123/api/attachments/agent-avatar?view=1");
    expect(screen.queryByText("Stale Participant")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Agent One/ }));

    expect(screen.queryByTitle("세션 시작")).toBeNull();
  });
});
