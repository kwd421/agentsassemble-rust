import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ParticipantRemovalControls from "./ParticipantRemovalControls";

afterEach(cleanup);

describe("participant removal confirmation", () => {
  it.each(["kick", "export"] as const)("confirms %s, fences duplicate submission and displays failure", async (action) => {
    let reject!: (error: Error) => void;
    const onRemove = vi.fn(() => new Promise<void>((_resolve, fail) => { reject = fail; }));
    render(<ParticipantRemovalControls participantId="guest" displayName="Guest" onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: action === "kick" ? "Guest 내보내기" : "Guest 영구 퇴장" }));
    expect(onRemove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(screen.queryByRole("button", { name: action === "kick" ? "내보내기" : "영구 퇴장" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: action === "kick" ? "Guest 내보내기" : "Guest 영구 퇴장" }));
    fireEvent.click(screen.getByRole("button", { name: action === "kick" ? "내보내기" : "영구 퇴장" }));
    fireEvent.click(screen.getByRole("button", { name: "처리 중…" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
    expect(onRemove).toHaveBeenCalledWith("guest", action);
    reject(new Error("Removal was rejected"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Removal was rejected"));
    expect((screen.getByRole("button", { name: action === "kick" ? "내보내기" : "영구 퇴장" }) as HTMLButtonElement).disabled).toBe(false);
  });
});
