import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, type Mock } from "vitest";

type AgentCreateApiMocks = Record<
  | "chooseLocalWorkspace"
  | "deleteProviderCredential"
  | "fetchProviderCredentialStatus"
  | "refreshProviderCatalog"
  | "setProviderCredential"
  | "startProviderLogin",
  Mock
>;

export function resetAgentCreateApiMocks(apiMocks: AgentCreateApiMocks): void {
  apiMocks.chooseLocalWorkspace.mockReset();
  apiMocks.chooseLocalWorkspace.mockResolvedValue({
    selected: true,
    path: "/tmp/agentsassemble-workspace",
  });
  apiMocks.fetchProviderCredentialStatus.mockReset();
  apiMocks.fetchProviderCredentialStatus.mockResolvedValue({
    configured: false,
    source: "missing",
  });
  apiMocks.deleteProviderCredential.mockReset();
  apiMocks.refreshProviderCatalog.mockReset();
  apiMocks.refreshProviderCatalog.mockResolvedValue({
    status: "ready",
    catalog_revision: "cat-authenticated",
    providers: [],
  });
  apiMocks.setProviderCredential.mockReset();
  apiMocks.setProviderCredential.mockResolvedValue({
    configured: true,
    source: "keyring",
  });
  apiMocks.startProviderLogin.mockReset();
  apiMocks.startProviderLogin.mockResolvedValue({
    status: "authenticated",
    provider_id: "cursor",
    message: "Cursor 로그인이 완료됐습니다.",
  });
}

export function primaryActionButton(): HTMLButtonElement {
  const button = screen
    .getByRole("dialog", { name: "에이전트 추가" })
    .querySelector<HTMLButtonElement>(".dc-agent-create-primary");
  if (!button) throw new Error("Agent create primary action was not rendered");
  return button;
}

export async function chooseWorkspace(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "폴더 선택" }));
}

export async function chooseProviderControl(label: string, option: string): Promise<void> {
  const toggle = screen.queryByRole("switch", { name: label });
  if (toggle) {
    if (!toggle.textContent?.includes(option)) await userEvent.click(toggle);
    if (!toggle.textContent?.includes(option)) await userEvent.click(toggle);
    return;
  }
  await userEvent.click(screen.getByRole("combobox", { name: label }));
  const directOption = screen.queryByRole("menuitemradio", { name: option });
  if (directOption) {
    await userEvent.click(directOption);
    return;
  }
  const family = modelFamilyFromLabel(option);
  if (label === "모델" && family) {
    const familyItem = screen.queryByRole("menuitem", { name: family });
    if (familyItem) await userEvent.click(familyItem);
  }
  await userEvent.click(screen.getByRole("option", { name: option }));
}

function modelFamilyFromLabel(label: string): string {
  return ["Haiku", "Sonnet", "Opus", "Fable", "GPT", "Gemini", "Grok", "GLM", "Kimi"]
    .find((family) => new RegExp(`(^|\\s)${family}(\\s|$)`, "i").test(label)) || "";
}

export function expectProviderControlValue(label: string, option: string): void {
  expect(screen.getByLabelText(label).textContent).toContain(option);
}
