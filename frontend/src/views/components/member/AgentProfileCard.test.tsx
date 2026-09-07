import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../../test/agentSession";
vi.mock("../ImageCropper", () => ({ default: ({ file, onCropped }: { file: File; onCropped: (file: File) => void }) =>
  <button type="button" onClick={() => onCropped(file)}>사진 선택 완료</button> }));
import AgentProfileCard from "./AgentProfileCard";

afterEach(cleanup);
beforeAll(() => {
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: () => "blob:profile-preview" });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: vi.fn() });
});
afterAll(() => {
  Reflect.deleteProperty(URL, "createObjectURL");
  Reflect.deleteProperty(URL, "revokeObjectURL");
});

it("keeps editing with identity, preserves failed drafts and views only canonical state", async () => {
  const session = agentSessionFixture({ display_name: "Original", runtime_status: "busy" });
  const onSave = vi.fn().mockRejectedValueOnce(new Error("Save failed")).mockResolvedValue(undefined);
  const runtimeDraft = <input aria-label="Runtime draft" defaultValue="Original setting" />;
  const { rerender } = render(<AgentProfileCard session={session} onSave={onSave}>{runtimeDraft}</AgentProfileCard>);
  fireEvent.change(screen.getByLabelText("Runtime draft"), { target: { value: "Unsaved setting" } });
  expect(screen.queryByLabelText("표시 이름")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  expect(screen.getByLabelText("Runtime draft").closest("[hidden]")).toBeTruthy();
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "New name" } });
  rerender(<AgentProfileCard session={{ ...session, display_name: "Canonical change" }} onSave={onSave}>{runtimeDraft}</AgentProfileCard>);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("New name");
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Save failed"));
  expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ session_id: session.session_id }), { display_name: "New name" });
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("프로필을 변경했어요."));
  expect(screen.getByRole("heading", { name: "Canonical change" })).toBeTruthy();
  expect((screen.getByLabelText("Runtime draft") as HTMLInputElement).value).toBe("Unsaved setting");
  rerender(<AgentProfileCard session={{ ...session, display_name: "New name" }} onSave={onSave}>{runtimeDraft}</AgentProfileCard>);
  expect(screen.getByRole("heading", { name: "New name" })).toBeTruthy();
});

it("cancels local photo removal and commits an explicit clear only on save", async () => {
  const session = agentSessionFixture({ display_name: "Original", avatar_image_url: `/api/agent-avatars/aa_${"a".repeat(32)}` });
  const onSave = vi.fn().mockResolvedValue(undefined);
  render(<AgentProfileCard session={session} avatarImage="blob:current-avatar" onSave={onSave}>{null}</AgentProfileCard>);
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  fireEvent.click(screen.getByLabelText("사진 옵션"));
  fireEvent.click(screen.getByRole("button", { name: "사진 제거" }));
  fireEvent.click(screen.getByRole("button", { name: "취소" }));
  expect(onSave).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  fireEvent.click(screen.getByLabelText("사진 옵션"));
  fireEvent.click(screen.getByRole("button", { name: "사진 제거" }));
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(session, { display_name: "Original", avatar_image_url: "" }));
});

it("stages cropping until one profile save and cancels owned upload on unmount", () => {
  const session = agentSessionFixture({ display_name: "Original" });
  const onSave = vi.fn();
  const onAvatarUpdate = vi.fn().mockImplementation(() => new Promise(() => {}));
  const { unmount } = render(<AgentProfileCard session={session} onSave={onSave} onAvatarUpdate={onAvatarUpdate}>{null}</AgentProfileCard>);
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  const file = new File(["image"], "avatar.png", { type: "image/png" });
  fireEvent.change(screen.getByLabelText("에이전트 프로필 사진 선택"), { target: { files: [file] } });
  fireEvent.click(screen.getByRole("button", { name: "사진 선택 완료" }));
  expect(onAvatarUpdate).not.toHaveBeenCalled();
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "With photo" } });
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  expect(onAvatarUpdate).toHaveBeenCalledWith(session, file, "With photo", expect.any(AbortSignal));
  expect(onSave).not.toHaveBeenCalled();
  const signal = onAvatarUpdate.mock.calls[0][3] as AbortSignal;
  expect(signal.aborted).toBe(false);
  unmount();
  expect(signal.aborted).toBe(true);
});

it("blocks oversized Unicode names before saving or uploading", async () => {
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onAvatarUpdate = vi.fn();
  render(<AgentProfileCard session={agentSessionFixture()} onSave={onSave} onAvatarUpdate={onAvatarUpdate}>{null}</AgentProfileCard>);
  fireEvent.click(screen.getByRole("button", { name: "프로필 편집" }));
  const input = screen.getByLabelText("표시 이름");
  fireEvent.change(input, { target: { value: "😀".repeat(81) } });
  expect(screen.getByRole("alert").textContent).toContain("80");
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  expect(onSave).not.toHaveBeenCalled();
  expect(onAvatarUpdate).not.toHaveBeenCalled();
  fireEvent.change(input, { target: { value: "😀".repeat(80) } });
  fireEvent.click(screen.getByRole("button", { name: "변경사항 저장" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.anything(), { display_name: "😀".repeat(80) }));
});
