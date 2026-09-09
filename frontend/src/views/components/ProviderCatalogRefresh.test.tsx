import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { refreshProviderCatalog } from "../../api/providerOperations";
import type { ProviderCatalog } from "../../types/generated/ProviderCatalog";
import ProviderCatalogRefresh from "./ProviderCatalogRefresh";

vi.mock("../../api/providerOperations", () => ({ refreshProviderCatalog: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("refreshes only on demand and distinguishes individual discovery failures", async () => {
  vi.mocked(refreshProviderCatalog).mockResolvedValue({
    status: "ready", catalog_revision: "fresh", discovered_at: "2026-09-09T00:00:00Z",
    providers: [{ discovery_status: "ready" }, { discovery_status: "failed" }],
  } as ProviderCatalog);
  render(<ProviderCatalogRefresh />);
  expect(refreshProviderCatalog).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "모델 목록 새로고침" }));
  expect((await screen.findByRole("status")).textContent).toContain("1개 제공자 확인, 1개 확인 불가");
  vi.mocked(refreshProviderCatalog).mockRejectedValue(new Error("연결 실패"));
  fireEvent.click(screen.getByRole("button", { name: "모델 목록 새로고침" }));
  expect(await screen.findByText("연결 실패")).toBeTruthy();
  expect(screen.queryByText(/1개 제공자 확인/)).toBeNull();
});
