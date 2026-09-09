import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RoomAgentSession } from "../../api";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { agentSessionFixture } from "../../test/agentSession";
import { codexProvider } from "./AgentCreateModal.testProviders";
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
    expect(screen.queryByRole("button", { name: "런타임 설정 저장" })).toBeNull();
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
    expect((screen.getByRole("button", { name: "런타임 설정 저장" }) as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(screen.getByRole("button", { name: "중지" }));
    expect(onControl).toHaveBeenCalledWith(session, "stop");
    rerender(<AgentSessionDetails session={{ ...session, runtime_status: "stopped", recovery_required: false }}
      provider={codexProvider()} onControl={onControl} />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "중지" })).toBeNull());
    expect(screen.getByRole("button", { name: turnCount ? "재개" : "시작" })).toBeTruthy();
  });

  it("shows only diagnostics owned by the current Agent Session contract", () => {
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
    fireEvent.click(screen.getByText("고급 진단"));

    expect(screen.getByText("turns 2")).toBeTruthy();
    expect(screen.getByText("cursor event-2")).toBeTruthy();
    expect(
      screen.getByText(
        (_, element) =>
          element?.tagName === "P" &&
          element.textContent === "오류 원인 · provider unavailable"
      )
    ).toBeTruthy();
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
