import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AppOverlays from "./AppOverlays";
import type { AppController } from "./useAppController";
import type { CompanionInviteControls } from "./useCompanionInvites";
import { codexProvider } from "../views/components/AgentCreateModal.testProviders";

vi.mock("../lib/desktopBridge", async (original) => ({ ...await original<typeof import("../lib/desktopBridge")>(), isDesktopWebview: () => true }));
afterEach(cleanup);
const controls = { available: true, provider: "", displayName: "", creating: false, status: "", invites: [],
  setProvider: vi.fn(), setDisplayName: vi.fn(), create: vi.fn(), copy: vi.fn() } as CompanionInviteControls;
function controller(owner: boolean, local = false): AppController {
  return { activeRoom: { label: "Room", meetingId: "general" }, agentCreateOpen: true,
    canControlActiveAgents: owner, setAgentCreateOpen: vi.fn(),
    canonicalRoom: { availableProviders: [{ ...codexProvider(), available: false, startable: false }],
      providerCatalog: { catalog_revision: "catalog" }, agentSessions: [], participantRecords: [] },
    guestSession: local ? null : { operator: owner, serverSurface: { server_id: "server" } },
    centralDirectory: { servers: [{ server_id: "server", alias: "집 Mac" }] },
    roomLifecycle: { enabled: false }, pairedRoomLifecycle: { enabled: false },
  } as unknown as AppController;
}
it("offers remote owners both computers without claiming remote discovery is local availability", () => {
  render(<AppOverlays controller={controller(true)} companionInvites={controls} />);
  expect(screen.getByRole("radio", { name: "이 컴퓨터" })).toBeTruthy();
  expect(screen.getByRole("radiogroup", { name: "어디서 실행할까요?" })).toBeTruthy();
  expect(screen.getByText("집 Mac")).toBeTruthy();
  expect(screen.getByRole("listitem", { name: "Codex" }).getAttribute("data-unavailable")).toBe("false");
  expect(screen.queryByRole("combobox")).toBeNull();
  expect(screen.queryByText("목록에만 추가")).toBeNull();
  expect(screen.getByRole("button", { name: "이 컴퓨터에서 계속" }).closest("footer")).toBeTruthy();
  expect(screen.queryByRole("combobox", { name: "모델" })).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: "서버 컴퓨터" }));
  expect(screen.queryByRole("combobox", { name: "AI 종류" })).toBeNull();
  fireEvent.click(screen.getByRole("radio", { name: "이 컴퓨터" }));
  expect(screen.getByRole("listitem", { name: "Codex" })).toBeTruthy();
});
it("lets writable members add only on this computer without server agent control", () => {
  render(<AppOverlays controller={controller(false)} companionInvites={controls} />);
  expect(screen.getByRole("listitem", { name: "Codex" })).toBeTruthy();
  expect(screen.queryByRole("radio")).toBeNull();
});
it("preserves local server creation and hides creation when neither authority is available", () => {
  const view = render(<AppOverlays controller={controller(true, true)} companionInvites={{ ...controls, available: false }} />);
  expect(screen.getByRole("dialog")).toBeTruthy();
  expect(screen.queryByRole("radio")).toBeNull();
  view.rerender(<AppOverlays controller={controller(false)} companionInvites={{ ...controls, available: false }} />);
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("uses the unknown server caption instead of the page hostname and exposes pending handoff", () => {
  const owner = controller(true);
  owner.centralDirectory = null;
  render(<AppOverlays controller={owner} companionInvites={{ ...controls, creating: true }} />);
  expect(screen.getByText("방을 연 컴퓨터")).toBeTruthy();
  expect(screen.queryByText(window.location.host)).toBeNull();
  expect(screen.getByRole("button", { name: "여는 중…" })).toHaveProperty("disabled", true);
  expect(screen.queryByText("AI 추가")).toBeNull();
});
