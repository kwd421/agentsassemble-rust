import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import AgentCreateModal from "./AgentCreateModal";
import { RoomSocketSayError } from "../../roomSocketClient";
import { codexProvider } from "./AgentCreateModal.testProviders";
import { chooseWorkspace, primaryActionButton, resetAgentCreateApiMocks } from "./AgentCreateModal.testUi";

const apiMocks = vi.hoisted(() => ({ chooseLocalWorkspace: vi.fn(),
  deleteProviderCredential: vi.fn(), fetchProviderCredentialStatus: vi.fn(), setProviderCredential: vi.fn() }));
vi.mock("../../api", async (original) => ({ ...(await original<typeof import("../../api")>()), ...apiMocks }));
afterEach(cleanup);
beforeEach(() => resetAgentCreateApiMocks(apiMocks));

it.each([false, true])("recovers the same creation after uncertain outcome (start=%s)", async (start) => {
  const retry = vi.fn().mockRejectedValueOnce(new Error("connection unavailable")).mockResolvedValue({});
  const onCreate = vi.fn().mockRejectedValue(new RoomSocketSayError("uncertain", "outcome_unknown", retry));
  const onClose = vi.fn();
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" catalogRevision="cat-test"
    providers={[codexProvider()]} onClose={onClose} onCreate={onCreate} />);
  await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await chooseWorkspace();
  const startToggle = screen.getByRole("switch", { name: "추가하자마자 실행" });
  if ((startToggle.getAttribute("aria-checked") === "true") !== start) await userEvent.click(startToggle);
  await userEvent.click(primaryActionButton());
  await screen.findByText("uncertain");
  await userEvent.click(primaryActionButton());
  await screen.findByText("connection unavailable");
  await userEvent.click(primaryActionButton());
  await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
  expect(onCreate).toHaveBeenCalledOnce();
  expect(onCreate).toHaveBeenCalledWith(expect.objectContaining({ startNow: start }));
  expect(retry).toHaveBeenCalledTimes(2);
});

it("uses a new creation intent after the user changes the retained form", async () => {
  const retry = vi.fn();
  const onCreate = vi.fn().mockRejectedValueOnce(new RoomSocketSayError("uncertain", "outcome_unknown", retry))
    .mockResolvedValue(undefined);
  render(<AgentCreateModal open meetingId="room-a" roomLabel="Room A" catalogRevision="cat-test"
    providers={[codexProvider()]} onClose={() => {}} onCreate={onCreate} />);
  await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await chooseWorkspace();
  await userEvent.click(primaryActionButton());
  await screen.findByText("uncertain");
  await userEvent.type(screen.getByPlaceholderText("방에 표시될 이름"), " changed");
  await userEvent.click(primaryActionButton());
  await waitFor(() => expect(onCreate).toHaveBeenCalledTimes(2));
  expect(retry).not.toHaveBeenCalled();
});

it("retains the exact creation retry when only the catalog revision refreshes", async () => {
  const retry = vi.fn().mockResolvedValue(undefined);
  const onCreate = vi.fn().mockRejectedValue(new RoomSocketSayError("uncertain", "outcome_unknown", retry));
  const onClose = vi.fn();
  const providers = [codexProvider()];
  const props = { open: true, meetingId: "room-a", roomLabel: "Room A", providers, onClose, onCreate };
  const view = render(<AgentCreateModal {...props} catalogRevision="cat-before" />);
  await userEvent.click(screen.getByRole("listitem", { name: "Codex" }));
  await chooseWorkspace();
  await userEvent.click(primaryActionButton());
  await screen.findByText("uncertain");
  view.rerender(<AgentCreateModal {...props} catalogRevision="cat-after" />);
  await userEvent.click(primaryActionButton());
  await waitFor(() => expect(retry).toHaveBeenCalledOnce());
  expect(onCreate).toHaveBeenCalledOnce();
  expect(onClose).toHaveBeenCalledOnce();
});
