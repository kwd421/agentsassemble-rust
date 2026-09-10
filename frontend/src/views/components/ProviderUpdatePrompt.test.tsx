import { StrictMode } from "react";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { providerUpdateOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import { ApiError } from "../../lib/apiErrors";
import ProviderUpdatePrompt from "./ProviderUpdatePrompt";

vi.mock("../../api/providerOperations", () => ({ providerUpdateOperation: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ openProviderSetupHelp: vi.fn() }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const offer = { provider_id: "grok", current_version: "1.0.5", latest_version: "1.0.24",
  update_available: true, native_update: true, observed_at: "2026-09-09T00:00:00Z", completed: false };

it("checks automatically once in StrictMode and Later dismisses without an update", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValue(offer);
  render(<StrictMode><ProviderUpdatePrompt providerId="grok" /></StrictMode>);
  fireEvent.click(await screen.findByRole("button", { name: "나중에" }));
  expect(providerUpdateOperation).toHaveBeenCalledExactlyOnceWith("grok", undefined);
  expect(screen.queryByRole("region")).toBeNull();
  expect(screen.queryByRole("button")).toBeNull();
});

it("leaves no permanent controls when no newer version exists", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValue({ ...offer, update_available: false });
  const { container } = render(<ProviderUpdatePrompt providerId="grok" />);
  await waitFor(() => expect(providerUpdateOperation).toHaveBeenCalledOnce());
  expect(container.textContent).toBe("");
  expect(screen.queryByRole("button")).toBeNull();
});

it("sends the displayed version once and reports only the confirmed installed version", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer);
  const updating = vi.fn();
  render(<ProviderUpdatePrompt providerId="grok" onUpdating={updating} />);
  const button = await screen.findByRole("button", { name: "업데이트" });
  updating.mockClear();
  let finish!: (value: typeof offer) => void;
  vi.mocked(providerUpdateOperation).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  fireEvent.click(button);
  fireEvent.click(button);
  expect(providerUpdateOperation).toHaveBeenCalledTimes(2);
  expect(providerUpdateOperation).toHaveBeenLastCalledWith("grok", offer.latest_version);
  expect(updating).toHaveBeenCalledExactlyOnceWith(true);
  expect(screen.queryByText(/업데이트했어요/)).toBeNull();
  await act(async () => finish({ ...offer, current_version: offer.latest_version, update_available: false, completed: true }));
  expect(screen.getByText("1.0.24 버전으로 업데이트했어요.")).toBeTruthy();
  expect(updating).toHaveBeenLastCalledWith(false);
  expect(openProviderSetupHelp).not.toHaveBeenCalled();
});

it("rechecks a lost response without repeating the installation", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer)
    .mockRejectedValueOnce(new TypeError("Failed to fetch"))
    .mockResolvedValueOnce({ ...offer, current_version: offer.latest_version, update_available: false, completed: true });
  render(<ProviderUpdatePrompt providerId="grok" />);
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  expect((await screen.findByRole("alert")).textContent).toContain("업데이트 결과를 다시 확인");
  expect(screen.queryByRole("button", { name: "업데이트" })).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "버전 다시 확인" }));
  await screen.findByText("1.0.24 버전으로 업데이트했어요.");
  expect(providerUpdateOperation).toHaveBeenLastCalledWith("grok", undefined);
});


it.each([
  ["provider_update_installation_unconfirmed", false],
  ["provider_update_cleanup_unconfirmed", true],
  ["provider_update_busy", true],
])("keeps the execution guard consistent with owner error %s", async (code, guarded) => {
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer)
    .mockRejectedValueOnce(new ApiError(503, "업데이트 결과", code));
  const updating = vi.fn();
  render(<ProviderUpdatePrompt providerId="grok" onUpdating={updating} />);
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  await screen.findByRole("alert");
  expect(updating).toHaveBeenLastCalledWith(guarded);
});
