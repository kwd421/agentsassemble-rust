import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../api";
import { DEFAULT_ROOM_APPEARANCE } from "../../lib/roomAppearance";
import { agentSessionFixture } from "../../test/agentSession";
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

const SESSION: RoomAgentSession = agentSessionFixture({
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
});

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

afterEach(cleanup);

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

  it("does not group a room participant by stale LiveAgent ownership", () => {
    const remoteOwner: RoomMember = {
      meeting_id: "room-1",
      participant_id: "remote-owner",
      display_name: "Remote Owner",
      role: "human",
      participant_type: "human",
      provider_kind: "",
      connection_kind: "browser",
      status: "joined",
      source: "",
      created_at: "",
      updated_at: "",
    };
    render(
      <MobileRoomInfoPanel
        room={{ id: "room-1", label: "Room One", meetingId: "room-1", topic: "" }}
        appearance={DEFAULT_ROOM_APPEARANCE}
        channelLabel="general"
        agents={[
          {
            ...AGENT,
            owner_id: "remote-owner",
            owner_display_name: "Remote Owner",
          },
        ]}
        members={[remoteOwner, { ...STALE_MEMBER, owner_id: "" }]}
        onClose={vi.fn()}
      />
    );

    const remoteGroup = screen.getByText("Remote Owner").closest(
      ".dc-mobile-info-member-section"
    );
    const unassignedGroup = screen.getByText("소유자 정보 없음").closest(
      ".dc-mobile-info-member-section"
    );
    expect(remoteGroup).not.toBeNull();
    expect(unassignedGroup).not.toBeNull();
    expect(remoteGroup?.textContent).not.toContain("Agent One");
    expect(unassignedGroup?.textContent).toContain("Agent One");
  });

  it("keeps participant kind independent from its mutable room role", () => {
    const remoteOwner: RoomMember = {
      meeting_id: "room-1",
      participant_id: "remote-owner",
      display_name: "Remote Owner",
      role: "human",
      participant_type: "human",
      provider_kind: "",
      connection_kind: "browser",
      status: "joined",
      source: "",
      created_at: "",
      updated_at: "",
    };
    const crossRoleAgent: RoomMember = {
      meeting_id: "room-1",
      participant_id: "agent-cross-role",
      display_name: "Cross Role Agent",
      role: "human",
      participant_type: "local",
      provider_kind: "codex",
      connection_kind: "agent_session",
      owner_id: "remote-owner",
      status: "joined",
      source: "agent_session",
      created_at: "",
      updated_at: "",
    };

    render(
      <MobileRoomInfoPanel
        room={{ id: "room-1", label: "Room One", meetingId: "room-1", topic: "" }}
        appearance={DEFAULT_ROOM_APPEARANCE}
        channelLabel="general"
        agents={[]}
        members={[remoteOwner, crossRoleAgent]}
        onClose={vi.fn()}
      />
    );

    const remoteGroup = screen.getByText("Remote Owner").closest(
      ".dc-mobile-info-member-section"
    );
    expect(remoteGroup).not.toBeNull();
    expect(remoteGroup?.textContent).toContain("Cross Role Agent");
  });
});
