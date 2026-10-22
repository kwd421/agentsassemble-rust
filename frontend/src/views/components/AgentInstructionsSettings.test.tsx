import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  fetchAgentInstructions: vi.fn(),
  saveAgentInstructions: vi.fn(),
}));
vi.mock("../../api/agentInstructions", async (original) => ({ ...(await original<object>()), ...api }));

import AgentInstructionsSettings from "./AgentInstructionsSettings";

afterEach(() => { cleanup(); vi.clearAllMocks(); });

describe("AgentInstructionsSettings", () => {
  it("loads, edits and saves the owner's instructions for one AI", async () => {
    api.fetchAgentInstructions.mockResolvedValue("");
    api.saveAgentInstructions.mockImplementation(async (_room: string, _id: string, text: string) => text.trim());
    render(<AgentInstructionsSettings roomId="room-1" participantId="connector-a" name="아쎄이" />);
    const details = screen.getByText("지침").closest("details")!;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    const box = await screen.findByRole("textbox", { name: "아쎄이 지침" });
    expect(api.fetchAgentInstructions).toHaveBeenCalledWith("room-1", "connector-a", null);
    const save = screen.getByRole("button", { name: "저장" }) as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    fireEvent.change(box, { target: { value: "리뷰만 맡아요." } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await waitFor(() => expect(api.saveAgentInstructions).toHaveBeenCalledWith("room-1", "connector-a", "리뷰만 맡아요.", null));
    expect(await screen.findByText("지침을 저장했어요. 다음 차례부터 따라요.")).toBeTruthy();
  });

  it("refuses text over the limit", async () => {
    api.fetchAgentInstructions.mockResolvedValue("");
    render(<AgentInstructionsSettings roomId="room-1" participantId="agent-1" name="Agent" />);
    const details = screen.getByText("지침").closest("details")!;
    details.open = true;
    fireEvent(details, new Event("toggle"));
    const box = await screen.findByRole("textbox", { name: "Agent 지침" });
    fireEvent.change(box, { target: { value: "가".repeat(4001) } });
    expect((screen.getByRole("button", { name: "저장" }) as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("4000자까지 쓸 수 있어요.")).toBeTruthy();
  });
});
