import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RoomAgentSession } from "../../api";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { agentSessionFixture } from "../../test/agentSession";
import { codexProvider, geminiProvider } from "./AgentCreateModal.testProviders";
import AgentSessionDetails from "./AgentSessionDetails";
import { agentSessionUpdatesFromEvents } from "../../lib/canonicalRoomProjection";
import { publicRoomEventIsValid } from "../../lib/roomSocketValidation";
import { event } from "../../test/roomSocketHarness";
import type { RoomEvent } from "../../api";

const personaApi = vi.hoisted(() => ({
  fetchPersonaAssets: vi.fn(),
  importPersonaAsset: vi.fn(),
}));

vi.mock("../../api/personas", () => ({
  fetchPersonaAssets: personaApi.fetchPersonaAssets,
  importPersonaAsset: personaApi.importPersonaAsset,
}));

afterEach(cleanup);

beforeEach(() => {
  personaApi.fetchPersonaAssets.mockReset();
  personaApi.fetchPersonaAssets.mockResolvedValue([
    {
      id: "new-guide",
      display_name: "New Guide",
      asset_kind: "card",
      source_kind: "ccv3",
      lorebook_count: 1,
      asset_count: 0,
      ignored_feature_count: 0,
      tag_count: 0,
      thumbnail_url: "",
    },
  ]);
});

describe("AgentSessionDetails diagnostics", () => {
  it("adds Korean context to retained OpenCode errors", () => {
    render(<AgentSessionDetails session={agentSessionFixture({
      provider_kind: "opencode_server", last_error_code: "provider_turn_failed",
      last_error: "The OpenCode provider request failed.", recovery_required: true,
    })} />);
    expect(screen.getByText(/OpenCode 요청을 처리하지 못했어요/)).toBeTruthy();
  });

  it.each([
    { runtime_status: "disconnected", turn_count: 0, external_owned: true, process_ownership: "external" },
    { runtime_status: "stopped", turn_count: 2, external_owned: true, process_ownership: "external" },
    { runtime_status: "error", turn_count: 0, external_owned: false, process_ownership: "external" },
    { runtime_status: "disconnected", turn_count: 0, external_owned: true, process_ownership: "server" },
  ] satisfies Partial<RoomAgentSession>[])("keeps external start and configuration with the external owner (%j)", (state) => {
    render(<AgentSessionDetails session={agentSessionFixture({ ...state, enabled: false })}
      provider={codexProvider()} onControl={vi.fn()} onConfigure={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "시작" })).toBeNull();
    expect(screen.queryByRole("button", { name: "재개" })).toBeNull();
    expect(screen.queryByText("실행 설정")).toBeNull();
    expect(screen.queryByRole("button", { name: "실행 설정 저장" })).toBeNull();
    expect(screen.getByText(/시작과 실행 설정은 연결한 앱에서/)).toBeTruthy();
    expect(personaApi.fetchPersonaAssets).not.toHaveBeenCalled();
  });

  it("keeps resident pause/resume managed-only while preserving external cleanup", async () => {
    const onControl = vi.fn().mockResolvedValue(undefined);
    const session = agentSessionFixture({ external_owned: true, process_ownership: "external", enabled: true, runtime_status: "idle" });
    const { rerender } = render(<AgentSessionDetails session={session} onControl={onControl} />);
    expect(screen.queryByRole("button", { name: "일시정지" })).toBeNull();
    const paused: RoomAgentSession = { ...session, runtime_status: "paused" };
    rerender(<AgentSessionDetails session={paused} onControl={onControl} />);
    expect(screen.queryByRole("button", { name: "재개" })).toBeNull();
    expect(onControl).not.toHaveBeenCalled();
    const recovery: RoomAgentSession = { ...session, runtime_status: "disconnected", recovery_required: true };
    rerender(<AgentSessionDetails session={recovery} onControl={onControl} />);
    await userEvent.click(screen.getByRole("button", { name: "중지" }));
    expect(onControl).toHaveBeenLastCalledWith(recovery, "stop");
  });

  it("offers recovery instead of stop for a runtime stranded by a killed server", async () => {
    const onControl = vi.fn().mockResolvedValue(undefined);
    const session = agentSessionFixture({
      runtime_status: "disconnected", enabled: false, recovery_required: true,
      last_error_code: "runtime_authority_uncertain",
      last_error: "Provider runtime authority could not be confirmed.",
    });
    render(<AgentSessionDetails session={session} onControl={onControl} />);
    expect(screen.queryByRole("button", { name: "중지" })).toBeNull();
    expect(screen.getByText(/앱이 갑자기 종료돼/)).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "복구" }));
    expect(onControl).toHaveBeenCalledWith(session, "stop");
  });

  it.each([0, 2])("requires cleanup before restarting a disconnected session (%s turns)", async (turnCount) => {
    const onControl = vi.fn().mockResolvedValue(undefined);
    const session = agentSessionFixture({
      runtime_status: "disconnected", enabled: false, recovery_required: true,
      turn_count: turnCount, last_error_code: "managed_bridge_exited",
    });
    const { rerender } = render(<AgentSessionDetails session={session} provider={codexProvider()}
      onControl={onControl} onConfigure={vi.fn()} />);
    expect(screen.queryByRole("button", { name: "시작" })).toBeNull();
    expect(screen.queryByRole("button", { name: "재개" })).toBeNull();
    fireEvent.click(screen.getByText("실행 설정"));
    expect((screen.getByRole("button", { name: "실행 설정 저장" }) as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "중지" }));
    expect(onControl).toHaveBeenCalledWith(session, "stop");
    rerender(<AgentSessionDetails session={{ ...session, runtime_status: "stopped", recovery_required: false }}
      provider={codexProvider()} onControl={onControl} />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "중지" })).toBeNull());
    expect(screen.getByRole("button", { name: turnCount ? "재개" : "시작" })).toBeTruthy();
  });

  it("shows one Korean reason and keeps raw provider text behind the disclosure", () => {
    const session: RoomAgentSession = agentSessionFixture({
      room_id: "room-1",
      session_id: "session-1",
      participant_id: "agent-1",
      display_name: "Agent One",
      status: "error",
      runtime_status: "error",
      enabled: true,
      provider_kind: "grok_acp",
      runtime_kind: "acp",
      connection_kind: "agent_session",
      turn_count: 2,
      last_seen_event_id: "event-2",
      last_error: "provider unavailable",
    });

    render(<AgentSessionDetails session={session} />);
    expect(screen.getByText("에이전트 요청을 처리하지 못했어요.")).toBeTruthy();
    // The raw provider string stays available, but only inside the disclosure.
    expect(screen.getByText("provider unavailable").closest("details")).toBeTruthy();
  });

  it("renders a timeout code as one Korean line without English", () => {
    const session: RoomAgentSession = agentSessionFixture({
      provider_kind: "opencode_server",
      runtime_status: "error",
      last_error_code: "provider_turn_timeout",
      last_error: "The OpenCode turn timed out.",
      recovery_required: true,
    });

    const { container } = render(<AgentSessionDetails session={session} />);
    expect(screen.getByText("AI 응답 시간이 초과됐어요. 다시 시도해 주세요.")).toBeTruthy();
    expect(screen.queryByText(/timed out/)).toBeNull();
    expect(container.textContent).not.toContain("The OpenCode turn timed out.");
  });

  it.each([
    ["provider_api_busy", "The AI service is temporarily overloaded (HTTP 503).", "AI 서비스가 지금 붐벼요. 잠시 후 다시 시도해 주세요."],
    ["provider_room_action_missing", "Agent Platform ended without the required room action.", "AI가 방에 답을 남기지 않고 끝냈어요. 다시 요청해 주세요."],
    ["provider_api_unavailable", "The Gemini API request did not complete. (HTTP 500)", "AI 서비스가 요청을 처리하지 못했어요. 잠시 후 다시 시도해 주세요."],
    ["provider_api_unavailable", "The Gemini API request did not complete.", "AI 서비스에 연결하지 못했어요. 인터넷 연결을 확인하고 다시 시도해 주세요."],
  ])("renders %s as one Korean line without English", (code, raw, korean) => {
    const session = agentSessionFixture({ provider_kind: "agent_platform_api", runtime_status: "error",
      last_error_code: code, last_error: raw, recovery_required: true });
    const { container } = render(<AgentSessionDetails session={session} />);
    expect(screen.getByText(korean, { exact: false })).toBeTruthy();
    expect(container.textContent).not.toContain(raw);
    expect(container.textContent).not.toContain("오류 원인");
    expect(container.textContent?.includes("(503)")).toBe(code === "provider_api_busy");
    expect(container.textContent?.includes("(500)")).toBe(raw.includes("HTTP 500"));
  });

  it.each([undefined, false, true])("requires advertised interrupt support (%s)", async (support) => {
    const onControl = vi.fn().mockResolvedValue(undefined);
    const session = agentSessionFixture({ runtime_status: "busy", enabled: true });
    const provider = support === undefined ? undefined : {
      ...codexProvider(),
      turn_interrupt: support ? "retained_runtime" as const : "unsupported" as const,
    };
    const { rerender } = render(
      <AgentSessionDetails session={session} provider={provider} onControl={onControl} />
    );
    const interrupt = screen.queryByRole("button", { name: "응답 중단" }) as HTMLButtonElement | null;
    if (support === true) {
      expect(interrupt).toBeTruthy();
      await userEvent.click(interrupt!);
      expect(onControl).toHaveBeenCalledWith(session, "interrupt");
      await waitFor(() => expect(interrupt!.disabled).toBe(false));
      rerender(<AgentSessionDetails session={{ ...session, recovery_required: true }} provider={provider} onControl={onControl} />);
      expect(screen.queryByRole("button", { name: "응답 중단" })).toBeNull();
      expect(screen.getByText("복구 필요")).toBeTruthy();
      expect(screen.queryByText("응답 중")).toBeNull();
    } else {
      expect(interrupt).toBeNull();
      expect(onControl).not.toHaveBeenCalled();
    }
  });

  it.each([undefined, false, true])("uses the external connection projection instead of the host catalog (%s)", async (support) => {
    const session = agentSessionFixture({ room_id: "general", external_owned: true,
      process_ownership: "external", runtime_status: "busy", enabled: true,
      external_retained_interrupt: support });
    const stateEvent = { ...event(1), type: "agent_session_state", participant_id: session.participant_id,
      participant_type: "agent", session_id: session.session_id, runtime_status: session.runtime_status, display_name: session.display_name,
      agent_session: session } as unknown as RoomEvent;
    expect(publicRoomEventIsValid(stateEvent, "general")).toBe(true);
    const projected = agentSessionUpdatesFromEvents([stateEvent])[0];
    const onControl = vi.fn();
    // Deliberately disagree with the external report, using the same provider kind.
    const provider = { ...codexProvider(), turn_interrupt: support === true ? "unsupported" as const : "retained_runtime" as const };
    const { rerender } = render(<AgentSessionDetails session={projected} provider={provider} onControl={onControl} />);
    const interrupt = screen.queryByRole("button", { name: "응답 중단" });
    if (support === true) {
      expect(interrupt).toBeTruthy();
      await userEvent.click(interrupt!);
      expect(onControl).toHaveBeenCalledWith(projected, "interrupt");
      rerender(<AgentSessionDetails session={{ ...projected, recovery_required: true }} provider={provider} onControl={onControl} />);
      expect(screen.queryByRole("button", { name: "응답 중단" })).toBeNull();
    } else {
      expect(interrupt).toBeNull();
      expect(onControl).not.toHaveBeenCalled();
    }
  });

  it("leaves the speed tier out of an OpenCode-harness session's settings", async () => {
    const onConfigure = vi.fn().mockResolvedValue(undefined);
    const session = agentSessionFixture({
      runtime_status: "stopped", enabled: false, provider_kind: "gemini_api",
      model: "gemini-3.1-flash-lite", service_tier: "", execution_harness: "opencode",
      permission_mode: "meeting_read_only",
    });
    const { rerender } = render(<AgentSessionDetails session={session} provider={geminiProvider()}
      onConfigure={onConfigure} />);
    fireEvent.click(screen.getByText("실행 설정"));
    expect(screen.queryByLabelText("응답 속도")).toBeNull();
    expect(screen.queryByText(/선택하세요/)).toBeNull();
    expect(screen.getByText("모델·권한을 함께 저장해요. 변경은 다음 세션 시작부터 적용돼요.")).toBeTruthy();

    rerender(<AgentSessionDetails session={{ ...session, execution_harness: "builtin" }}
      provider={geminiProvider()} onConfigure={onConfigure} />);
    expect(screen.getByLabelText("응답 속도")).toBeTruthy();
  });

  it("keeps an unsaved choice when the catalog refresh replaces the provider", async () => {
    const session = agentSessionFixture({
      runtime_status: "stopped", enabled: false, provider_kind: "gemini_api",
      model: "gemini-3.1-flash-lite", service_tier: "default", permission_mode: "meeting_read_only",
    });
    const { rerender } = render(<AgentSessionDetails session={session} provider={geminiProvider()}
      onConfigure={vi.fn()} />);
    fireEvent.click(screen.getByText("실행 설정"));
    await userEvent.click(screen.getByRole("combobox", { name: "권한" }));
    await userEvent.click(screen.getByRole("option", { name: /작업 폴더 쓰기/ }));

    rerender(<AgentSessionDetails session={session} provider={geminiProvider()} onConfigure={vi.fn()} />);
    expect(screen.getByRole("combobox", { name: "권한" }).textContent).toContain("작업 폴더 쓰기");

    rerender(<AgentSessionDetails session={{ ...session, permission_mode: "workspace_write", model: "other" }}
      provider={geminiProvider()} onConfigure={vi.fn()} />);
    rerender(<AgentSessionDetails session={session} provider={geminiProvider()} onConfigure={vi.fn()} />);
    expect(screen.getByRole("combobox", { name: "권한" }).textContent).toContain("대화 전용");
  });

  it("offers no save while the catalog reloads and nothing was changed", () => {
    const session = agentSessionFixture({
      runtime_status: "stopped", enabled: false, provider_kind: "gemini_api",
      model: "gemini-3.1-flash-lite", service_tier: "", execution_harness: "opencode",
      permission_mode: "meeting_read_only",
    });
    const save = () => (screen.getByRole("button", { name: "실행 설정 저장" }) as HTMLButtonElement).disabled;
    const { rerender } = render(<AgentSessionDetails session={session} provider={geminiProvider()} onConfigure={vi.fn()} />);
    fireEvent.click(screen.getByText("실행 설정"));
    expect(save()).toBe(true);
    rerender(<AgentSessionDetails session={session} onConfigure={vi.fn()}
      provider={{ ...geminiProvider(), controls: [], discovery_status: "loading" }} />);
    expect(save()).toBe(true);
    rerender(<AgentSessionDetails session={session} provider={geminiProvider()} onConfigure={vi.fn()} />);
    expect(save()).toBe(true);
  });

  it("replaces the applied bot card on a stopped API session", async () => {
    const onConfigure = vi.fn().mockResolvedValue(undefined);
    const provider: NativeCliProviderAvailability = {
      id: "deepseek",
      display_name: "DeepSeek",
      provider_kind: "deepseek_api",
      runtime_kind: "api",
      connection_kind: "native_cli_bridge",
      workspace_required: false,
      default_model: "deepseek-chat",
      catalog_group: "api",
      interactive: true,
      turn_interrupt: "unsupported",
      startable: true,
      available: true,
      discovery_status: "ready",
      catalog_source: "static_manifest",
      credential_available: true,
      controls: [],
    };
    const session: RoomAgentSession = agentSessionFixture({
      room_id: "room-1",
      session_id: "session-1",
      participant_id: "agent-1",
      display_name: "Guide",
      status: "available",
      runtime_status: "stopped",
      enabled: false,
      provider_kind: "deepseek_api",
      runtime_kind: "api",
      connection_kind: "agent_session",
      persona_card_id: "old-guide",
      persona_card: {
        id: "old-guide",
        display_name: "Old Guide",
        asset_kind: "card",
        source_kind: "ccv3",
        lorebook_count: 1,
        asset_count: 0,
        ignored_feature_count: 0,
        tag_count: 0,
        thumbnail_url: "",
      },
    });

    render(
      <AgentSessionDetails
        session={session}
        provider={provider}
        onConfigure={onConfigure}
      />
    );

    expect(screen.getByText("현재 적용 · Old Guide")).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: /Old Guide/ }));
    await waitFor(() => expect(screen.getByRole("radio", { name: /New Guide/ })).toBeTruthy());
    await userEvent.click(screen.getByRole("radio", { name: /New Guide/ }));
    await userEvent.click(screen.getByRole("button", { name: "적용 교체" }));

    expect(onConfigure).toHaveBeenCalledWith(session, {
      persona_card_id: "new-guide",
    });
  });
});
