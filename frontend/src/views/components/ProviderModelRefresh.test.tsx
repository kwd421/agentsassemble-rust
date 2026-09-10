import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { refreshLocalProviderCatalog } from "../../api/providerOperations";
import type { ProviderCatalog } from "../../types/generated/ProviderCatalog";
import ProviderModelRefresh from "./ProviderModelRefresh";

vi.mock("../../api/providerOperations", () => ({ refreshLocalProviderCatalog: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("does not probe before selection, then automatically inspects and manually refreshes only the selection", async () => {
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValue({
    status: "ready", catalog_revision: "fresh", discovered_at: "2026-09-09T00:00:00Z",
    providers: [{ id: "codex", discovery_status: "ready" }, { id: "deepseek", discovery_status: "failed" }],
  } as ProviderCatalog);
  const view = render(<ProviderModelRefresh title="Harness Providers" providerId="" automaticAllowed />);
  expect(refreshLocalProviderCatalog).not.toHaveBeenCalled();
  view.rerender(<ProviderModelRefresh title="Harness Providers" providerId="codex" automaticAllowed />);
  await waitFor(() => expect(refreshLocalProviderCatalog).toHaveBeenCalledWith("codex", false, expect.any(AbortSignal)));
  await waitFor(() => expect(screen.getByRole("button", { name: "모델 목록 새로고침" }).hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByRole("button", { name: "모델 목록 새로고침" }));
  expect(await screen.findByText("모델 목록을 새로고침했어요.")).toBeTruthy();
  expect(refreshLocalProviderCatalog).toHaveBeenLastCalledWith("codex", true, undefined);
  vi.mocked(refreshLocalProviderCatalog).mockRejectedValue(new Error("연결 실패"));
  fireEvent.click(screen.getByRole("button", { name: "모델 목록 새로고침" }));
  expect(await screen.findByText("연결 실패")).toBeTruthy();
  expect(screen.queryByText("모델 목록을 새로고침했어요.")).toBeNull();
});

it("does not inspect an authentication-blocked provider or publish an old selection's completion", async () => {
  let failOld!: (error: Error) => void;
  vi.mocked(refreshLocalProviderCatalog).mockReturnValue(new Promise((_, reject) => { failOld = reject; }));
  const view = render(<ProviderModelRefresh title="Providers" providerId="codex" automaticAllowed />);
  expect(refreshLocalProviderCatalog).toHaveBeenCalledTimes(1);
  view.rerender(<ProviderModelRefresh title="Providers" providerId="deepseek" automaticAllowed={false} />);
  await act(async () => { failOld(new Error("old selection failed")); });
  expect(refreshLocalProviderCatalog).toHaveBeenCalledTimes(1);
  expect(screen.queryByText("old selection failed")).toBeNull();
  expect(screen.getByRole("button", { name: "모델 목록 새로고침" }).hasAttribute("disabled")).toBe(true);
});


it("does not inspect the remote room host's provider from a native guest view", () => {
  render(<ProviderModelRefresh title="Providers" providerId="codex" automaticAllowed localAvailable={false} />);
  expect(refreshLocalProviderCatalog).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "모델 목록 새로고침" })).toBeNull();
});
