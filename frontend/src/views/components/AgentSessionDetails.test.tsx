import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RoomAgentSession } from "../../api";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { agentSessionFixture } from "../../test/agentSession";
import AgentSessionDetails from "./AgentSessionDetails";

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
      status: "stopped",
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
