import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createLocalAttendee, fetchLocalAttendee, commandLocalAttendee } from "../../api/localAttendee";
import { fetchLocalProviderCatalog, refreshLocalProviderCatalog } from "../../api/providerOperations";
import { chooseLocalWorkspace } from "../../api";
import { ApiError } from "../../lib/apiErrors";
import { localAttendeeLink } from "../../lib/localAttendee";
import { codexProvider } from "./AgentCreateModal.testProviders";
import LocalAttendeePanel from "./LocalAttendeePanel";

vi.mock("../../api/localAttendee", async (original) => ({ ...await original<typeof import("../../api/localAttendee")>(),
  createLocalAttendee: vi.fn(), fetchLocalAttendee: vi.fn(), commandLocalAttendee: vi.fn() }));
vi.mock("../../api/providerOperations", () => ({ fetchLocalProviderCatalog: vi.fn(), refreshLocalProviderCatalog: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true, requestDesktopHostProductSurface: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../api", async (original) => ({ ...await original<typeof import("../../api")>(), chooseLocalWorkspace: vi.fn() }));
const packet = { request_id: "00000000-0000-4000-8000-000000000001", room_id: "remote-room", room_uid: "00000000-0000-4000-8000-000000000002",
  invite_id: "00000000-0000-4000-8000-000000000003", display_name: "Browser draft", provider: "codex",
  attend_command: "assemble room attend --provider codex", expires_at: "2099-01-01T00:00:00Z", join_url: "https://room.example.test/join?token=fixture-invitation" };
const admitted = { request_id: packet.request_id, room_id: packet.room_id, room_uid: packet.room_uid, participant_id: "remote-participant", phase: "admitted" as const, error_code: null };
const missing = () => new ApiError(404, "missing", "local_attendee_missing");
beforeEach(() => {
  const fragment = new URL(localAttendeeLink(packet)).hash;
  window.history.replaceState(null, "", `/?attendee-create=${packet.request_id}${fragment}`);
  const catalog = { status: "ready", catalog_revision: "local-revision", providers: [codexProvider()] };
  vi.mocked(fetchLocalProviderCatalog).mockResolvedValue(catalog);
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValue(catalog);
  vi.mocked(fetchLocalAttendee).mockRejectedValue(missing());
  vi.mocked(chooseLocalWorkspace).mockResolvedValue({ selected: true, path: "/local/workspace" });
});
afterEach(() => { cleanup(); vi.resetAllMocks(); window.history.replaceState(null, "", "/"); });

async function chooseDraft() {
  await screen.findByRole("dialog", { name: "에이전트 추가" });
  await waitFor(() => expect((screen.getByPlaceholderText("방에 표시될 이름") as HTMLInputElement).value).toBe("Browser draft"));
  fireEvent.click(screen.getByRole("button", { name: "폴더 선택" }));
  await waitFor(() => expect((screen.getByLabelText("선택한 작업 폴더") as HTMLInputElement).value).toBe("/local/workspace"));
}

it("preserves the local draft after rejection, then adds and starts through the same retained owner", async () => {
  vi.mocked(createLocalAttendee).mockRejectedValueOnce(new ApiError(409, "모델을 다시 선택해 주세요.", "invalid_model"))
    .mockResolvedValueOnce(admitted);
  vi.mocked(commandLocalAttendee).mockResolvedValueOnce({ ...admitted, phase: "running" })
    .mockResolvedValueOnce({ ...admitted, phase: "stopped" });
  render(<LocalAttendeePanel />);
  await chooseDraft();
  fireEvent.click(screen.getByRole("switch", { name: "추가하자마자 실행" }));
  fireEvent.click(screen.getByRole("button", { name: "추가" }));
  await screen.findByText("모델을 다시 선택해 주세요.", { selector: ".dc-agent-create-status" });
  expect((screen.getByLabelText("선택한 작업 폴더") as HTMLInputElement).value).toBe("/local/workspace");
  fireEvent.click(screen.getByRole("button", { name: "추가" }));
  fireEvent.click(await screen.findByRole("button", { name: "이 PC에서 실행" }));
  await screen.findByText("이 PC에서 실행 중이에요.");
  fireEvent.click(screen.getByRole("button", { name: "에이전트 종료하고 나가기" }));
  await screen.findByText("에이전트 종료와 방 나가기를 확인했어요.");
  expect(createLocalAttendee).toHaveBeenLastCalledWith(packet, expect.objectContaining({ creation: expect.objectContaining({
    catalog_revision: "local-revision", display_name: "Browser draft", workspace: "/local/workspace", start: false,
  }) }));
  expect(commandLocalAttendee).toHaveBeenNthCalledWith(1, packet, "start");
  expect(commandLocalAttendee).toHaveBeenNthCalledWith(2, packet, "cancel");
});

it("keeps uncertain submitted input exact and never treats a missing read as permission for an edited duplicate", async () => {
  vi.mocked(createLocalAttendee).mockRejectedValueOnce(new TypeError("response lost")).mockResolvedValueOnce(admitted);
  render(<LocalAttendeePanel />);
  await chooseDraft();
  fireEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  await screen.findByRole("button", { name: "같은 요청 다시 시도" });
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
  await waitFor(() => expect(fetchLocalAttendee).toHaveBeenCalledTimes(2));
  await act(async () => {});
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "같은 요청 다시 시도" }));
  await screen.findByRole("button", { name: "이 PC에서 실행" });
  expect(vi.mocked(createLocalAttendee).mock.calls[0][1]).toBe(vi.mocked(createLocalAttendee).mock.calls[1][1]);
});

it("restores an existing operation without mounting setup or submitting again", async () => {
  vi.mocked(fetchLocalAttendee).mockResolvedValue({ ...admitted, phase: "running" });
  render(<LocalAttendeePanel />);
  await screen.findByText("이 PC에서 실행 중이에요.");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(refreshLocalProviderCatalog).not.toHaveBeenCalled();
  expect(createLocalAttendee).not.toHaveBeenCalled();
});
