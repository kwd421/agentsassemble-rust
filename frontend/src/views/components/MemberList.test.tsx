import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../api";
import { agentSessionFixture } from "../../test/agentSession";
import { participantFixture } from "../../test/participant";
import MemberList from "./MemberList";
import { RoomActivity } from "../../app/roomActivity";


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

const SESSION: RoomAgentSession = agentSessionFixture({
  room_id: "room-1",
  session_id: "session-1",
  participant_id: "agent-1",
  display_name: "Agent One",
  status: "available",
  runtime_status: "stopped",
  enabled: true,
  provider_kind: "codex",
  runtime_kind: "codex_app_server",
  connection_kind: "agent_session",
});

afterEach(() => {
  cleanup();
  localStorage.clear();
});

describe("MemberList component wiring", () => {
  it("shows 응답 중 for a working external AI, whichever list it comes from", () => {
    const connector = participantFixture({ participant_id: "connector-1", display_name: "아쎄이", participant_type: "agent", role: "agent" });
    const asAgent: LiveAgent = { ...AGENT, agent_id: "connector-1", display_name: "아쎄이", provider_kind: "", connection_kind: "room_connector", engagement_mode: "" };
    for (const agents of [[], [asAgent]]) {
      render(
        <RoomActivity.Provider value={["connector-1"]}>
          <MemberList agents={agents} members={[participantFixture(), connector]} viewerParticipantId="operator-local" roomId="general" roomName="General" />
        </RoomActivity.Provider>
      );
      expect(screen.getByText("응답 중")).toBeTruthy();
      cleanup();
    }
    // Idle, it keeps the plain row it always had: no chip.
    render(<MemberList agents={[]} members={[participantFixture(), connector]} viewerParticipantId="operator-local" roomId="general" roomName="General" />);
    expect(screen.queryByText("응답 중")).toBeNull();
    expect(document.querySelector(".dc-member-status-chip")).toBeNull();
  });

  it.each([true, false])("marks the host as 방장, the viewer as 나, and ordinary members with no badge (host=%s)", (host) => {
    render(<MemberList agents={[]} members={[participantFixture(), participantFixture({ participant_id: "central-member", display_name: "Hihi" })]}
      viewerParticipantId={host ? "operator-local" : "central-member"} canEditRoles={host} roomId="general" roomName="General" />);
    expect(screen.getByText(host ? "방장" : "나")).toBeTruthy();
    expect(screen.getByText("Hihi")).toBeTruthy();
    expect(screen.queryByText("HOST")).toBeNull();
    for (const label of ["Guest", "게스트", "YOU"]) expect(screen.queryByText(label)).toBeNull();
  });

  it("opens a person's profile card from their name and closes it with Escape", () => {
    render(<MemberList agents={[]} members={[participantFixture(), participantFixture({ participant_id: "central-member", display_name: "Hihi", created_at: "2026-06-11T03:00:00Z" })]}
      viewerParticipantId="operator-local" canEditRoles roomId="general" roomName="General" />);
    fireEvent.click(screen.getByRole("button", { name: "Hihi 프로필 보기" }));
    const card = screen.getByRole("dialog", { name: "Hihi 프로필" });
    expect(within(card).getByText("참가한 날")).toBeTruthy();
    expect(within(card).getByText("2026년 6월 11일")).toBeTruthy();
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Hihi 프로필" })).toBeNull();
  });

  it("opens only moderation from the row's context menu, without an overflow button", () => {
    render(<MemberList agents={[AGENT]} agentSessions={[SESSION]} roomId="room-1" roomName="Room One" onParticipantRemove={vi.fn()} />);
    expect(screen.queryByLabelText("Agent One 관리 메뉴")).toBeNull();
    fireEvent.contextMenu(screen.getByRole("button", { name: "Agent One 프로필 보기" }), { clientX: 40, clientY: 40 });
    expect(screen.getByRole("menu")).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Agent One 추방하기" }));
    expect(screen.getByRole("button", { name: "추방하기" })).toBeTruthy();
  });

  it("closes the row's context menu on Escape while focus stays on the row", () => {
    render(<MemberList agents={[AGENT]} agentSessions={[SESSION]} roomId="room-1" roomName="Room One" onParticipantRemove={vi.fn()} />);
    const row = screen.getByRole("button", { name: "Agent One 프로필 보기" });
    row.focus();
    fireEvent.contextMenu(row, { clientX: 40, clientY: 40 });
    expect(screen.getByRole("menu")).toBeTruthy();
    fireEvent.keyDown(row, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
  });

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
  });

  it("lets the owner rename an external AI from its profile, like an app AI", async () => {
    const connector = participantFixture({ room_id: "room-1", participant_id: "connector-1", display_name: "아쎄이", participant_type: "agent", role: "agent" });
    const onAgentProfileUpdate = vi.fn().mockResolvedValue(undefined);
    render(<MemberList agents={[]} members={[participantFixture({ room_id: "room-1" }), connector]} viewerParticipantId="operator-local"
      roomId="room-1" roomName="Room One" onAgentProfileUpdate={onAgentProfileUpdate} onAgentAvatarUpdate={vi.fn()} />);
    fireEvent.click(screen.getByText("아쎄이"));
    const dialog = screen.getByRole("dialog", { name: "아쎄이" });
    expect(within(dialog).getByText("외부 AI")).toBeTruthy();
    expect(within(dialog).getByRole("button", { name: "프로필 사진 변경" })).toBeTruthy();
    const name = within(dialog).getByRole("textbox", { name: "표시 이름" });
    fireEvent.focus(name);
    fireEvent.change(name, { target: { value: "기합 아쎄이" } });
    fireEvent.blur(name);
    await waitFor(() => expect(onAgentProfileUpdate).toHaveBeenCalledWith(
      expect.objectContaining({ participant_id: "connector-1", session_id: "connector-1", room_id: "room-1" }),
      { display_name: "기합 아쎄이" }));
  });

  it("closes the detail modal on a click outside the card, not inside it", () => {
    render(<MemberList agents={[AGENT]} agentSessions={[SESSION]} roomId="room-1" roomName="Room One" onAgentControl={vi.fn()} />);
    fireEvent.click(screen.getByText("Agent One"));
    const dialog = screen.getByRole("dialog", { name: "Agent One" });
    // A modal dialog reports its own backdrop clicks as the dialog itself, so only the
    // pointer position separates them. jsdom gives the card a zero-sized rectangle.
    fireEvent.click(within(dialog).getByRole("region", { name: "Agent One 실행 및 설정" }), { clientX: 5, clientY: 5 });
    expect(screen.queryByRole("dialog", { name: "Agent One" })).toBeTruthy();
    fireEvent.click(dialog, { clientX: 0, clientY: 0 });
    expect(screen.queryByRole("dialog", { name: "Agent One" })).toBeTruthy();
    fireEvent.click(dialog, { clientX: 5, clientY: 5 });
    expect(screen.queryByRole("dialog", { name: "Agent One" })).toBeNull();
  });

  it("takes an Agent Session moderation scope from its canonical room participant", async () => {
    const onParticipantMute = vi.fn().mockResolvedValue(undefined);
    render(
      <MemberList
        agents={[{ ...AGENT, meeting_id: "" }]}
        agentSessions={[SESSION]}
        members={[
          participantFixture({
            room_id: "room-1",
            participant_id: "agent-1",
            display_name: "Agent One",
            role: "agent",
            participant_type: "agent",
            status: "joined",
            created_at: "",
            updated_at: "",
          }),
        ]}
        roomId="room-1"
        roomName="Room One"
        canModerate
        onParticipantMute={onParticipantMute}
      />
    );

    fireEvent.contextMenu(screen.getByText("Agent One"));
    fireEvent.click(screen.getByRole("menuitem", { name: "음소거" }));

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
    expect(screen.queryByRole("menuitem", { name: "추방하기" })).toBeNull();
  });

  it("pauses, resumes and stops every eligible agent of an owner from the group header", async () => {
    const second = { ...AGENT, agent_id: "agent-2", display_name: "Agent Two" };
    const idle = agentSessionFixture({
      session_id: "session-1", participant_id: "agent-1", display_name: "Agent One",
      runtime_status: "idle",
    });
    const paused = agentSessionFixture({
      session_id: "session-2", participant_id: "agent-2", display_name: "Agent Two",
      runtime_status: "paused",
    });
    const onAgentControl = vi.fn(async () => {});
    render(
      <MemberList
        agents={[AGENT, second]}
        agentSessions={[idle, paused]}
        roomId="room-1"
        roomName="Room One"
        onAgentControl={onAgentControl}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "전체 일시정지" }));
    await waitFor(() => expect(onAgentControl).toHaveBeenCalledTimes(1));
    expect(onAgentControl).toHaveBeenCalledWith(idle, "pause");
    expect(screen.queryByRole("alert")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "전체 재개" }));
    await waitFor(() => expect(onAgentControl).toHaveBeenCalledTimes(2));
    expect(onAgentControl).toHaveBeenCalledWith(paused, "resume");
    fireEvent.click(screen.getByRole("button", { name: "전체 중지" }));
    await waitFor(() => expect(onAgentControl).toHaveBeenCalledTimes(4));
    expect(onAgentControl).toHaveBeenCalledWith(idle, "stop");
    expect(onAgentControl).toHaveBeenCalledWith(paused, "stop");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("reports only bulk control failures under the group header", async () => {
    const idle = agentSessionFixture({ session_id: "session-1", participant_id: "agent-1", runtime_status: "idle" });
    const onAgentControl = vi.fn(async () => { throw new Error("제어 실패"); });
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[idle]}
        roomId="room-1"
        roomName="Room One"
        onAgentControl={onAgentControl}
      />
    );
    fireEvent.click(screen.getByRole("button", { name: "전체 일시정지" }));
    expect((await screen.findByRole("alert")).textContent).toContain("일시정지하지 못했어요.");
    expect(onAgentControl).toHaveBeenCalledTimes(1);
  });

  it("hides bulk controls the viewer may not use", () => {
    const external = agentSessionFixture({
      session_id: "session-1", participant_id: "agent-1",
      external_owned: true, process_ownership: "external", runtime_status: "idle",
    });
    const view = render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[external]}
        roomId="room-1"
        roomName="Room One"
        onAgentControl={vi.fn()}
      />
    );
    expect(screen.queryByRole("button", { name: "전체 일시정지" })).toBeNull();
    expect(screen.queryByRole("button", { name: "전체 재개" })).toBeNull();
    expect(screen.queryByRole("button", { name: "전체 중지" })).toBeNull();
    view.rerender(
      <MemberList
        agents={[AGENT]}
        agentSessions={[external]}
        roomId="room-1"
        roomName="Room One"
      />
    );
    expect(screen.queryByRole("button", { name: "전체 일시정지" })).toBeNull();
    expect(screen.queryByRole("button", { name: "전체 재개" })).toBeNull();
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
      "gpt-5.6-sol, Priority, 추론 Ultra"
    );
    const memberRow = modelLine.closest(".dc-member");
    expect(modelLine.textContent).toContain("gpt-5.6-sol");
    // Ultra's galaxy replaces the text badge; the accessible name keeps the tier.
    expect(modelLine.textContent).not.toContain("Ultra");
    expect(modelLine.querySelector(".dc-member-effort")).toBeNull();
    expect(memberRow?.getAttribute("data-ultra")).toBe("true");
  });

  it.each([
    ["priority", "Priority", "zap"],
    ["fast", "Fast", "zap"],
    ["flex", "Flex", "coins"],
  ] as const)("shows the %s speed tier with the right icon and accessible label", (tier, label, icon) => {
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[{ ...SESSION, model: "gpt-5.6-sol", service_tier: tier }]}
        roomId="room-1"
        roomName="Room One"
      />
    );
    const modelLine = screen.getByLabelText(`gpt-5.6-sol, ${label}`);
    expect(modelLine.querySelector(`[data-icon="${icon}"]`)).toBeTruthy();
    expect(modelLine.querySelector(icon === "zap" ? '[data-icon="coins"]' : '[data-icon="zap"]')).toBeNull();
  });

  it("leaves the default speed tier unmarked in the member row", () => {
    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[{ ...SESSION, model: "gpt-5.6-sol", service_tier: "default" }]}
        roomId="room-1"
        roomName="Room One"
      />
    );
    const modelLine = screen.getByLabelText("gpt-5.6-sol");
    expect(modelLine.querySelector("[data-icon]")).toBeNull();
  });

  it("draws the galaxy background only for a maximum-reasoning agent", () => {
    const { container, rerender } = render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[{ ...SESSION, reasoning_effort: "ultra" }]}
        roomId="room-1"
        roomName="Room One"
      />
    );
    expect(container.querySelector(".dc-member")?.getAttribute("data-galaxy")).toBe("true");

    rerender(
      <MemberList
        agents={[AGENT]}
        agentSessions={[{ ...SESSION, reasoning_effort: "high" }]}
        roomId="room-1"
        roomName="Room One"
      />
    );
    expect(container.querySelector(".dc-member")?.hasAttribute("data-galaxy")).toBe(false);
  });

  it.each([
    ["minimal", "minimal"], ["low", "low"], ["medium", "medium"], ["high", "high"], ["xhigh", "xhigh"],
  ])("shows %s effort as the provider names it in visible and accessible member labels", (effort, label) => {
    render(<MemberList agents={[AGENT]} agentSessions={[{ ...SESSION, model: "gpt-5.6-sol", reasoning_effort: effort }]}
      roomId="room-1" roomName="Room One" />);
    expect(screen.getByLabelText(`gpt-5.6-sol, 추론 ${label}`).textContent).toContain(label);
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

    fireEvent.contextMenu(screen.getByText("Agent One"));
    fireEvent.click(screen.getByRole("menuitemradio", { name: "리뷰어" }));

    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain(
        "역할을 변경하지 못했어요."
      )
    );
    expect(onRoleChange).toHaveBeenCalledWith("agent-1", "reviewer");
  });

  it("renders room-owned roles independently from agent identity", () => {
    const members: RoomMember[] = [
      participantFixture({
        room_id: "room-1",
        participant_id: "operator-local",
        display_name: "Host",
        role: "director",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "agent-1",
        display_name: "Canonical Agent",
        role: "reviewer",
        participant_type: "agent",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
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
      within(screen.getByText("Host").closest(".dc-member") as HTMLElement).getByText("진행")
    ).toBeTruthy();
    expect(
      within(
        screen.getByText("Implementation Coder").closest(".dc-member") as HTMLElement
      ).getByText("리뷰어")
    ).toBeTruthy();
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

  it("toggles an owner's agent group with one header control and a decorative spine", () => {
    const secondAgent: LiveAgent = { ...AGENT, agent_id: "agent-2", display_name: "Agent Two" };
    render(
      <MemberList
        agents={[AGENT, secondAgent]}
        members={[participantFixture()]}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
      />
    );

    const group = screen.getByText("Operator").closest(".dc-person-member-group") as HTMLElement;
    const toggles = within(group).getAllByRole("button", { name: /에이전트 2명/ });
    expect(toggles).toHaveLength(1);
    const toggle = toggles[0];
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(within(group).getByText("Agent Two")).toBeTruthy();

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(within(group).queryByText("Agent One")).toBeNull();
    expect(within(group).getAllByRole("button", { name: /에이전트 2명/ })).toHaveLength(1);

    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(within(group).getByText("Agent Two")).toBeTruthy();

    const spine = group.querySelector(".dc-owner-agent-spine") as HTMLElement | null;
    expect(spine).not.toBeNull();
    expect(spine?.tagName).not.toBe("BUTTON");
    expect(spine?.getAttribute("aria-hidden")).toBe("true");
    expect(spine?.getAttribute("tabindex")).toBeNull();
  });

  it("does not infer participant ownership from local runtime custody", () => {
    const members: RoomMember[] = [
      participantFixture({
        room_id: "room-1",
        participant_id: "operator-local",
        display_name: "Host",
        role: "director",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "remote-owner",
        display_name: "Remote Owner",
        role: "human",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "agent-1",
        display_name: "Participant Copy",
        role: "agent",
        participant_type: "agent",
        owner_id: "remote-owner",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
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

  it("does not fall back to LiveAgent ownership for a room participant", () => {
    const members: RoomMember[] = [
      participantFixture({
        room_id: "room-1",
        participant_id: "remote-owner",
        display_name: "Remote Owner",
        role: "human",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "agent-1",
        display_name: "Participant Copy",
        role: "agent",
        participant_type: "agent",
        owner_id: "",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
    ];

    render(
      <MemberList
        agents={[
          {
            ...AGENT,
            owner_id: "remote-owner",
            owner_display_name: "Remote Owner",
          },
        ]}
        members={members}
        viewerParticipantId="operator-local"
        roomId="room-1"
        roomName="Room One"
      />
    );

    const remoteGroup = screen.getByText("Remote Owner").closest(
      ".dc-person-member-group"
    );
    const unassignedGroup = screen.getByText("소유자 정보 없음").closest(
      ".dc-person-member-group"
    );
    expect(remoteGroup).not.toBeNull();
    expect(unassignedGroup).not.toBeNull();
    expect(within(remoteGroup as HTMLElement).queryByText("Agent One")).toBeNull();
    expect(within(unassignedGroup as HTMLElement).getByText("Agent One")).toBeTruthy();
  });

  it("keeps participant kind independent when roles cross presentation defaults", () => {
    const members: RoomMember[] = [
      participantFixture({
        room_id: "room-1",
        participant_id: "operator-local",
        display_name: "Human Reviewer",
        role: "reviewer",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "agent-1",
        display_name: "Agent Human Role",
        role: "human",
        participant_type: "agent",
        owner_id: "operator-local",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
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
      participantFixture({
        room_id: "room-1",
        participant_id: "operator-local",
        display_name: "호스트",
        role: "human",
        participant_type: "human",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
      participantFixture({
        room_id: "room-1",
        participant_id: "guest-1",
        display_name: "Guest",
        role: "human",
        participant_type: "human",
        owner_id: "guest-1",
        status: "joined",
        created_at: "",
        updated_at: "",
      }),
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
      avatar_image_url: "/api/agent-avatars/aa_0123456789abcdef0123456789abcdef",
      provider_kind: "antigravity_live_session",
    };
    render(
      <MemberList
        agents={[{ ...AGENT, display_name: "Live Agent Copy" }]}
        agentSessions={[canonicalSession]}
        members={[
          participantFixture({
            room_id: "room-1",
            participant_id: "agent-1",
            display_name: "Stale Participant",
            avatar_image_url: "/api/attachments/participant-avatar?view=1",
            role: "reviewer",
            participant_type: "agent",
            status: "joined",
            created_at: "",
            updated_at: "",
          }),
        ]}
        displayResourceBase="http://127.0.0.1:43123"
        roomId="room-1"
        roomName="Room One"
        canEditRoles
        onAgentControl={vi.fn()}
      />
    );

    const canonicalRow = screen.getByText("Session Makima").closest(".dc-member");
    expect(canonicalRow).not.toBeNull();
    expect(canonicalRow?.querySelector(".dc-member-avatar-image")?.getAttribute("src")).toBe(
      "http://127.0.0.1:43123/api/agent-avatars/aa_0123456789abcdef0123456789abcdef"
    );

    expect(within(canonicalRow as HTMLElement).getByText("리뷰어")).toBeTruthy();
    expect(screen.queryByText("Stale Participant")).toBeNull();
  });

  it("keeps profile mutation unavailable until an Agent profile owner exists", () => {
    const onAgentConfigure = vi.fn().mockResolvedValue(undefined);

    render(
      <MemberList
        agents={[AGENT]}
        agentSessions={[SESSION]}
        displayResourceBase="http://127.0.0.1:43123"
        roomId="room-1"
        roomName="Room One"
        onAgentControl={vi.fn()}
        onAgentConfigure={onAgentConfigure}
      />
    );

    expect(
      screen.getByText("Agent One").closest(".dc-member")
        ?.querySelector(".dc-member-avatar-image")
    ).toBeNull();
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

it.each([['macos', 'Mac'], ['windows', 'Windows'], ['linux', 'Linux']] as const)("shows %s only for a connected external AI", (os, label) => {
  const session = { ...SESSION, external_owned: true, process_ownership: "external", runtime_status: "idle" as const, execution_os: os };
  const view = render(<MemberList agents={[AGENT]} agentSessions={[session]} roomId="room-1" roomName="Room One" />);
  expect(screen.getByText(`${label}에서 실행 중`)).toBeTruthy();
  for (const hidden of [{ ...session, execution_os: undefined }, { ...session, execution_os: 'other' as const },
    { ...session, runtime_status: 'disconnected' as const }, { ...session, runtime_status: 'busy' as const, last_error_code: 'bridge_disconnected' },
    { ...session, last_error_code: 'room_observation_unconfirmed' }, { ...session, runtime_status: 'error' as const }]) {
    view.rerender(<MemberList agents={[AGENT]} agentSessions={[hidden]} roomId="room-1" roomName="Room One" />);
    expect(screen.queryByText(/에서 실행 중/)).toBeNull();
  }
});
