import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import AgentCreateModal from "./AgentCreateModal";
import { defaultAgentDisplayName } from "./agentCreateModel";
import { agentCreationPayload } from "../../api/agentSessions";
import { localAttendeeCreateRequest } from "../../api/localAttendee";
import { pickAi, chooseWorkspace } from "./AgentCreateModal.testUi";
import { workPermissionControl, codexProvider, claudeProvider, openCodeProvider, deepSeekProvider, lmStudioProvider } from "./AgentCreateModal.testProviders";

vi.mock("../../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../api")>()),
  chooseLocalWorkspace: vi.fn().mockResolvedValue({ selected: true, path: "/tmp/local-workspace" }),
}));

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
    expect(hint.compareDocumentPosition(screen.getByRole("button", { name: "AI 고르기" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    pickAi("Codex");
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
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  expect(within(screen.getByRole("list", { name: "AI" })).getAllByRole("listitem").map((item) => item.getAttribute("aria-label")))
    .toEqual(["ChatGPT Codex", "DeepSeek", "LM Studio"]);
  expect(within(screen.getByRole("list", { name: "라우터" })).getAllByRole("listitem").map((item) => item.getAttribute("aria-label")))
    .toEqual(["OpenCode"]);
  // A missing app is still offered: choosing it leads to its setup instead of a dead end.
  expect(screen.getByRole("listitem", { name: "ChatGPT Codex" }).textContent).toContain("설치 필요");
  pickAi(/Codex/);
  const setup = screen.getByRole("link", { name: "이 컴퓨터에서 Codex 설정 열기" });
  expect(setup.getAttribute("href")).toBe("agentsassemble://provider-setup/codex");
  expect((screen.getByRole("button", { name: "추가" }) as HTMLButtonElement).disabled).toBe(true);
  expect(onCreate).not.toHaveBeenCalled();
});

it("does not call a still-loading app missing, and marks it once discovery says so", () => {
  const providers = [codexProvider(), claudeProvider(), openCodeProvider()].map((provider) => ({
    ...provider, available: false, startable: false, discovery_status: "loading",
    catalog_source: "discovered", controls: [], default_model: "",
  }));
  const { rerender } = render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={providers} onClose={() => undefined} onCreate={vi.fn()} />);
  fireEvent.click(screen.getByRole("button", { name: "AI 고르기" }));
  const row = (name: RegExp) => screen.getByRole("listitem", { name });
  expect(row(/ChatGPT/).textContent).not.toContain("설치 필요");
  expect(row(/Claude/).textContent).not.toContain("설치 필요");

  rerender(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[{ ...providers[0], discovery_status: "failed", discovery_error_code: "command_missing" },
      providers[1], openCodeProvider()]}
    onClose={() => undefined} onCreate={vi.fn()} />);
  expect(row(/ChatGPT/).textContent).toContain("설치 필요");
  expect(row(/Claude/).textContent).not.toContain("설치 필요");
});

it("disables conversation-only permission for a free OpenCode model", async () => {
  const onCreate = vi.fn().mockResolvedValue(undefined);
  const provider = openCodeProvider();
  const model = provider.controls[0];
  model.default_value = model.options[0].value;
  const permissions = workPermissionControl();
  permissions.options.push({ value: "full_access", label: "전체 액세스" });
  provider.controls.push(permissions);
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A"
    providers={[provider]} catalogRevision="local-revision" onClose={() => undefined} onCreate={onCreate} />);
  pickAi("OpenCode");
  expect(screen.getByText("OpenCode 무료 모델은 작업 폴더 쓰기나 전체 액세스 권한이 필요해요.")).toBeTruthy();
  const permission = screen.getByRole("combobox", { name: "권한" });
  expect(permission.textContent).toContain("작업 폴더 쓰기");
  await userEvent.click(permission);
  expect((screen.getByRole("option", { name: "대화 전용" }) as HTMLButtonElement).disabled).toBe(true);
  await userEvent.click(screen.getByRole("option", { name: "전체 액세스" }));
  expect(permission.textContent).toContain("전체 액세스");
  expect(screen.getAllByText("방에 있는 누구의 말이든 이 컴퓨터에서 승인 없이 명령으로 실행될 수 있어요.")).toHaveLength(1);
  await chooseWorkspace();
  await userEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  const request = onCreate.mock.calls[0][0];
  expect(request.fullAccessAcknowledged).toBe(true);
  expect(agentCreationPayload(request)).toMatchObject({ full_access_acknowledged: true });
  const packet = { request_id: "local-request", room_id: "room-a", room_uid: "room-uid", provider: "opencode", display_name: "Local", invite_id: "invite", expires_at: "2030-01-01T00:00:00Z", attend_command: "attend", join_url: "https://example.test/join?token=secret" };
  expect(localAttendeeCreateRequest(packet, request).creation).toMatchObject({ full_access_acknowledged: true });
  expect(agentCreationPayload({ ...request, fullAccessAcknowledged: undefined })).not.toHaveProperty("full_access_acknowledged");
});


it.each([false, true])("defaults to the catalog label and preserves typed names (local: %s)", async localProviderActions => {
  const provider = openCodeProvider();
  provider.controls[0].default_value = "opencode/mimo-v2.6-flash-free";
  provider.controls[0].options = [{ value: "opencode/mimo-v2.6-flash-free", label: "MiMo 2.6 Flash" }];
  const props = { open: true, meetingId: "room-a", roomLabel: "Room A", localProviderActions,
    providers: [provider, codexProvider()], onClose: vi.fn(), onCreate: vi.fn() };
  const { rerender } = render(<AgentCreateModal {...props} />);
  pickAi("OpenCode");
  const name = screen.getByLabelText("표시 이름") as HTMLInputElement;
  expect(name.value).toBe("MiMo 2.6 Flash");
  const refreshed = { ...provider, controls: [{ ...provider.controls[0], options: [
    { value: "opencode/mimo-v2.6-flash-free", label: "opencode/mimo-v2.6-flash-free" },
  ] }] };
  rerender(<AgentCreateModal {...props} providers={[refreshed, codexProvider()]} />);
  expect(name.value).toBe("mimo-v2.6-flash-free");
  fireEvent.change(name, { target: { value: "My agent" } });
  rerender(<AgentCreateModal {...props} />);
  expect(name.value).toBe("My agent");
  pickAi("Codex");
  expect(name.value).toBe("My agent");
});


it.each(["", "opencode/mimo-v2.6-flash-free"])("uses the readable model id segment for a non-human catalog label: %s", label => {
  const provider = openCodeProvider();
  provider.controls[0].options = [{ value: "opencode/mimo-v2.6-flash-free", label }];
  expect(defaultAgentDisplayName(provider, { model: "opencode/mimo-v2.6-flash-free" })).toBe("mimo-v2.6-flash-free");
  expect(defaultAgentDisplayName(provider, { model: "" })).toBe("OpenCode");
});
