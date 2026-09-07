import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ParticipantRemovalControls from "./ParticipantRemovalControls";

afterEach(cleanup);

describe("participant removal confirmation", () => {
  it.each(["kick", "export"] as const)("confirms %s, fences duplicate submission and displays failure", async (action) => {
    let reject!: (error: Error) => void;
    const onRemove = vi.fn(() => new Promise<void>((_resolve, fail) => { reject = fail; }));
    render(<ParticipantRemovalControls participantId="guest" displayName="Guest" onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: action === "kick" ? "Guest 강퇴" : "Guest 참가 종료" }));
    expect(onRemove).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "취소" }));
    expect(screen.queryByRole("button", { name: "확인" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: action === "kick" ? "Guest 강퇴" : "Guest 참가 종료" }));
    fireEvent.click(screen.getByRole("button", { name: "확인" }));
    fireEvent.click(screen.getByRole("button", { name: "확인" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
    expect(onRemove).toHaveBeenCalledWith("guest", action);
    reject(new Error("Removal was rejected"));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toBe("Removal was rejected"));
    expect((screen.getByRole("button", { name: "확인" }) as HTMLButtonElement).disabled).toBe(false);
  });
});
