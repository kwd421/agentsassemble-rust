import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { codexProvider, claudeProvider, openCodeProvider, deepSeekProvider, lmStudioProvider } from "./AgentCreateModal.testProviders";

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
    await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
    expect(screen.getByLabelText("표시 이름")).toBeTruthy();
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
    .toEqual(["구독 에이전트", "API 키", "내 컴퓨터"]);
  expect(within(screen.getByRole("list", { name: "구독 에이전트 제공자" })).getAllByRole("listitem")
    .map((item) => item.getAttribute("aria-label"))).toEqual(["OpenCode", "Codex"]);
  expect(screen.getByRole("listitem", { name: "Codex" }).getAttribute("data-unavailable")).toBe("true");
  await userEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
  const setup = screen.getByRole("link", { name: "이 PC에서 Codex 설정 열기" });
  expect(setup.getAttribute("href")).toBe("agentsassemble://provider-setup/codex");
  expect((screen.getByRole("button", { name: "추가" }) as HTMLButtonElement).disabled).toBe(true);
  expect(onCreate).not.toHaveBeenCalled();
});

it("keeps installed providers undimmed and in catalog order before model discovery", () => {
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[{ ...codexProvider(), startable: false, discovery_status: "idle", controls: [] },
      { ...claudeProvider(), startable: false, discovery_status: "idle", controls: [] }, openCodeProvider()]}
    onClose={() => undefined} onCreate={vi.fn()} />);
  const choices = within(screen.getByRole("list", { name: "구독 에이전트 제공자" })).getAllByRole("listitem");
  expect(choices.map((item) => item.getAttribute("aria-label"))).toEqual(["Codex", "Claude Code", "OpenCode"]);
  expect(choices.map((item) => item.getAttribute("data-unavailable"))).toEqual(["false", "false", "false"]);
});
