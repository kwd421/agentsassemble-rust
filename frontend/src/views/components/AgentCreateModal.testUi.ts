import { fireEvent, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, type Mock } from "vitest";

type AgentCreateApiMocks = Record<
  | "chooseLocalWorkspace"
  | "deleteProviderCredential"
  | "fetchProviderCredentialStatus"
  | "setProviderCredential",
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
  apiMocks.setProviderCredential.mockReset();
  apiMocks.setProviderCredential.mockResolvedValue({
    configured: true,
    source: "keyring",
  });
}

export function primaryActionButton(): HTMLButtonElement {
  const button = screen
    .getByRole("dialog", { name: "에이전트 추가" })
    .querySelector<HTMLButtonElement>(".dc-agent-footer-actions .dc-button--primary");
  if (!button) throw new Error("Agent create primary action was not rendered");
  return button;
}

export async function chooseWorkspace(): Promise<void> {
  await userEvent.click(screen.getByRole("button", { name: "폴더 선택" }));
}

export async function chooseProviderControl(label: string, option: string): Promise<void> {
  const toggle = screen.queryByRole("switch", { name: label });
  if (toggle) {
    const wantOn = option !== "기본";
    if ((toggle.getAttribute("aria-checked") === "true") !== wantOn) await userEvent.click(toggle);
    if ((toggle.getAttribute("aria-checked") === "true") !== wantOn) await userEvent.click(toggle);
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
  const control = screen.getByLabelText(label);
  if (control.getAttribute("role") === "switch" && option !== "선택 필요") {
    expect(control.getAttribute("aria-checked")).toBe(String(option !== "기본"));
    return;
  }
  expect(control.textContent).toContain(option);
}

/** Goes back to the AI list when the settings step is showing and chooses the row whose name matches. */
export function pickAi(name: string | RegExp): void {
  const opener = screen.queryByRole("button", { name: "AI 다시 고르기" });
  if (opener) fireEvent.click(opener);
  fireEvent.click(screen.getByRole("listitem", { name: typeof name === "string" ? new RegExp(name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")) : name }));
}
