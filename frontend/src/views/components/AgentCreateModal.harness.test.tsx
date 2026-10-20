import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { agentCreationPayload } from "../../api/agentSessions";
import {
  cerebrasProvider,
  codexProvider,
  deepSeekProvider,
  geminiProvider,
  openCodeProvider,
} from "./AgentCreateModal.testProviders";
import {
  chooseProviderControl,
  pickAi,
  primaryActionButton,
} from "./AgentCreateModal.testUi";

afterEach(cleanup);

const HARNESS_HINT =
  "OpenCode가 이 API 키로 실행돼요. 파일·명령 도구를 쓸 수 있어요.";

function renderModal(providers = [deepSeekProvider(), openCodeProvider()]) {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  render(
    <AgentCreateModal
      open
      meetingId="room-a"
      roomLabel="Room A"
      catalogRevision="catalog-1"
      providers={providers}
      onClose={() => undefined}
      onCreate={onCreate}
    />
  );
  return onCreate;
}

describe("AgentCreateModal OpenCode harness", () => {
  it("lets a DeepSeek agent choose OpenCode and sends the harness", async () => {
    const onCreate = renderModal();
    pickAi("DeepSeek");

    const harness = screen.getByRole("combobox", { name: "하네스" });
    expect(harness.textContent).toContain("기본");
    expect(screen.queryByText(HARNESS_HINT)).toBeNull();

    await chooseProviderControl("하네스", "OpenCode");
    expect(screen.getByText(HARNESS_HINT)).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "하네스" }).textContent).toContain("OpenCode");

    await userEvent.click(primaryActionButton());
    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith(
        expect.objectContaining({ providerId: "deepseek", executionHarness: "opencode" })
      )
    );
    expect(agentCreationPayload(onCreate.mock.calls[0][0])).toMatchObject({
      provider_id: "deepseek",
      execution_harness: "opencode",
    });
  });

  it("offers the Gemini harness select and defaults to the built-in driver", async () => {
    const onCreate = renderModal([geminiProvider(), openCodeProvider()]);
    pickAi("Gemini");

    expect(screen.getByRole("combobox", { name: "하네스" }).textContent).toContain("기본");
    await userEvent.click(primaryActionButton());
    await waitFor(() =>
      expect(onCreate).toHaveBeenCalledWith(
        expect.objectContaining({ providerId: "gemini", executionHarness: "builtin" })
      )
    );
    expect(agentCreationPayload(onCreate.mock.calls[0][0])).toMatchObject({
      provider_id: "gemini",
      execution_harness: "builtin",
    });
  });

  it("hides the service tier OpenCode ignores and restores it for the built-in driver", async () => {
    renderModal([geminiProvider(), openCodeProvider()]);
    pickAi("Gemini");

    expect(screen.getByLabelText("응답 속도")).toBeTruthy();
    await chooseProviderControl("하네스", "OpenCode");
    expect(screen.queryByLabelText("응답 속도")).toBeNull();
    expect(screen.getByText(HARNESS_HINT)).toBeTruthy();
    await chooseProviderControl("하네스", "기본");
    expect(screen.getByLabelText("응답 속도")).toBeTruthy();
  });

  it("hides the harness select without OpenCode or for another API provider", () => {
    const { unmount } = render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        catalogRevision="catalog-1"
        providers={[deepSeekProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn()}
      />
    );
    pickAi("DeepSeek");
    expect(screen.queryByRole("combobox", { name: "하네스" })).toBeNull();
    unmount();

    renderModal([cerebrasProvider(), codexProvider(), openCodeProvider()]);
    pickAi("Cerebras");
    expect(screen.queryByRole("combobox", { name: "하네스" })).toBeNull();
  });
});
