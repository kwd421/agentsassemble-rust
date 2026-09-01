import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../api";
import MemberList from "./MemberList";


const AGENT: LiveAgent = {
  agent_id: "agent-1",
  display_name: "Agent One",
  owner_id: "operator-local",
  status: "online",
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

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("MemberList component wiring", () => {
  it("opens the extracted detail modal with Agent Session controls", () => {
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[SESSION]}
        roomId="room-1"
        roomName="Room One"
        onAgentControl={vi.fn()}
      />
    );

    const agentRow = screen.getByText("Agent One");
    expect(agentRow.closest(".dc-person-member-group")?.textContent).toContain(
      "소유자 정보 없음"
    );
    fireEvent.click(agentRow);

    const dialog = screen.getByRole("dialog", { name: "Agent One" });
    expect(within(dialog).getByRole("region", { name: "Agent One 실행 및 설정" })).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "시작" })).toBeTruthy();
    expect(within(dialog).getByText("고급 진단")).toBeTruthy();
  });

  it("takes an Agent Session moderation scope from its canonical room participant", async () => {
    const onParticipantMute = vi.fn().mockResolvedValue(undefined);
    render(
      <MemberList
        agents={[{ ...AGENT, meeting_id: "" }]}
        agentSessions={[SESSION]}
        members={[
          {
            meeting_id: "room-1",
            participant_id: "agent-1",
            display_name: "Agent One",
            role: "agent",
            participant_type: "local",
            provider_kind: "codex",
            connection_kind: "agent_session",
            status: "joined",
            source: "agent_session",
            created_at: "",
            updated_at: "",
          },
        ]}
        roomId="room-1"
        roomName="Room One"
        canModerate
        onParticipantMute={onParticipantMute}
      />
    );

    fireEvent.contextMenu(screen.getByText("Agent One"));
    fireEvent.click(screen.getByRole("menuitem", { name: "뮤트" }));

    await waitFor(() => expect(onParticipantMute).toHaveBeenCalledWith("agent-1", true));
  });

  it("does not expose moderation actions when the room supplied no callable action", () => {
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[SESSION]}
        roomId="room-1"
        roomName="Room One"
        canModerate
      />
    );

    fireEvent.contextMenu(screen.getByText("Agent One"));

    expect(screen.queryByRole("menu")).toBeNull();
    expect(screen.queryByRole("menuitem", { name: "내보내기" })).toBeNull();
  });

  it("shows the active model controls in the member row", () => {
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[
          {
            ...SESSION,
            model: "gpt-5.6-sol",
            reasoning_effort: "ultra",
            service_tier: "priority",
          },
        ]}
        roomId="room-1"
        roomName="Room One"
      />
    );

    const modelLine = screen.getByLabelText(
      "gpt-5.6-sol, Fast, 추론 Ultra"
    );
    const memberRow = modelLine.closest("[role='button']");
    expect(modelLine.textContent).toContain("gpt-5.6-sol");
    expect(modelLine.textContent).toContain("Ultra");
    expect(memberRow?.getAttribute("data-ultra")).toBe("true");
  });

  it("keeps a failed canonical role change visible instead of silently diverging", async () => {
    const onRoleChange = vi.fn().mockRejectedValue(
      new Error("canonical role update rejected")
    );
    render(
      <MemberList
        agents={[AGENT]}
        roomId="room-1"
        roomName="Room One"
        canEditRoles
        onRoleChange={onRoleChange}
      />
    );

    fireEvent.change(screen.getByRole("combobox", { name: "Agent One 역할" }), {
      target: { value: "reviewer" },
    });

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain(
        "canonical role update rejected"
      )
    );
    expect(onRoleChange).toHaveBeenCalledWith("agent-1", "reviewer");
  });

  it("renders room-owned roles independently from agent identity", () => {
    const members: RoomMember[] = [
      {
        meeting_id: "room-1",
        participant_id: "operator-local",
        display_name: "Host",
        role: "director",
        participant_type: "human",
        provider_kind: "",
        connection_kind: "browser",
        status: "joined",
        source: "room",
        created_at: "",
        updated_at: "",
      },
      {
        meeting_id: "room-1",
        participant_id: "agent-1",
        display_name: "Canonical Agent",
        role: "reviewer",
        participant_type: "local",
        provider_kind: "codex",
        connection_kind: "agent_session",
        status: "joined",
        source: "agent_session",
        created_at: "",
        updated_at: "",
      },
    ];
    render(
      <MemberList
        agents={[{ ...AGENT, display_name: "Implementation Coder" }]}
        members={members}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
        canEditRoles
      />
    );

    expect(
      (screen.getByRole("combobox", { name: "Host 역할" }) as HTMLSelectElement).value
    ).toBe("director");
    expect(
      (screen.getByRole("combobox", {
        name: "Implementation Coder 역할",
      }) as HTMLSelectElement).value
    ).toBe("reviewer");
    expect(screen.getByText("Host").closest(".dc-owner-agent-list")).toBeNull();
    expect(
      screen.getByText("Implementation Coder").closest(".dc-owner-agent-list")
    ).not.toBeNull();
  });

  it("does not infer agent ownership from room-management permission", () => {
    render(
      <MemberList
        agents={[{ ...AGENT, owner_id: undefined, owner_display_name: "Remote Owner" }]}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
        canEditRoles
      />
    );

    const group = screen.getByText("Remote Owner").closest(".dc-person-member-group");
    expect(group).not.toBeNull();
    expect(within(group as HTMLElement).getByText("Agent One")).toBeTruthy();
  });

  it("does not infer participant ownership from local runtime custody", () => {
    const members: RoomMember[] = [
      {
        meeting_id: "room-1",
        participant_id: "operator-local",
        display_name: "Host",
        role: "director",
        participant_type: "human",
        provider_kind: "",
        connection_kind: "browser",
        status: "joined",
        source: "",
        created_at: "",
        updated_at: "",
      },
      {
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
      },
      {
        meeting_id: "room-1",
        participant_id: "agent-1",
        display_name: "Participant Copy",
        role: "agent",
        participant_type: "local",
        provider_kind: "codex",
        connection_kind: "agent_session",
        owner_id: "remote-owner",
        status: "joined",
        source: "agent_session",
        created_at: "",
        updated_at: "",
      },
    ];

    render(
      <MemberList
        agents={[]}
        agentSessions={[{ ...SESSION, external_owned: false }]}
        members={members}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
        onAgentControl={vi.fn()}
      />
    );

    const hostGroup = screen.getByText("Host").closest(".dc-person-member-group");
    const remoteGroup = screen.getByText("Remote Owner").closest(".dc-person-member-group");
    expect(hostGroup).not.toBeNull();
    expect(remoteGroup).not.toBeNull();
    expect(within(remoteGroup as HTMLElement).getByText("Agent One")).toBeTruthy();
    expect(within(hostGroup as HTMLElement).queryByText("Agent One")).toBeNull();
  });

  it("keeps participant kind independent when roles cross presentation defaults", () => {
    const members: RoomMember[] = [
      {
        meeting_id: "room-1",
        participant_id: "operator-local",
        display_name: "Human Reviewer",
        role: "reviewer",
        participant_type: "human",
        provider_kind: "",
        connection_kind: "browser",
        status: "joined",
        source: "",
        created_at: "",
        updated_at: "",
      },
      {
        meeting_id: "room-1",
        participant_id: "agent-1",
        display_name: "Agent Human Role",
        role: "human",
        participant_type: "local",
        provider_kind: "codex",
        connection_kind: "agent_session",
        owner_id: "operator-local",
        status: "joined",
        source: "",
        created_at: "",
        updated_at: "",
      },
    ];

    render(
      <MemberList
        agents={[AGENT]}
        members={members}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
        canEditRoles
      />
    );

    expect(
      screen.getByText("Human Reviewer").closest(".dc-owner-agent-list")
    ).toBeNull();
    expect(
      screen.getByText("Agent One").closest(".dc-owner-agent-list")
    ).not.toBeNull();
  });

  it("keeps the canonical host in the people group for an invited browser viewer", () => {
    const members: RoomMember[] = [
      {
        meeting_id: "room-1",
        participant_id: "operator-local",
        display_name: "호스트",
        role: "human",
        participant_type: "human",
        provider_kind: "",
        connection_kind: "browser",
        status: "joined",
        source: "room",
        created_at: "",
        updated_at: "",
      },
      {
        meeting_id: "room-1",
        participant_id: "guest-1",
        display_name: "Guest",
        role: "human",
        participant_type: "human",
        provider_kind: "",
        connection_kind: "browser",
        status: "joined",
        source: "invite",
        created_at: "",
        updated_at: "",
      },
    ];

    render(
      <MemberList
        agents={[AGENT]}
        members={members}
        viewerParticipantId="guest-1"
        roomId="room-1"
        roomName="Room One"
        canEditRoles={false}
      />
    );

    const hostGroup = screen.getByText("호스트").closest(".dc-person-member-group");
    const guestGroup = screen.getByText("Guest").closest(".dc-person-member-group");
    expect(hostGroup).not.toBeNull();
    expect(guestGroup).not.toBeNull();
    expect(within(hostGroup as HTMLElement).getByText("Agent One")).toBeTruthy();
    expect(within(guestGroup as HTMLElement).queryByText("Agent One")).toBeNull();
    expect(screen.queryByText("다른 사람's Agent One")).toBeNull();
  });

  it("renders Agent Session identity with the room-owned role", () => {
    const canonicalSession = {
      ...SESSION,
      display_name: "Session Makima",
      avatar_image_url: "/api/attachments/session-avatar?view=1",
      provider_kind: "antigravity_live_session",
    };
    render(
      <MemberList
        agents={[{ ...AGENT, display_name: "Live Agent Copy" }]}
        agentSessions={[canonicalSession]}
        members={[
          {
            meeting_id: "room-1",
            participant_id: "agent-1",
            display_name: "Stale Participant",
            avatar_image_url: "/api/attachments/participant-avatar?view=1",
            role: "reviewer",
            participant_type: "local",
            provider_kind: "codex",
            connection_kind: "agent_session",
            status: "joined",
            source: "agent_session",
            created_at: "",
            updated_at: "",
          },
        ]}
        displayResourceBase="http://127.0.0.1:43123"
        roomId="room-1"
        roomName="Room One"
        canEditRoles
        onAgentControl={vi.fn()}
      />
    );

    const canonicalRow = screen.getByText("Session Makima").closest("[role='button']");
    expect(canonicalRow).not.toBeNull();
    expect(
      canonicalRow?.querySelector(".dc-member-avatar-image")?.getAttribute("src")
    ).toBe("http://127.0.0.1:43123/api/attachments/session-avatar?view=1");
    expect(
      (screen.getByRole("combobox", {
        name: "Session Makima 역할",
      }) as HTMLSelectElement).value
    ).toBe("reviewer");
    expect(screen.queryByText("Stale Participant")).toBeNull();
  });

  it("keeps profile mutation unavailable until an Agent profile owner exists", () => {
    const onAgentConfigure = vi.fn().mockResolvedValue(undefined);
    const sessionWithAvatar = {
      ...SESSION,
      avatar_image_url: "/api/attachments/agent-avatar?view=1",
    };

    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[sessionWithAvatar]}
        displayResourceBase="http://127.0.0.1:43123"
        roomId="room-1"
        roomName="Room One"
        onAgentControl={vi.fn()}
        onAgentConfigure={onAgentConfigure}
      />
    );

    expect(
      screen.getByText("Agent One").closest("[role='button']")
        ?.querySelector(".dc-member-avatar-image")
        ?.getAttribute("src")
    ).toBe("http://127.0.0.1:43123/api/attachments/agent-avatar?view=1");
    fireEvent.click(screen.getByText("Agent One"));
    const dialog = screen.getByRole("dialog", { name: "Agent One" });
    expect(within(dialog).queryByRole("textbox", { name: "표시 이름" })).toBeNull();
    expect(within(dialog).queryByLabelText("에이전트 프로필 사진 선택")).toBeNull();
    expect(within(dialog).queryByRole("button", { name: /프로필 사진 편집/ })).toBeNull();
    expect(
      within(dialog).getByRole("region", { name: "Agent One 실행 및 설정" })
    ).toBeTruthy();
    expect(onAgentConfigure).not.toHaveBeenCalled();
  });
});
