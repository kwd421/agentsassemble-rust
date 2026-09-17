import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { providerInstallOperation } from "../../api/providerOperations";
import { ApiError } from "../../lib/apiErrors";
import ProviderInstallPrompt from "./ProviderInstallPrompt";

vi.mock("../../api/providerOperations", () => ({ providerInstallOperation: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const offer = {
  provider_id: "claude",
  package: "@anthropic-ai/claude-code",
  version: "2.1.271",
  command: ["npm", "install", "--global", "--prefix", "C:\\npm", "@anthropic-ai/claude-code@2.1.271"],
  completed: false,
};

it("runs nothing until the user reads the exact command and confirms that version", async () => {
  vi.mocked(providerInstallOperation).mockResolvedValueOnce(offer);
  const installed = vi.fn();
  const updating = vi.fn();
  render(<ProviderInstallPrompt providerId="claude" displayName="Claude Code" onInstalled={installed} onUpdating={updating} />);

  expect(providerInstallOperation).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "앱에서 설치하기" }));
  expect(await screen.findByText(offer.command.join(" "))).toBeTruthy();
  expect(providerInstallOperation).toHaveBeenCalledExactlyOnceWith("claude");
  expect(installed).not.toHaveBeenCalled();

  vi.mocked(providerInstallOperation).mockResolvedValueOnce({ ...offer, completed: true });
  fireEvent.click(screen.getByRole("button", { name: "설치" }));

  expect(await screen.findByText("Claude Code CLI 2.1.271을 설치했어요.")).toBeTruthy();
  expect(providerInstallOperation).toHaveBeenLastCalledWith("claude", "2.1.271");
  expect(installed).toHaveBeenCalledOnce();
  expect(updating.mock.calls).toEqual([[true], [false]]);
});

it("cancels the offer without installing", async () => {
  vi.mocked(providerInstallOperation).mockResolvedValueOnce(offer);
  render(<ProviderInstallPrompt providerId="claude" displayName="Claude Code" />);
  fireEvent.click(screen.getByRole("button", { name: "앱에서 설치하기" }));
  fireEvent.click(await screen.findByRole("button", { name: "취소" }));

  expect(screen.getByRole("button", { name: "앱에서 설치하기" })).toBeTruthy();
  expect(providerInstallOperation).toHaveBeenCalledOnce();
});

it("keeps the updating guard when an install result is uncertain", async () => {
  vi.mocked(providerInstallOperation).mockResolvedValueOnce(offer);
  const updating = vi.fn();
  render(<ProviderInstallPrompt providerId="claude" displayName="Claude Code" onUpdating={updating} />);
  fireEvent.click(screen.getByRole("button", { name: "앱에서 설치하기" }));
  vi.mocked(providerInstallOperation).mockRejectedValueOnce(new ApiError(503, "설치 프로세스의 종료를 확인하지 못했어요.", "provider_install_cleanup_unconfirmed"));
  fireEvent.click(await screen.findByRole("button", { name: "설치" }));

  expect((await screen.findByRole("alert")).textContent).toContain("상태를 다시 확인해 주세요");
  expect(updating.mock.calls).toEqual([[true]]);
});
