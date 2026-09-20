import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { providerInstallOperation, providerUpdateOperation, refreshLocalProviderCatalog } from "../../api/providerOperations";
import { ApiError } from "../../lib/apiErrors";
import { codexProvider } from "./AgentCreateModal.testProviders";
import ProviderSetupPanel from "./ProviderSetupPanel";

vi.mock("../../api/providerOperations", () => ({ providerInstallOperation: vi.fn(), providerUpdateOperation: vi.fn(), refreshLocalProviderCatalog: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true,
  requestDesktopHostProductSurface: vi.fn().mockResolvedValue(undefined), openProviderSetupHelp: vi.fn() }));
vi.mock("./ProviderLogin", () => ({ default: () => null }));
afterEach(() => { cleanup(); vi.clearAllMocks(); });
const ready = { ...codexProvider(), update_supported: true };
const catalog = (provider = ready) => ({ status: "ready", catalog_revision: "revision", discovered_at: "", providers: [provider] });
const offer = { provider_id: "codex", current_version: "1.0.0", latest_version: "2.0.0",
  update_available: true, native_update: true, completed: false, observed_at: "2026-09-10T00:00:00Z" };

it("replaces stale readiness after an installed update with failed discovery and retries only the catalog", async () => {
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValueOnce(catalog())
    .mockResolvedValueOnce(catalog({ ...ready, startable: false, discovery_status: "failed",
      discovery_error_code: "model_discovery_failed", discovery_error: "모델 목록을 확인하지 못했어요." }))
    .mockResolvedValueOnce(catalog());
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer)
    .mockRejectedValueOnce(new ApiError(503, "업데이트했지만 모델 목록을 갱신하지 못했어요. 모델 목록을 새로고침해 주세요.", "provider_update_catalog_unavailable"));
  render(<ProviderSetupPanel providerId="codex" />);
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  await screen.findByText("모델 목록을 확인하지 못했어요.");
  expect(screen.queryByText("이 PC에서 사용할 준비가 됐어요.")).toBeNull();
  expect(screen.getByRole("alert").textContent).toContain("업데이트했지만");
  expect(refreshLocalProviderCatalog).toHaveBeenLastCalledWith("codex", false, undefined);
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "설정 후 상태 다시 확인" })));
  await screen.findByText("이 PC에서 사용할 준비가 됐어요.");
  expect(refreshLocalProviderCatalog).toHaveBeenLastCalledWith("codex", true, undefined);
  expect(providerUpdateOperation).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).toBeNull();
});

it("rereads readiness after confirmed update success", async () => {
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValue(catalog());
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer)
    .mockResolvedValueOnce({ ...offer, completed: true, update_available: false, current_version: "2.0.0" });
  render(<ProviderSetupPanel providerId="codex" />);
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  await waitFor(() => expect(refreshLocalProviderCatalog).toHaveBeenCalledTimes(2));
  await screen.findByText("2.0.0 버전으로 업데이트했어요.");
  await screen.findByText("이 PC에서 사용할 준비가 됐어요.");
});

it("recovers an uncertain install through check and unlocks parent catalog refresh", async () => {
  const missing = { ...ready, startable: false, available: false, install_supported: true,
    discovery_error_code: "command_missing", discovery_status: "failed" } as const;
  const installed = { ...ready, update_supported: false };
  const installation = { provider_id: "codex", package: "@openai/codex", version: "1.0.0",
    command: ["npm", "install", "@openai/codex@1.0.0"], completed: false };
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValueOnce(catalog(missing))
    .mockResolvedValueOnce(catalog(installed));
  vi.mocked(providerInstallOperation).mockResolvedValueOnce(installation)
    .mockRejectedValueOnce(new ApiError(503, "설치 상태가 불확실해요.", "provider_install_cleanup_unconfirmed"))
    .mockResolvedValueOnce({ ...installation, completed: true });
  render(<ProviderSetupPanel providerId="codex" />);
  fireEvent.click(await screen.findByRole("button", { name: "앱에서 설치하기" }));
  fireEvent.click(await screen.findByRole("button", { name: "설치" }));
  await screen.findByText(/설치가 끝났는지 상태를 다시 확인해 주세요/);
  expect((screen.getByRole("button", { name: "설정 후 상태 다시 확인" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "앱에서 설치하기" }));
  await screen.findByText("이 PC에서 사용할 준비가 됐어요.");
  await waitFor(() => expect((screen.getByRole("button", { name: "설정 후 상태 다시 확인" }) as HTMLButtonElement).disabled).toBe(false));
  expect(refreshLocalProviderCatalog).toHaveBeenCalledTimes(2);
  expect(providerInstallOperation).toHaveBeenLastCalledWith("codex");
});
