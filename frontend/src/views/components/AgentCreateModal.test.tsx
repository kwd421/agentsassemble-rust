import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import {
  cerebrasProvider,
  claudeProvider,
  codexProvider,
  codexProviderWithRelations,
  deepSeekProvider,
  lmStudioProvider,
  ollamaProvider,
  openCodeProvider,
} from "./AgentCreateModal.testProviders";
import {
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
            connection_kind: "native_cli_bridge",
            executable: "codex",
            default_model: "gpt-5.6-luna",
            interactive: true,
            startable: true,
            available: true,
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
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
    expect(screen.getByRole("listitem", { name: "Claude Code" }).getAttribute("data-active")).toBe("true");
  });

  it("keeps a provider usable while its last verified catalog is shown", async () => {
    const onCreate = vi.fn().mockResolvedValue(undefined);
    const warning = "Catalog refresh timed out. Using the last verified model list.";
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="cat-stale"
        providers={[
          {
            ...codexProvider(),
            catalog_source: "stale_cache",
            discovery_error_code: "model_discovery_timeout",
            discovery_error: warning,
          },
        ]}
        onClose={() => undefined}
        onCreate={onCreate}
      />
    );

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
    await chooseWorkspace();

    expect(screen.getByText(warning)).toBeTruthy();
    expect(primaryActionButton().hasAttribute("disabled")).toBe(false);
    await userEvent.click(primaryActionButton());
    expect(onCreate).toHaveBeenCalledWith(
      expect.objectContaining({
        providerId: "codex",
        catalogRevision: "cat-stale",
        modelId: "gpt-5.6-luna",
      })
    );
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
            id: "cursor",
            display_name: "Cursor",
            provider_kind: "cursor_live_session",
            executable: "cursor-agent",
            startable: false,
            discovery_status: "failed",
            discovery_error_code: "authentication_required",
            discovery_error: "Cursor CLI 로그인이 필요합니다.",
            credential_available: false,
            controls: [],
          },
        ]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );

    await userEvent.click(screen.getByRole("listitem", { name: "Cursor" }));
    expect(screen.getByText("Cursor CLI 로그인이 필요합니다.")).toBeTruthy();
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

    await userEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
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

    expect(screen.getByText("선택한 provider가 현재 catalog에 없습니다.")).toBeTruthy();
    expect(screen.getByRole("listitem", { name: "Codex" }).getAttribute("data-active")).toBe("false");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
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

    await userEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
    expectProviderControlValue("모델", "선택 필요");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
    expect(screen.getByText("모델의 유효한 기본값이 없어 직접 선택해야 합니다.")).toBeTruthy();
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
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

    await userEvent.click(screen.getByRole("listitem", { name: "OpenCode" }));
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

    expect(screen.queryByRole("listitem", { name: "DeepSeek" })).toBeNull();
    await userEvent.click(screen.getByRole("listitem", { name: "API" }));
    expect(screen.getByRole("list", { name: "API 프로바이더" })).toBeTruthy();
    expect(screen.queryByLabelText("API 키")).toBeNull();

    await userEvent.click(screen.getByRole("listitem", { name: "DeepSeek" }));
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

  it("projects one mixed-location provider into matching Harness and Local model lists", async () => {
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
    expect(screen.queryByRole("listitem", { name: "LM Studio" })).toBeNull();
    await userEvent.click(screen.getByRole("listitem", { name: "Ollama" }));
    expectProviderControlValue("모델", "Nemotron 3 Super");
    expect(screen.getByRole("combobox", { name: "모델" }).textContent).toContain(
      "Free tier"
    );

    await userEvent.click(screen.getByRole("listitem", { name: "Local" }));
    expect(screen.getByRole("listitem", { name: "Ollama" })).toBeTruthy();
    expect(screen.getByRole("listitem", { name: "LM Studio" })).toBeTruthy();
    await userEvent.click(screen.getByRole("listitem", { name: "Ollama" }));

    expect(screen.queryByLabelText("API 키")).toBeNull();
    const model = screen.getByRole("combobox", { name: "모델" }) as HTMLButtonElement;
    expect(model.disabled).toBe(false);
    expectProviderControlValue("모델", "선택 필요");
    expect(primaryActionButton().hasAttribute("disabled")).toBe(true);
    await chooseProviderControl("모델", "Gemma 4 12B Local");
    expect(model.disabled).toBe(true);
    expect(
      (screen.getByRole("combobox", { name: "추론 강도" }) as HTMLButtonElement).disabled
    ).toBe(true);
    expect(
      (screen.getByRole("switch", { name: "응답 속도" }) as HTMLButtonElement).disabled
    ).toBe(true);
    expectProviderControlValue("권한", "읽기 전용");
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

    await userEvent.click(screen.getByRole("listitem", { name: "API" }));
    await userEvent.click(screen.getByRole("listitem", { name: "DeepSeek" }));
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

    expect((await screen.findByLabelText("API 키") as HTMLInputElement).value).toBe("");
  });

  it("does not expose credentials for an API provider without a Rust operation", async () => {
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

    await userEvent.click(screen.getByRole("listitem", { name: "API" }));
    await userEvent.click(
      screen.getByRole("listitem", { name: "Cerebras" })
    );
    expect(screen.queryByLabelText("API 키")).toBeNull();
    expect(apiMocks.fetchProviderCredentialStatus).not.toHaveBeenCalledWith("cerebras");
    expect(apiMocks.setProviderCredential).not.toHaveBeenCalled();
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

    await userEvent.click(screen.getByRole("listitem", { name: "API" }));
    await userEvent.click(screen.getByRole("listitem", { name: "DeepSeek" }));
    const deleteButton = await screen.findByRole("button", { name: "저장 키 삭제" });
    await userEvent.click(deleteButton);

    await waitFor(() =>
      expect(screen.getByText("secure store unavailable")).toBeTruthy()
    );
    expect(screen.getByRole("dialog", { name: "에이전트 추가" })).toBeTruthy();
    expect((deleteButton as HTMLButtonElement).disabled).toBe(false);
    expect(screen.getByText(/키 설정됨/)).toBeTruthy();
  });
});
