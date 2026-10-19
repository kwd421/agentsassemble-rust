import { StrictMode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import MemberJoinPanel from "./MemberJoinPanel";

const central = vi.hoisted(() => ({
  session: null as null | { token: string; person: { display_name: string } },
  pending: "", create: vi.fn(), recover: vi.fn(), open: vi.fn(),
}));
vi.mock("../../lib/central/identity", () => ({
  loadCentralSession: () => central.session,
  loadPendingCentralRecoveryCode: () => central.pending,
  clearPendingCentralRecoveryCode: () => { central.pending = ""; },
  createCentralGuest: central.create, recoverCentralGuest: central.recover,
}));
vi.mock("../../lib/central/secureMemberEntry", () => ({
  previewSecureMember: async (entry: { inviteToken: string }) => {
    if (entry.inviteToken !== "memory-only-invite") throw new Error("lost invitation");
    return { label: "Invited server", endpoint_origin: "https://invited.example", endpoint_generation: 1,
      server_id: "invited-server", target: { server_id: "invited-server" } };
  },
  SecureMemberAdmission: { open: central.open },
}));
const entry = { server_id: "invited-server", registration_epoch: "current-epoch", inviteToken: "memory-only-invite" };
function authenticated() {
  central.session = { token: "central-session", person: { display_name: "Guest identity" } };
  central.pending = "NEW-RECOVERY-CODE";
  return { recovery_code: central.pending };
}
beforeEach(() => {
  central.session = null; central.pending = ""; vi.clearAllMocks();
  localStorage.clear(); sessionStorage.clear();
  central.create.mockImplementation(authenticated); central.recover.mockImplementation(authenticated);
});
afterEach(cleanup);

it.each(["new", "recover"])("%s guest stays in the invitation and requires code acknowledgement before consent", async (path) => {
  render(<StrictMode><MemberJoinPanel secureEntry={entry} /></StrictMode>);
  fireEvent.click(await screen.findByRole("button", { name: path === "new" ? "새 게스트로 계속" : "이미 복구 코드가 있어요" }));
  fireEvent.change(screen.getByLabelText(path === "new" ? "표시 이름" : "게스트 복구 코드"), { target: { value: path === "new" ? "Guest identity" : "OLD-CODE" } });
  fireEvent.click(screen.getByRole("button", { name: path === "new" ? "게스트 만들기" : "같은 게스트로 로그인" }));
  const next = await screen.findByRole("button", { name: "계속" });
  expect((next as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "참가하기" })).toBeNull();
  expect(central.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("checkbox")); fireEvent.click(next);
  await screen.findByRole("button", { name: "참가하기" });
  expect(screen.getByText("Guest identity 계정으로 참가")).toBeTruthy();
  expect(screen.queryByText("https://invited.example")).toBeNull();
  expect(screen.queryByRole("button", { name: "목록에 다시 표시하고 참가" })).toBeNull();
  expect(central.pending).toBe("");
  expect(central.open).not.toHaveBeenCalled();
  expect(JSON.stringify([localStorage, sessionStorage])).not.toContain(entry.inviteToken);
});

it("failed recovery preserves input and invitation for explicit retry without admission", async () => {
  central.recover.mockRejectedValueOnce(new Error("invalid proof"));
  render(<MemberJoinPanel secureEntry={entry} />);
  fireEvent.click(await screen.findByRole("button", { name: "이미 복구 코드가 있어요" }));
  fireEvent.change(screen.getByLabelText("게스트 복구 코드"), { target: { value: "OLD-CODE" } });
  fireEvent.click(screen.getByRole("button", { name: "같은 게스트로 로그인" }));
  await screen.findByRole("alert");
  expect((screen.getByLabelText("게스트 복구 코드") as HTMLInputElement).value).toBe("OLD-CODE");
  expect(central.session).toBeNull(); expect(central.open).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "같은 게스트로 로그인" }));
  await screen.findByRole("checkbox");
  expect(screen.queryByRole("button", { name: "참가하기" })).toBeNull();
});

it("an interrupted code acknowledgement cannot be skipped by an existing session", async () => {
  authenticated();
  render(<MemberJoinPanel secureEntry={entry} />);
  const next = await screen.findByRole("button", { name: "계속" });
  expect((next as HTMLButtonElement).disabled).toBe(true);
  expect(screen.queryByRole("button", { name: "참가하기" })).toBeNull();
  fireEvent.click(screen.getByRole("checkbox")); fireEvent.click(next);
  await screen.findByRole("button", { name: "참가하기" });
  expect(central.open).not.toHaveBeenCalled();
});
