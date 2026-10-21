import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AgentHandoffTrustSetting from "./AgentHandoffTrustSetting";
import type { RoomAgentSession } from "../../api";

const api = vi.hoisted(() => ({ fetchAgentHandoffTrust: vi.fn(), setAgentHandoffTrust: vi.fn() }));
vi.mock("../../api/agentHandoffTrust", () => api);
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const session = { room_id: "room", session_id: "agent", permission_mode: "full_access" } as RoomAgentSession;

it("starts off and turns on only after the host confirms", async () => {
  api.fetchAgentHandoffTrust.mockResolvedValue(false);
  api.setAgentHandoffTrust.mockResolvedValue(true);
  render(<AgentHandoffTrustSetting session={session} />);
  const toggle = screen.getByRole("switch", { name: "다른 사람 AI가 넘긴 일도 묻지 않기" });
  expect(toggle.hasAttribute("disabled")).toBe(true);
  await waitFor(() => expect(toggle.hasAttribute("disabled")).toBe(false));
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  expect(api.setAgentHandoffTrust).toHaveBeenCalledWith("room", "agent", true);
});

it("keeps the switch off and says so when the host cannot be read", async () => {
  api.fetchAgentHandoffTrust.mockRejectedValue(new Error("offline"));
  render(<AgentHandoffTrustSetting session={session} />);
  expect(await screen.findByRole("status")).toBeTruthy();
  expect(screen.getByRole("switch").hasAttribute("disabled")).toBe(true);
});
