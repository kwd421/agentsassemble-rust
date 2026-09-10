import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AgentCreateModal from "./AgentCreateModal";
import { codexProvider } from "./AgentCreateModal.testProviders";
import { postJsonServerOperator } from "../../api/http";

vi.mock("../../api/http", async (original) => ({
  ...await original<typeof import("../../api/http")>(), postJsonServerOperator: vi.fn(),
}));
vi.mock("../../lib/desktopBridge", async (original) => ({
  ...await original<typeof import("../../lib/desktopBridge")>(), isDesktopWebview: () => true,
}));
vi.mock("./ProviderModelRefresh", () => ({ default: () => null }));
vi.mock("./ProviderSetupActions", () => ({ default: () => null }));
afterEach(() => { cleanup(); vi.resetAllMocks(); });

it("returns required login to the same creation draft and removes the login control", async () => {
  let complete!: (value: unknown) => void;
  vi.mocked(postJsonServerOperator).mockImplementation(() => new Promise((resolve) => { complete = resolve; }));
  const ready = { ...codexProvider(), login_supported: true };
  const required = { ...ready, startable: false, discovery_status: "failed" as const,
    discovery_error_code: "authentication_required" };
  const props = { open: true, meetingId: "room-a", roomLabel: "Room A", onClose: vi.fn(), onCreate: vi.fn() };
  const view = render(<AgentCreateModal {...props} providers={[required]} />);
  fireEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
  await screen.findByText("로그인 창에서 안내를 따라 주세요.");
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "My preserved draft" } });
  await act(async () => complete({ provider_id: "codex", status: "authenticated" }));
  view.rerender(<AgentCreateModal {...props} providers={[ready]} />);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("My preserved draft");
  expect(screen.queryByRole("button", { name: "Codex 로그인" })).toBeNull();
  expect(postJsonServerOperator).toHaveBeenCalledExactlyOnceWith("/api/providers/login", { provider_id: "codex" });
});

it.each(["ready", "probe_failed", "remote"])("does not start local login for %s", async (state) => {
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" onClose={vi.fn()} onCreate={vi.fn()}
    localProviderActions={state !== "remote"} providers={[{ ...codexProvider(), login_supported: true,
      discovery_error_code: state === "remote" ? "authentication_required" : state }]} />);
  fireEvent.click(screen.getByRole("listitem", { name: /Codex/ }));
  await act(async () => {});
  expect(postJsonServerOperator).not.toHaveBeenCalled();
  expect(screen.queryByRole("button", { name: "Codex 로그인" })).toBeNull();
});
