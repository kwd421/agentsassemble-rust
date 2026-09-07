import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { agentSessionFixture } from "../../../test/agentSession";
vi.mock("../ImageCropper", () => ({ default: ({ file, onCropped }: { file: File; onCropped: (file: File) => void }) =>
  <button onClick={() => onCropped(file)}>잘라서 저장</button> }));
import AgentIdentitySettings from "./AgentIdentitySettings";

afterEach(cleanup);

it("saves only identity through the server callback and exposes failed saves", async () => {
  const session = agentSessionFixture({ display_name: "Original", runtime_status: "busy" });
  const onSave = vi.fn().mockRejectedValueOnce(new Error("Save failed")).mockResolvedValue(undefined);
  const { rerender } = render(<AgentIdentitySettings session={session} onSave={onSave} />);
  fireEvent.change(screen.getByLabelText("표시 이름"), { target: { value: "New name" } });
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("Save failed"));
  expect(onSave).toHaveBeenCalledWith(session, { display_name: "New name" });
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("New name");
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  await waitFor(() => expect(screen.getByRole("status").textContent).toBe("에이전트 프로필 저장됨"));
  rerender(<AgentIdentitySettings session={{ ...session, display_name: "From server" }} onSave={onSave} />);
  expect((screen.getByLabelText("표시 이름") as HTMLInputElement).value).toBe("From server");
});

it("owns cropped upload cancellation and clears only the Agent avatar", async () => {
  const session = agentSessionFixture({ display_name: "Original", avatar_image_url: `/api/agent-avatars/aa_${"a".repeat(32)}` });
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onAvatarUpdate = vi.fn().mockImplementation(() => new Promise(() => {}));
  const { unmount } = render(<AgentIdentitySettings session={session} onSave={onSave} onAvatarUpdate={onAvatarUpdate} />);
  fireEvent.click(screen.getByRole("button", { name: "프로필 사진 삭제" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(session, { display_name: "Original", avatar_image_url: "" }));
  await waitFor(() => expect((screen.getByLabelText("에이전트 프로필 사진 선택") as HTMLInputElement).disabled).toBe(false));
  const file = new File(["image"], "avatar.png", { type: "image/png" });
  fireEvent.change(screen.getByLabelText("에이전트 프로필 사진 선택"), { target: { files: [file] } });
  fireEvent.click(screen.getByRole("button", { name: "잘라서 저장" }));
  expect(onAvatarUpdate).toHaveBeenCalledWith(session, file, "Original", expect.any(AbortSignal));
  const signal = onAvatarUpdate.mock.calls[0][3] as AbortSignal;
  expect(signal.aborted).toBe(false);
  unmount();
  expect(signal.aborted).toBe(true);
});


it("blocks oversized names before saving or uploading and counts Unicode characters", async () => {
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onAvatarUpdate = vi.fn();
  render(<AgentIdentitySettings session={agentSessionFixture()} onSave={onSave} onAvatarUpdate={onAvatarUpdate} />);
  const input = screen.getByLabelText("표시 이름");
  fireEvent.change(input, { target: { value: "😀".repeat(81) } });
  expect(screen.getByRole("alert").textContent).toContain("80");
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  expect(onSave).not.toHaveBeenCalled();
  expect((screen.getByRole("button", { name: "프로필 사진 변경" }) as HTMLButtonElement).disabled).toBe(true);
  expect(onAvatarUpdate).not.toHaveBeenCalled();
  fireEvent.change(input, { target: { value: "😀".repeat(80) } });
  fireEvent.click(screen.getByRole("button", { name: "프로필 저장" }));
  await waitFor(() => expect(onSave).toHaveBeenCalledWith(expect.anything(), { display_name: "😀".repeat(80) }));
});
