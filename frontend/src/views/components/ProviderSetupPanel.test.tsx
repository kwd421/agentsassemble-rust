import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { providerUpdateOperation, refreshLocalProviderCatalog } from "../../api/providerOperations";
import { ApiError } from "../../lib/apiErrors";
import { codexProvider } from "./AgentCreateModal.testProviders";
import ProviderSetupPanel from "./ProviderSetupPanel";

vi.mock("../../api/providerOperations", () => ({ providerUpdateOperation: vi.fn(), refreshLocalProviderCatalog: vi.fn() }));
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
