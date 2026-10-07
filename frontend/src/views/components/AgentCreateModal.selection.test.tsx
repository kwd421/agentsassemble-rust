import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { workPermissionControl, codexProvider, claudeProvider, openCodeProvider, deepSeekProvider, lmStudioProvider } from "./AgentCreateModal.testProviders";

afterEach(cleanup);

describe("AgentCreateModal provider selection", () => {
  it("does not ask for a display name before a provider is selected", async () => {
    render(
      <AgentCreateModal
        open
        meetingId="room-a"
        roomLabel="Room A"
        providers={[codexProvider()]}
        onClose={() => undefined}
        onCreate={vi.fn().mockResolvedValue(undefined)}
      />
    );

    expect(screen.queryByLabelText("표시 이름")).toBeNull();
    expect(screen.getByRole("dialog").getAttribute("data-provider-selected")).toBe("false");
    const hint = screen.getByText("사용할 AI를 골라 주세요.");
    expect(hint.compareDocumentPosition(screen.getByRole("list", { name: "구독 에이전트 AI" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
    expect(screen.getByLabelText("표시 이름")).toBeTruthy();
    expect(screen.getByRole("dialog").getAttribute("data-provider-selected")).toBe("true");
  });
});

it("keeps an unavailable provider selectable for local setup without permitting creation", async () => {
  const onCreate = vi.fn();
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[{ ...codexProvider(), available: false, startable: false, discovery_status: "failed",
      discovery_error_code: "command_missing", discovery_error: "CLI가 설치되지 않았어요." }, openCodeProvider(), deepSeekProvider(), lmStudioProvider()]}
    onClose={() => undefined} onCreate={onCreate} />);
  expect(screen.queryByRole("list", { name: "에이전트 종류" })).toBeNull();
  expect(screen.getAllByRole("heading", { level: 3 }).map((item) => item.textContent))
    .toEqual(["구독 에이전트", "API 키", "로컬 모델"]);
  expect(within(screen.getByRole("list", { name: "구독 에이전트 AI" })).getAllByRole("listitem")
    .map((item) => item.getAttribute("aria-label"))).toEqual(["OpenCode", "Codex"]);
  expect(screen.getByRole("listitem", { name: "Codex" }).getAttribute("data-unavailable")).toBe("true");
  await userEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
  const setup = screen.getByRole("link", { name: "이 PC에서 Codex 설정 열기" });
  expect(setup.getAttribute("href")).toBe("agentsassemble://provider-setup/codex");
  expect((screen.getByRole("button", { name: "추가" }) as HTMLButtonElement).disabled).toBe(true);
  expect(onCreate).not.toHaveBeenCalled();
});

it("keeps initial loading providers undimmed in catalog order", () => {
  const providers = [codexProvider(), claudeProvider(), openCodeProvider()].map((provider) => ({
    ...provider, available: false, startable: false, discovery_status: "loading",
    catalog_source: "discovered", controls: [], default_model: "",
  }));
  const { rerender } = render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={providers} onClose={() => undefined} onCreate={vi.fn()} />);
  const choices = () => within(screen.getByRole("list", { name: "구독 에이전트 AI" })).getAllByRole("listitem");
  expect(choices().map((item) => item.getAttribute("aria-label"))).toEqual(["Codex", "Claude Code", "OpenCode"]);
  expect(choices().map((item) => item.getAttribute("data-unavailable"))).toEqual(["false", "false", "false"]);

  rerender(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[{ ...providers[0], discovery_status: "failed", discovery_error_code: "command_missing" },
      providers[1], openCodeProvider()]}
    onClose={() => undefined} onCreate={vi.fn()} />);
  expect(choices().map((item) => item.getAttribute("aria-label"))).toEqual(["Claude Code", "OpenCode", "Codex"]);
  expect(choices().map((item) => item.getAttribute("data-unavailable"))).toEqual(["false", "false", "true"]);
});

it("disables conversation-only permission for a free OpenCode model", async () => {
  const provider = openCodeProvider();
  const model = provider.controls[0];
  model.default_value = model.options[0].value;
  const permissions = workPermissionControl();
  permissions.options.push({ value: "full_access", label: "전체 액세스" });
  provider.controls.push(permissions);
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[provider]} onClose={() => undefined} onCreate={vi.fn()} />);
  await userEvent.click(screen.getByRole("listitem", { name: "OpenCode" }));
  expect(screen.getByText("OpenCode 무료 모델은 작업 폴더 쓰기나 전체 액세스 권한이 필요해요.")).toBeTruthy();
  const permission = screen.getByRole("combobox", { name: "권한" });
  expect(permission.textContent).toContain("작업 폴더 쓰기");
  await userEvent.click(permission);
  expect((screen.getByRole("option", { name: "대화 전용" }) as HTMLButtonElement).disabled).toBe(true);
  await userEvent.click(screen.getByRole("option", { name: "전체 액세스" }));
  expect(permission.textContent).toContain("전체 액세스");
  expect(screen.getAllByText("방에 있는 누구의 말이든 이 컴퓨터에서 승인 없이 명령으로 실행될 수 있어요.")).toHaveLength(1);
});
