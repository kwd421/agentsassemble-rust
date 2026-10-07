import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createLocalAttendee, fetchLocalAttendee, commandLocalAttendee } from "../../api/localAttendee";
import { fetchLocalProviderCatalog, refreshLocalProviderCatalog } from "../../api/providerOperations";
import { chooseLocalWorkspace } from "../../api";
import { ApiError } from "../../lib/apiErrors";
import { localAttendeeLink } from "../../lib/localAttendee";
import { codexProvider, openCodeProvider, workPermissionControl } from "./AgentCreateModal.testProviders";
import { requestDesktopBootstrapStatus } from "../../lib/desktopBridge";
import { bootstrapCentral } from "../../lib/central/identity";
import { saveLocalProfile } from "../../lib/localProfile";
import LocalAttendeePanel from "./LocalAttendeePanel";
import type { LocalAttendeeStatus } from "../../types/generated/LocalAttendeeStatus";

vi.mock("../../api/localAttendee", async (original) => ({ ...await original<typeof import("../../api/localAttendee")>(),
  createLocalAttendee: vi.fn(), fetchLocalAttendee: vi.fn(), commandLocalAttendee: vi.fn() }));
vi.mock("../../api/providerOperations", () => ({ fetchLocalProviderCatalog: vi.fn(), refreshLocalProviderCatalog: vi.fn() }));
vi.mock("../../lib/desktopBridge", () => ({ isDesktopWebview: () => true, requestDesktopBootstrapStatus: vi.fn(), requestDesktopHostProductSurface: vi.fn().mockResolvedValue(undefined) }));
vi.mock("../../api", async (original) => ({ ...await original<typeof import("../../api")>(), chooseLocalWorkspace: vi.fn() }));
vi.mock("../../lib/central/identity", () => ({ bootstrapCentral: vi.fn() }));
vi.mock("../../lib/localProfile", () => ({ saveLocalProfile: vi.fn() }));
const packet = { request_id: "00000000-0000-4000-8000-000000000001", room_id: "remote-room", room_uid: "00000000-0000-4000-8000-000000000002",
  invite_id: "00000000-0000-4000-8000-000000000003", display_name: "Browser draft", provider: "codex",
  attend_command: "assemble room attend --provider codex", expires_at: "2099-01-01T00:00:00Z", join_url: "https://room.example.test/join?token=fixture-invitation" };
const admitted = { request_id: packet.request_id, room_id: packet.room_id, room_uid: packet.room_uid, participant_id: "remote-participant", phase: "admitted" as const, error_code: null };
const missing = () => new ApiError(404, "missing", "local_attendee_missing");
beforeEach(() => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase: "complete" } as Awaited<ReturnType<typeof requestDesktopBootstrapStatus>>);
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

it.each([false, true])("offers only supported companion permissions after catalog refresh (full access: %s)", async fullAccess => {
  const provider = codexProvider();
  const permission = workPermissionControl();
  provider.controls.push(permission);
  if (fullAccess) permission.options.push({ value: "full_access", label: "전체 액세스" });
  const catalog = { status: "ready", catalog_revision: "local-revision", providers: [provider] };
  vi.mocked(fetchLocalProviderCatalog).mockResolvedValue(catalog);
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValue(catalog);
  render(<LocalAttendeePanel />);
  await chooseDraft();
  await userEvent.click(screen.getByRole("button", { name: "AI 다시 확인" }));
  await screen.findByText("모델 목록을 새로고침했어요.");
  const control = screen.getByRole("combobox", { name: "권한" });
  expect(control.textContent).toContain("대화 전용");
  if (fullAccess) {
    await userEvent.click(control);
    expect(screen.getAllByRole("option").map(option => option.textContent)).toEqual(["대화 전용", "전체 액세스"]);
  } else {
    expect((control as HTMLButtonElement).disabled).toBe(true);
    expect(screen.queryByText("전체 액세스")).toBeNull();
  }
  expect(screen.queryByText(/작업 폴더 쓰기/)).toBeNull();
});

it("requires warned full access for a free OpenCode companion and submits its acknowledgement", async () => {
  const provider = openCodeProvider();
  provider.controls[0].default_value = provider.controls[0].options[0].value;
  const permission = workPermissionControl();
  permission.options.push({ value: "full_access", label: "전체 액세스" });
  provider.controls.push(permission);
  const catalog = { status: "ready", catalog_revision: "local-revision", providers: [provider] };
  vi.mocked(fetchLocalProviderCatalog).mockResolvedValue(catalog);
  vi.mocked(refreshLocalProviderCatalog).mockResolvedValue(catalog);
  const openCodePacket = { ...packet, provider: "opencode", attend_command: "assemble room attend --provider opencode" };
  window.history.replaceState(null, "", `/?attendee-create=${packet.request_id}${new URL(localAttendeeLink(openCodePacket)).hash}`);
  vi.mocked(createLocalAttendee).mockResolvedValue(admitted);
  render(<LocalAttendeePanel />);
  await chooseDraft();
  expect(screen.getByText("OpenCode 무료 모델은 이 컴퓨터에서 전체 액세스로만 쓸 수 있어요.")).toBeTruthy();
  const control = screen.getByRole("combobox", { name: "권한" });
  expect(control.textContent).toContain("전체 액세스");
  await userEvent.click(control);
  expect((screen.getByRole("option", { name: "대화 전용" }) as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("option", { name: "작업 폴더 쓰기" })).toBeNull();
  await userEvent.click(screen.getByRole("option", { name: "전체 액세스" }));
  expect(screen.getAllByText("방에 있는 누구의 말이든 이 컴퓨터에서 승인 없이 명령으로 실행될 수 있어요.")).toHaveLength(1);
  await userEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  await screen.findByRole("button", { name: "이 컴퓨터에서 실행" });
  expect(createLocalAttendee).toHaveBeenCalledWith(openCodePacket, expect.objectContaining({
    creation: expect.objectContaining({ permission_mode: "full_access", full_access_acknowledged: true }),
  }));
});

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
  fireEvent.click(await screen.findByRole("button", { name: "이 컴퓨터에서 실행" }));
  await screen.findByText("이 컴퓨터에서 실행 중이에요.");
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
  await screen.findByRole("button", { name: "이 컴퓨터에서 실행" });
  expect(vi.mocked(createLocalAttendee).mock.calls[0][1]).toBe(vi.mocked(createLocalAttendee).mock.calls[1][1]);
});

it("restores an existing operation without mounting setup or submitting again", async () => {
  vi.mocked(fetchLocalAttendee).mockResolvedValue({ ...admitted, phase: "running" });
  render(<LocalAttendeePanel />);
  await screen.findByText("이 컴퓨터에서 실행 중이에요.");
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(refreshLocalProviderCatalog).not.toHaveBeenCalled();
  expect(createLocalAttendee).not.toHaveBeenCalled();
});

it.each([false, true])("cancels while %s create-and-start is held and ignores its late running result", async (createAndStart) => {
  let finish!: (value: LocalAttendeeStatus) => void;
  const held = new Promise<LocalAttendeeStatus>((resolve) => { finish = resolve; });
  const stopped: LocalAttendeeStatus = { ...admitted, phase: "stopped" };
  vi.mocked(createLocalAttendee).mockReturnValue(createAndStart ? held : Promise.resolve(admitted));
  vi.mocked(commandLocalAttendee).mockImplementation(async (_packet, action) => action === "cancel" ? stopped : held);
  render(<LocalAttendeePanel />);
  await chooseDraft();
  if (createAndStart) fireEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  else {
    fireEvent.click(screen.getByRole("switch", { name: "추가하자마자 실행" }));
    fireEvent.click(screen.getByRole("button", { name: "추가" }));
    fireEvent.click(await screen.findByRole("button", { name: "이 컴퓨터에서 실행" }));
  }
  const cancel = await screen.findByRole("button", { name: "에이전트 종료하고 나가기" });
  expect((cancel as HTMLButtonElement).disabled).toBe(false);
  fireEvent.click(cancel);
  await screen.findByText("에이전트 종료와 방 나가기를 확인했어요.");
  expect(commandLocalAttendee).toHaveBeenLastCalledWith(packet, "cancel");
  await act(async () => finish({ ...admitted, phase: "running" }));
  expect(screen.queryByText("이 컴퓨터에서 실행 중이에요.")).toBeNull();
  expect(screen.getByText("에이전트 종료와 방 나가기를 확인했어요.")).toBeTruthy();
  expect(createLocalAttendee).toHaveBeenCalledTimes(1);
});

it("retains a failed cancel and exact status access instead of allowing a replacement creation", async () => {
  let finish!: (value: LocalAttendeeStatus) => void;
  vi.mocked(createLocalAttendee).mockReturnValue(new Promise((resolve) => { finish = resolve; }));
  vi.mocked(commandLocalAttendee).mockRejectedValueOnce(missing()).mockResolvedValueOnce({ ...admitted, phase: "stopped" });
  render(<LocalAttendeePanel />);
  await chooseDraft();
  fireEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  fireEvent.click(await screen.findByRole("button", { name: "에이전트 종료하고 나가기" }));
  await screen.findByRole("alert");
  await act(async () => finish({ ...admitted, phase: "running" }));
  expect(screen.queryByRole("button", { name: "같은 요청 다시 시도" })).toBeNull();
  expect(screen.queryByText("에이전트 종료와 방 나가기를 확인했어요.")).toBeNull();
  vi.mocked(fetchLocalAttendee).mockResolvedValue({ ...admitted, phase: "running" });
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
  await screen.findByText("이 컴퓨터에서 실행 중이에요.");
  fireEvent.click(screen.getByRole("button", { name: "에이전트 종료하고 나가기" }));
  await screen.findByText("에이전트 종료와 방 나가기를 확인했어요.");
  expect(commandLocalAttendee).toHaveBeenCalledTimes(2);
  expect(createLocalAttendee).toHaveBeenCalledTimes(1);
});


it("shows the server host with its port and rejects a provider absent from this computer", async () => {
  const link = new URL(localAttendeeLink({ ...packet, join_url: "https://room.example.test:8443/join?token=fixture" }));
  window.history.replaceState(null, "", `/?attendee-create=${packet.request_id}${link.hash}`);
  vi.mocked(fetchLocalProviderCatalog).mockResolvedValue({ status: "ready", catalog_revision: "local", providers: [] });
  render(<LocalAttendeePanel />);
  expect(await screen.findByText("선택한 AI가 현재 목록에 없어요.")).toBeTruthy();
  expect(screen.getByText("room.example.test:8443 방에 참가해요")).toBeTruthy();
  expect(screen.queryByText(/remote-room/)).toBeNull();
  expect(createLocalAttendee).not.toHaveBeenCalled();
});


it("requires explicit preparation and reads the live account only after activation", async () => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase: "empty" } as Awaited<ReturnType<typeof requestDesktopBootstrapStatus>>);
  const person = { display_name: "Local owner", identity_kind: "guest" };
  vi.mocked(bootstrapCentral).mockResolvedValue({ person } as Awaited<ReturnType<typeof bootstrapCentral>>);
  let finish!: () => void;
  vi.mocked(saveLocalProfile).mockReturnValue(new Promise(resolve => { finish = () => resolve({ phase: "complete" } as Awaited<ReturnType<typeof saveLocalProfile>>); }));
  render(<LocalAttendeePanel />);
  const prepare = await screen.findByRole("button", { name: "이 컴퓨터에서 AI를 쓸 준비하기" });
  expect(bootstrapCentral).not.toHaveBeenCalled();
  expect(saveLocalProfile).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
  await waitFor(() => expect((prepare as HTMLButtonElement).disabled).toBe(false));
  expect(saveLocalProfile).not.toHaveBeenCalled();
  expect(fetchLocalProviderCatalog).not.toHaveBeenCalled();
  fireEvent.click(prepare);
  await waitFor(() => expect(saveLocalProfile).toHaveBeenCalledWith("Local owner", expect.any(String), person));
  expect(fetchLocalProviderCatalog).not.toHaveBeenCalled();
  await act(async () => finish());
  await screen.findByRole("dialog");
  expect(createLocalAttendee).not.toHaveBeenCalled();
});

it("exposes native string errors and retries bootstrap before fetching the catalog", async () => {
  vi.mocked(requestDesktopBootstrapStatus).mockRejectedValueOnce("native control unavailable");
  render(<LocalAttendeePanel />);
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "native control unavailable");
  expect(fetchLocalProviderCatalog).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
  await screen.findByRole("dialog");
  expect(bootstrapCentral).not.toHaveBeenCalled();
});

it.each(["repair_required", "initializing"])("keeps %s authority closed", async phase => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase } as Awaited<ReturnType<typeof requestDesktopBootstrapStatus>>);
  render(<LocalAttendeePanel />);
  await screen.findByRole("alert");
  expect(fetchLocalProviderCatalog).not.toHaveBeenCalled();
  expect(saveLocalProfile).not.toHaveBeenCalled();
});

it("shows the exact Korean failure reason without provider diagnostics", async () => {
  vi.mocked(fetchLocalAttendee).mockResolvedValue({ ...admitted, phase: "failed", error_code: "room_observation_unconfirmed" });
  render(<LocalAttendeePanel />);
  await screen.findByText("AI가 전달받은 방 내용을 읽었다고 확인하지 못했어요. 방에서 에이전트 추가를 다시 해 주세요.");
  expect(screen.queryByText(/브라우저에서/)).toBeNull();
  expect(screen.queryByText("이 컴퓨터에서 실행 중이에요.")).toBeNull();
});

it("translates native bootstrap failures and hides empty provider headings", async () => {
  vi.mocked(requestDesktopBootstrapStatus).mockRejectedValueOnce("Local identity bootstrap is not complete.");
  render(<LocalAttendeePanel />);
  await screen.findByText("이 컴퓨터의 사용자 설정이 완료되지 않았어요. 앱에서 로그인한 뒤 상태를 다시 확인해 주세요.");
  expect(screen.queryByText("Local identity bootstrap is not complete.")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "상태 다시 확인" }));
  await screen.findByRole("dialog", { name: "에이전트 추가" });
  expect(screen.queryByRole("heading", { name: "API 키" })).toBeNull();
  expect(screen.queryByRole("heading", { name: "로컬 모델" })).toBeNull();
});

it("keeps a creation-time bootstrap error Korean in both the panel and modal", async () => {
  vi.mocked(createLocalAttendee).mockRejectedValueOnce(new Error("Local identity bootstrap is not complete."));
  render(<LocalAttendeePanel />);
  await chooseDraft();
  fireEvent.click(screen.getByRole("button", { name: "추가하고 실행" }));
  await screen.findByRole("button", { name: "같은 요청 다시 시도" });
  expect(screen.getAllByText("이 컴퓨터의 사용자 설정이 완료되지 않았어요. 앱에서 로그인한 뒤 상태를 다시 확인해 주세요.").length).toBeGreaterThan(0);
  expect(screen.queryByText("Local identity bootstrap is not complete.")).toBeNull();
});

it.each([false, true])("does not bootstrap when the live session is missing or rejected (%s)", async rejected => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase: "empty" } as Awaited<ReturnType<typeof requestDesktopBootstrapStatus>>);
  if (rejected) vi.mocked(bootstrapCentral).mockRejectedValue(new Error("로그인 상태가 바뀌었어요."));
  else vi.mocked(bootstrapCentral).mockResolvedValue(null);
  render(<LocalAttendeePanel />);
  fireEvent.click(await screen.findByRole("button", { name: "이 컴퓨터에서 AI를 쓸 준비하기" }));
  await screen.findByRole("alert");
  expect(saveLocalProfile).not.toHaveBeenCalled();
  expect(fetchLocalProviderCatalog).not.toHaveBeenCalled();
  expect(createLocalAttendee).not.toHaveBeenCalled();
});
