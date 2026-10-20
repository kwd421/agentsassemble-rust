import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { agentSessionFixture } from "../../test/agentSession";
import { participantFixture } from "../../test/participant";
import {
  cerebrasProvider,
  claudeProvider,
  codexProvider,
  codexProviderWithRelations,
  deepSeekProvider,
  llmGatewayProvider,
  lmStudioProvider,
  ollamaProvider,
  openCodeProvider,
  openRouterProvider,
  tokenRouterProvider,
  vercelProvider,
} from "./AgentCreateModal.testProviders";
import { pickAi,
  chooseProviderControl,
  chooseWorkspace,
  expectProviderControlValue,
  primaryActionButton,
  resetAgentCreateApiMocks,
} from "./AgentCreateModal.testUi";

const apiMocks = vi.hoisted(() => ({
  chooseLocalWorkspace: vi.fn(),
  deleteProviderCredential: vi.fn(),
  fetchProviderCredentialStatus: vi.fn(),
  setProviderCredential: vi.fn(),
}));

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  chooseLocalWorkspace: apiMocks.chooseLocalWorkspace,
  deleteProviderCredential: apiMocks.deleteProviderCredential,
  fetchProviderCredentialStatus: apiMocks.fetchProviderCredentialStatus,
  setProviderCredential: apiMocks.setProviderCredential,
}));

afterEach(cleanup);

beforeEach(() => {
  resetAgentCreateApiMocks(apiMocks);
});

describe("AgentCreateModal", () => {
  it("explicitly re-adds a stored session without replacing its profile or asking for a new workspace", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
      providers={[codexProvider()]} onClose={() => {}} onCreate={onCreate}
      existingSessions={[agentSessionFixture({ room_id: "room-a", session_id: "stored-codex",
        display_name: "Stored Codex", model: "stored-model" }),
        agentSessionFixture({ room_id: "room-a", session_id: "exported-codex", participant_id: "exported-codex", display_name: "Exported Codex", model: "stored-model" })]}
      participants={[participantFixture({ room_id: "room-a", participant_id: "exported-codex", status: "exported" })]} />);
    pickAi("Codex");
    await userEvent.click(screen.getByRole("combobox", { name: "기존 세션" }));
    expect(screen.queryByRole("option", { name: "Exported Codex · stored-model" })).toBeNull();
    await userEvent.click(screen.getByRole("option", { name: "Stored Codex · stored-model" }));
    expect((screen.getByPlaceholderText("방에 표시될 이름") as HTMLInputElement).disabled).toBe(true);
    await userEvent.click(screen.getByRole("switch", { name: "추가하자마자 실행" }));
    await userEvent.click(primaryActionButton());
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: "stored-codex", modelId: "stored-model", startNow: false,
    })));
    expect(apiMocks.chooseLocalWorkspace).not.toHaveBeenCalled();
  });

  it("allows listing a stored session when its provider is unavailable while keeping launch and creation disabled", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
      providers={[{ ...codexProvider(), available: false, startable: false, controls: [],
        discovery_status: "failed", discovery_error: "Provider executable unavailable" }]}
      onClose={() => {}} onCreate={onCreate}
      existingSessions={[agentSessionFixture({ room_id: "room-a", session_id: "stored-offline",
        display_name: "Offline Codex", model: "stored-model" })]} />);
    pickAi("Codex");
    expect((primaryActionButton() as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("switch", { name: "추가하자마자 실행" }) as HTMLButtonElement).disabled).toBe(true);
    await chooseProviderControl("기존 세션", "Offline Codex · stored-model");
    await userEvent.click(primaryActionButton());
    await waitFor(() => expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({
      sessionId: "stored-offline", startNow: false,
    })));
  });

  it("submits only a server-catalog model value selected from a dropdown", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-test"
        providers={[
          {
            id: "codex",
            display_name: "Codex",
            provider_kind: "codex_live_session",
            runtime_kind: "live_cli",
            catalog_group: "harness",
            workspace_required: true,
            connection_kind: "native_cli_bridge",
            default_model: "gpt-5.6-luna",
            interactive: true,
            turn_interrupt: "unsupported",
            startable: true,
            available: true,
            discovery_status: "ready",
            catalog_source: "discovered",
            credential_available: false,
            controls: [
              {
                key: "model",
                label: "모델",
                kind: "combobox",
                default_value: "gpt-5.6-luna",
                options: [
                  { value: "gpt-5.6-luna", label: "Luna" },
                  { value: "gpt-5.3-codex-spark", label: "Spark" },
                ],
              },
            ],
          },
        ]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Codex");
    const model = screen.getByRole("combobox", { name: "모델" });
    await userEvent.click(model);
    expect(screen.getByRole("option", { name: "Luna" })).toBeTruthy();
    await userEvent.click(model);

    await chooseProviderControl("모델", "Spark");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
    await chooseWorkspace();
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        catalogRevision: "cat-test",
        modelId: "gpt-5.3-codex-spark",
        workspacePath: "/tmp/agentsassemble-workspace",
      })
    );
  });

  it("submits Claude Sonnet 4.6 by its exact model id", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-claude"
        providers={[claudeProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Claude Code");
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    expect(
      screen.getByRole("menuitemradio", { name: "Claude Sonnet 4.6" })
    ).toBeTruthy();
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));

    await chooseProviderControl("모델", "Claude Sonnet 4.6");
    await chooseWorkspace();
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "claude",
        modelId: "claude-sonnet-4-6",
      })
    );
  });

  it("initializes the first selected catalog without losing a name entered during discovery", async () => {
    const onCreate = vi.fn();
    const common = { open: true, meetingId: "room-a", roomLabel: "Room A", onClose: vi.fn(), onCreate };
    const ready = codexProvider();
    const loading = { ...ready, available: false, startable: false, discovery_status: "loading", controls: [], default_model: "" };
    const { rerender } = render(<AgentCreateModal {...common} providers={[loading]} catalogRevision="cold" />);
    pickAi("Codex");
    await userEvent.clear(screen.getByRole("textbox", { name: "표시 이름" }));
    await userEvent.type(screen.getByRole("textbox", { name: "표시 이름" }), "My draft");
    rerender(<AgentCreateModal {...common} providers={[ready]} catalogRevision="first" />);
    expectProviderControlValue("모델", ready.controls.find((control) => control.key === "model")!.options[0].label);
    expect((screen.getByRole("textbox", { name: "표시 이름" }) as HTMLInputElement).value).toBe("My draft");
    expect(screen.getByRole("switch", { name: "추가하자마자 실행" }).getAttribute("aria-checked")).toBe("true");
  });

  it("preserves provider and model selection when the catalog refreshes", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-one"
        providers={[codexProvider(), claudeProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Claude Code");
    await chooseProviderControl("모델", "Claude Sonnet 4.6");

    rerender(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-two"
        providers={[codexProvider(), { ...claudeProvider(), discovery_status: "ready" }]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    expectProviderControlValue("모델", "Claude Sonnet 4.6");
    expect(screen.getByRole("region", { name: "고른 AI" }).textContent).toContain("Claude");
  });

  it("does not advertise authentication when no Rust operation owns it", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-auth-required"
        providers={[
          {
            ...codexProvider(),
            id: "grok",
            display_name: "Grok",
            provider_kind: "grok_live_session",
            startable: false,
            discovery_status: "failed",
            discovery_error_code: "authentication_required",
            discovery_error: "Grok CLI 로그인이 필요합니다.",
            credential_available: false,
            controls: [],
          },
        ]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Grok");
    expect(screen.getByText("Grok CLI 로그인이 필요합니다.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: /로그인/ })).toBeNull();
  });

  it("does not switch providers when the selected provider disappears during refresh", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-one"
        providers={[codexProvider(), claudeProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Claude Code");
    await chooseProviderControl("모델", "Claude Sonnet 4.6");

    rerender(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-two"
        providers={[codexProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    expect(screen.getByText("선택한 AI가 현재 목록에 없어요.")).toBeTruthy();
    // The vanished choice is not swapped for another one: the picker asks again.
    expect(screen.getByRole("searchbox", { name: "AI 검색" })).toBeTruthy();
    // Nothing can be added from the list step.
    expect(document.querySelector(".dc-agent-create-primary")).toBeNull();
  });

  it("invalidates a selected model removed by a catalog refresh", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-one"
        providers={[claudeProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    pickAi("Claude Code");
    await chooseProviderControl("모델", "Claude Sonnet 4.6");
    const refreshed = claudeProvider();
    refreshed.controls[0].options = refreshed.controls[0].options.filter(
      (option) => option.value !== "claude-sonnet-4-6"
    );
    rerender(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-two"
        providers={[refreshed]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    expectProviderControlValue("모델", "선택 필요");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
  });

  it("does not silently choose the first option when a catalog default is invalid", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-invalid"
        providers={[
          {
            ...codexProvider(),
            controls: [
              {
                key: "model",
                label: "모델",
                kind: "combobox",
                default_value: "missing-model",
                options: [{ value: "available-model", label: "Available" }],
              },
            ],
          },
        ]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Codex");
    expectProviderControlValue("모델", "선택 필요");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("모델의 유효한 기본값이 없어 직접 선택해야 해요.")).toBeTruthy();
  });

  it("narrows effort and service tier options for the selected model", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-related"
        providers={[codexProviderWithRelations()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Codex");
    await chooseProviderControl("모델", "Variable model");

    await userEvent.click(screen.getByRole("combobox", { name: "추론 강도" }));
    expect(screen.getByRole("option", { name: "low" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "high" })).toBeTruthy();
    await userEvent.click(screen.getByRole("combobox", { name: "추론 강도" }));
    expect(screen.queryByRole("option", { name: "Fast" })).toBeNull();
    expect(
      (screen.getByRole("switch", { name: "응답 속도" }) as HTMLButtonElement).disabled
    ).toBe(true);
    expectProviderControlValue("응답 속도", "기본");

    await chooseProviderControl("추론 강도", "high");
    expect(
      (screen.getByRole("switch", { name: "응답 속도" }) as HTMLButtonElement).disabled
    ).toBe(false);
    await chooseProviderControl("응답 속도", "Fast");
  });

  it("keeps a provider option menu open while the user scrolls its options", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-scroll"
        providers={[codexProviderWithRelations()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Codex");
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));
    const menu = screen.getByRole("listbox", { name: "모델" });

    fireEvent.scroll(menu);

    expect(screen.getByRole("option", { name: "Variable model" })).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "모델" }).getAttribute("aria-expanded")).toBe(
      "true"
    );
  });

  it("requires explicit dependent settings after a model or effort change", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-related"
        providers={[codexProviderWithRelations()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Codex");
    await chooseProviderControl("모델", "High model");
    expectProviderControlValue("추론 강도", "선택 필요");
    expectProviderControlValue("응답 속도", "기본");
    await chooseProviderControl("추론 강도", "high");

    await chooseProviderControl("모델", "Variable model");
    await chooseProviderControl("추론 강도", "high");
    await chooseProviderControl("응답 속도", "Fast");
    await chooseProviderControl("추론 강도", "low");
    expectProviderControlValue("응답 속도", "선택 필요");
    await userEvent.click(screen.getByRole("switch", { name: "응답 속도" }));
    expect(screen.queryByRole("option", { name: "Fast" })).toBeNull();
  });

  it("invalidates an effort removed by a model relation change", async () => {
    const { rerender } = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-one"
        providers={[codexProviderWithRelations()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Codex");
    const refreshed = codexProviderWithRelations();
    refreshed.controls[0].options[0].metadata = {
      reasoning_efforts: ["high"],
      service_tiers: ["priority"],
    };
    rerender(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-two"
        providers={[refreshed]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    expectProviderControlValue("추론 강도", "선택 필요");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
  });

  it("groups a long model menu by recognizable model family", async () => {
    const provider = claudeProvider();
    provider.controls[0].options.splice(1, 0, {
      value: "claude-sonnet-5",
      label: "Claude Sonnet 5",
    });
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-families"
        providers={[provider]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Claude Code");
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));

    expect(
      screen.getByRole("menuitemradio", { name: "Claude Haiku 4.5" })
    ).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: "Haiku" })).toBeNull();
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Sonnet 제공사, 2개 모델" })
    );
    expect(
      within(screen.getByRole("listbox", { name: "Sonnet 모델" })).getByRole(
        "option",
        { name: "Claude Sonnet 4.6" }
      )
    ).toBeTruthy();
  });

  it("uses catalog metadata to group models and expose pricing badges", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-opencode"
        providers={[openCodeProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("OpenCode");
    await userEvent.click(screen.getByRole("combobox", { name: "모델" }));

    await userEvent.click(
      screen.getByRole("menuitem", { name: "Zen 제공사, 2개 모델" })
    );
    const zenModels = screen.getByRole("listbox", { name: "Zen 모델" });
    const freeModel = within(zenModels).getByRole("option", {
        name: "DeepSeek V4 Flash Free",
      });
    expect(freeModel).toBeTruthy();
    expect(within(freeModel).getByText("Free")).toBeTruthy();

    await userEvent.click(screen.getByRole("button", { name: "모델 목록으로 돌아가기" }));
    await userEvent.click(
      screen.getByRole("menuitem", { name: "Go 제공사, 2개 모델" })
    );
    expect(
      within(screen.getByRole("listbox", { name: "Go 모델" })).getByRole(
        "option",
        { name: "GLM 5.2" }
      )
    ).toBeTruthy();
  });

  it("routes API providers through the API choice before creating the selected provider", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-api"
        providers={[codexProvider(), deepSeekProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    expect(within(screen.getByRole("list", { name: "AI 목록" })).getByRole("listitem", { name: "DeepSeek" })).toBeTruthy();
    expect(screen.queryByLabelText("API 키")).toBeNull();

    pickAi("DeepSeek");
    expect(screen.getByLabelText("API 키")).toBeTruthy();
    await chooseProviderControl("최대 응답 길이", "8,192 토큰");
    await chooseProviderControl("권한", "작업 폴더 쓰기");
    await chooseWorkspace();
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "deepseek",
        maxOutputTokens: 8192,
      })
    );
  });

  it("shows one mixed-group provider and changes persona visibility with its selected model", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-local"
        providers={[ollamaProvider(), lmStudioProvider()]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    expect(screen.getByRole("listitem", { name: "Ollama" })).toBeTruthy();
    expect(screen.getByRole("listitem", { name: "LM Studio" })).toBeTruthy();
    pickAi("Ollama");
    expectProviderControlValue("모델", "Nemotron 3 Super");
    expect(screen.getByRole("combobox", { name: "모델" }).textContent).toContain(
      "Free tier"
    );

    expect(screen.getByRole("region", { name: "고른 AI" }).textContent).toContain("Ollama");
    expect(screen.queryByRole("button", { name: /사용 안 함/ })).toBeNull();

    expect(screen.queryByLabelText("API 키")).toBeNull();
    const model = screen.getByRole("combobox", { name: "모델" }) as HTMLButtonElement;
    expect(model.disabled).toBe(false);
    expectProviderControlValue("모델", "구독 에이전트");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(false);
    await chooseProviderControl("모델", "Gemma 4 12B · 로컬 모델 Local");
    expect(model.disabled).toBe(false);
    expectProviderControlValue("모델", "로컬 모델");
    expect(screen.getByRole("button", { name: /사용 안 함/ })).toBeTruthy();
    expect(screen.queryByRole("combobox", { name: "추론 강도" })).toBeNull();
    expect(screen.queryByRole("switch", { name: "응답 속도" })).toBeNull();
    expectProviderControlValue("권한", "대화 전용");
    expect(screen.queryByRole("button", { name: "폴더 선택" })).toBeNull();
    await userEvent.click(primaryActionButton());

    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "ollama",
        modelId: "gemma4:12b",
      })
    );
  });

  it("does not retain an unsaved provider secret after the modal closes", async () => {
    const provider = deepSeekProvider();
    const view = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-secret"
        providers={[provider]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("DeepSeek");
    const secretInput = screen.getByLabelText("API 키") as HTMLInputElement;
    await userEvent.type(secretInput, "sk-not-saved");
    expect(secretInput.value).toBe("sk-not-saved");

    view.rerender(
      <AgentCreateModal
        open={false}
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-secret"
        providers={[provider]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );
    await waitFor(() =>
      expect(screen.queryByRole("dialog", { name: "에이전트 추가" })).toBeNull()
    );
    view.rerender(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-secret"
        providers={[provider]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    // Reopening starts from the AI list; the same AI comes back without its unsaved key.
    pickAi("DeepSeek");
    expect((await screen.findByLabelText("API 키") as HTMLInputElement).value).toBe("");
  });

  it("uses the implemented credential operation advertised by an API provider", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-cerebras"
        providers={[deepSeekProvider(), cerebrasProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Cerebras");
    expect(await screen.findByLabelText("API 키")).toBeTruthy();
    expect(apiMocks.fetchProviderCredentialStatus).toHaveBeenCalledWith("cerebras");
    expect(apiMocks.setProviderCredential).not.toHaveBeenCalled();
  });

  it("uses OpenRouter's implemented credential operation", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-openrouter"
        providers={[openRouterProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("OpenRouter");
    expect(await screen.findByLabelText("API 키")).toBeTruthy();
    expect(apiMocks.fetchProviderCredentialStatus).toHaveBeenCalledWith("openrouter");
  });

  it("uses Vercel AI Gateway's implemented credential operation", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-vercel"
        providers={[vercelProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("Vercel AI Gateway");
    expect(await screen.findByLabelText("API 키")).toBeTruthy();
    expect(apiMocks.fetchProviderCredentialStatus).toHaveBeenCalledWith("vercel");
  });

  it("uses LLM Gateway's implemented credential operation", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-llm-gateway"
        providers={[llmGatewayProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("LLM Gateway");
    expect(await screen.findByLabelText("API 키")).toBeTruthy();
    expect(apiMocks.fetchProviderCredentialStatus).toHaveBeenCalledWith("llmgateway");
  });

  it("uses TokenRouter's implemented credential operation", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-tokenrouter"
        providers={[tokenRouterProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("TokenRouter");
    expect(await screen.findByLabelText("API 키")).toBeTruthy();
    expect(apiMocks.fetchProviderCredentialStatus).toHaveBeenCalledWith("tokenrouter");
  });

  it("keeps credential deletion retryable when the secure store rejects it", async () => {
    apiMocks.fetchProviderCredentialStatus.mockResolvedValue({
      configured: true,
      source: "keyring",
    });
    apiMocks.deleteProviderCredential.mockRejectedValue(
      new Error("secure store unavailable")
    );
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-secret"
        providers={[deepSeekProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    pickAi("DeepSeek");
    const deleteButton = await screen.findByRole("button", { name: "키 삭제" });
    await userEvent.click(deleteButton);

    await waitFor(() =>
      expect(screen.getByText("DeepSeek 키를 삭제하지 못했어요.")).toBeTruthy()
    );
    expect(screen.getByRole("dialog", { name: "에이전트 추가" })).toBeTruthy();
    expect((deleteButton as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByPlaceholderText("저장된 키 사용 중")).toBeTruthy();
  });
});
