import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AgentCreateModal from "./AgentCreateModal";
import { chooseLocalWorkspace } from "../../api";
import { codexProvider, claudeProvider } from "./AgentCreateModal.testProviders";
import { providerUpdateOperation } from "../../api/providerOperations";

vi.mock("../../api", async (original) => ({
  ...await original<typeof import("../../api")>(), chooseLocalWorkspace: vi.fn(),
}));
vi.mock("../../api/providerOperations", async (original) => ({
  ...await original<typeof import("../../api/providerOperations")>(), providerUpdateOperation: vi.fn(),
}));
vi.mock("../../lib/desktopBridge", async (original) => ({
  ...await original<typeof import("../../lib/desktopBridge")>(), isDesktopWebview: () => true,
}));
vi.mock("./ProviderModelRefresh", () => ({ default: () => null }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });
const offer = { provider_id: "codex", current_version: "1.0.0", latest_version: "2.0.0",
  update_available: true, native_update: true, completed: false, observed_at: "2026-09-10T00:00:00Z" };

it("preserves the creation draft and prevents creation during the selected CLI update", async () => {
  vi.mocked(providerUpdateOperation).mockResolvedValueOnce(offer);
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" catalogRevision="revision"
    onClose={vi.fn()} onCreate={vi.fn()} providers={[{ ...codexProvider(), update_supported: true }]} />);
  fireEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
  const update = await screen.findByRole("button", { name: "업데이트" });
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "Update draft" } });
  let complete!: (value: typeof offer) => void;
  vi.mocked(providerUpdateOperation).mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
  vi.mocked(chooseLocalWorkspace).mockResolvedValue({ selected: true, path: "/tmp/agentsassemble-workspace" });
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "폴더 선택" })));
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(update);
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(true);
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "Edited during update" } });
  await act(async () => complete({ ...offer, current_version: "2.0.0", update_available: false, completed: true }));
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("Edited during update");
  expect(providerUpdateOperation).toHaveBeenLastCalledWith("codex", "2.0.0");
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(false);
});

it.each(["remote", "authentication_required", "command_missing", "loading"])(
  "does not inspect a CLI version before local discovery/authentication: %s", async (state) => {
    render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" onClose={vi.fn()} onCreate={vi.fn()}
      localProviderActions={state !== "remote"} providers={[{ ...codexProvider(), update_supported: true,
        discovery_status: state === "loading" ? "loading" : "failed", discovery_error_code: state }]} />);
    fireEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
    await act(async () => {});
    expect(providerUpdateOperation).not.toHaveBeenCalled();
  }
);


it("retains each pending installation when selections and completion order differ", async () => {
  const finishes = new Map<string, (value: typeof offer) => void>();
  vi.mocked(providerUpdateOperation).mockImplementation(async (id, version) => {
    if (!version) return { ...offer, provider_id: id };
    return new Promise((resolve) => finishes.set(id, resolve));
  });
  vi.mocked(chooseLocalWorkspace).mockResolvedValue({ selected: true, path: "/tmp/agentsassemble-workspace" });
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" catalogRevision="revision"
    onClose={vi.fn()} onCreate={vi.fn()} providers={[codexProvider(), claudeProvider()].map((p) => ({ ...p, update_supported: true }))} />);
  fireEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await act(async () => fireEvent.click(screen.getByRole("button", { name: "폴더 선택" })));
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  fireEvent.click(screen.getByRole("listitem", { name: "Claude Code" }));
  fireEvent.click(await screen.findByRole("button", { name: "업데이트" }));
  fireEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await screen.findByRole("button", { name: "업데이트" });
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finishes.get("claude")?.({ ...offer, provider_id: "claude", current_version: "2.0.0", completed: true, update_available: false }));
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(true);
  await act(async () => finishes.get("codex")?.({ ...offer, current_version: "2.0.0", completed: true, update_available: false }));
  expect((screen.getByRole("button", { name: "추가하고 실행" }) as HTMLButtonElement).disabled).toBe(false);
});
