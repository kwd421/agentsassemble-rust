import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../../test/agentSession";
vi.mock("../ImageCropper", () => ({ default: ({ file, onCropped }: { file: File; onCropped: (file: File) => void }) =>
  <button type="button" onClick={() => onCropped(file)}>사진 선택 완료</button> }));
import AgentProfileCard from "./AgentProfileCard";

afterEach(cleanup);
beforeAll(() => {
  HTMLDialogElement.prototype.showModal ??= function showModal(this: HTMLDialogElement) { this.setAttribute("open", ""); };
  HTMLDialogElement.prototype.close ??= function close(this: HTMLDialogElement) { this.removeAttribute("open"); };
});
afterAll(() => vi.restoreAllMocks());

it("draws the galaxy background only for a maximum-reasoning agent's profile", () => {
  const { container, rerender } = render(
    <AgentProfileCard session={agentSessionFixture({ reasoning_effort: "ultra" })}>
      {null}
    </AgentProfileCard>
  );
  expect(container.querySelector("header")?.getAttribute("data-galaxy")).toBe("true");

  rerender(
    <AgentProfileCard session={agentSessionFixture({ reasoning_effort: "high" })}>
      {null}
    </AgentProfileCard>
  );
  expect(container.querySelector("header")?.hasAttribute("data-galaxy")).toBe(false);
});

it("edits the name in place: Enter saves, Escape restores and a failure keeps the draft", async () => {
  const session = agentSessionFixture({ display_name: "Original" });
  const onSave = vi.fn().mockRejectedValueOnce(new Error("Save failed")).mockResolvedValue(undefined);
  const { rerender } = render(<AgentProfileCard session={session} onSave={onSave}><p>runtime</p></AgentProfileCard>);
  expect(screen.queryByRole("button", { name: "프로필 편집" })).toBeNull();
  expect(screen.getByText("runtime")).toBeTruthy();
  const input = screen.getByLabelText("표시 이름") as HTMLInputElement;
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value: "Throwaway" } });
  fireEvent.keyDown(input, { key: "Escape" });
  fireEvent.blur(input);
  expect(input.value).toBe("Original");
  expect(onSave).not.toHaveBeenCalled();
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value: "  New name " } });
  fireEvent.blur(input);
  await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("이름을 저장하지 못했어요. 다시 시도해 주세요."));
  expect(onSave).toHaveBeenCalledWith(expect.objectContaining({ session_id: session.session_id }), { display_name: "New name" });
  expect(input.value).toBe("  New name ");
  fireEvent.focus(input);
  fireEvent.blur(input);
  await waitFor(() => expect(onSave).toHaveBeenCalledTimes(2));
  rerender(<AgentProfileCard session={{ ...session, display_name: "New name" }} onSave={onSave}><p>runtime</p></AgentProfileCard>);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("New name");
});

it("sends an unchanged or invalid name nowhere", () => {
  const onSave = vi.fn();
  render(<AgentProfileCard session={agentSessionFixture({ display_name: "Same" })} onSave={onSave}>{null}</AgentProfileCard>);
  const input = screen.getByLabelText("표시 이름") as HTMLInputElement;
  fireEvent.focus(input);
  fireEvent.blur(input);
  fireEvent.focus(input);
  fireEvent.change(input, { target: { value: "😀".repeat(81) } });
  expect(screen.getByRole("alert").textContent).toContain("80");
  fireEvent.blur(input);
  expect(onSave).not.toHaveBeenCalled();
  expect(input.value).toBe("Same");
});

it("goes from the photo straight to the picker and crop, saving the photo on apply", async () => {
  const session = agentSessionFixture({ display_name: "Original" });
  const onAvatarUpdate = vi.fn().mockResolvedValue(undefined);
  render(<AgentProfileCard session={session} onSave={vi.fn()} onAvatarUpdate={onAvatarUpdate}>{null}</AgentProfileCard>);
  const picker = screen.getByLabelText("에이전트 프로필 사진 선택") as HTMLInputElement;
  const click = vi.spyOn(picker, "click");
  fireEvent.click(screen.getByRole("button", { name: "프로필 사진 변경" }));
  expect(click).toHaveBeenCalled();
  const file = new File(["image"], "avatar.png", { type: "image/png" });
  fireEvent.change(picker, { target: { files: [file] } });
  fireEvent.click(screen.getByRole("button", { name: "사진 선택 완료" }));
  await waitFor(() => expect(onAvatarUpdate).toHaveBeenCalledWith(session, file, "Original", expect.any(AbortSignal)));
  await waitFor(() => expect(screen.queryByRole("button", { name: "사진 선택 완료" })).toBeNull());
});

it("offers change or removal for an existing photo and removes it with one save", async () => {
  const session = agentSessionFixture({ display_name: "Original", avatar_image_url: `/api/agent-avatars/aa_${"a".repeat(32)}` });
  const onSave = vi.fn().mockResolvedValue(undefined);
  render(<AgentProfileCard session={session} avatarImage="blob:current-avatar" onSave={onSave} onAvatarUpdate={vi.fn()}>{null}</AgentProfileCard>);
  fireEvent.click(screen.getByRole("button", { name: "프로필 사진 변경" }));
  expect(screen.getByRole("menuitem", { name: "사진 바꾸기" })).toBeTruthy();
  fireEvent.click(screen.getByRole("menuitem", { name: "사진 제거" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(session, { display_name: "Original", avatar_image_url: "" }));
});

it("cancels an owned photo upload on unmount", async () => {
  const onAvatarUpdate = vi.fn().mockImplementation(() => new Promise(() => {}));
  const { unmount } = render(<AgentProfileCard session={agentSessionFixture()} onSave={vi.fn()} onAvatarUpdate={onAvatarUpdate}>{null}</AgentProfileCard>);
  fireEvent.change(screen.getByLabelText("에이전트 프로필 사진 선택"), { target: { files: [new File(["x"], "a.png", { type: "image/png" })] } });
  fireEvent.click(screen.getByRole("button", { name: "사진 선택 완료" }));
  await waitFor(() => expect(onAvatarUpdate).toHaveBeenCalled());
  const signal = onAvatarUpdate.mock.calls[0][3] as AbortSignal;
  expect(signal.aborted).toBe(false);
  unmount();
  expect(signal.aborted).toBe(true);
});

it("shows a plain heading when the viewer may not edit", () => {
  render(<AgentProfileCard session={agentSessionFixture({ display_name: "Viewer only" })}>{null}</AgentProfileCard>);
  expect(screen.getByRole("heading", { name: "Viewer only" })).toBeTruthy();
  expect(screen.queryByLabelText("표시 이름")).toBeNull();
  expect(screen.queryByRole("button", { name: "프로필 사진 변경" })).toBeNull();
});
