import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import AgentHandoffTrustSetting from "./AgentHandoffTrustSetting";
import type { RoomAgentSession } from "../../api";
import { OwnerRouteHost, RoomPeople } from "../../app/ownerRouteHost";

const api = vi.hoisted(() => ({ fetchAgentTrustedOwners: vi.fn(), setAgentTrustedOwners: vi.fn() }));
vi.mock("../../api/agentHandoffTrust", () => api);
afterEach(() => { cleanup(); vi.resetAllMocks(); });

const session = { room_id: "room", session_id: "agent", permission_mode: "full_access" } as RoomAgentSession;
const people = [{ id: "human-bob", name: "Bob" }, { id: "human-eve", name: "Eve" }];

function renderTrust(host: string | null = null) {
  return render(
    <OwnerRouteHost.Provider value={host}>
      <RoomPeople.Provider value={people}><AgentHandoffTrustSetting session={session} /></RoomPeople.Provider>
    </OwnerRouteHost.Provider>
  );
}

it("trusts only the people the owner turns on, after the host confirms", async () => {
  api.fetchAgentTrustedOwners.mockResolvedValue([]);
  api.setAgentTrustedOwners.mockResolvedValue(["human-bob"]);
  renderTrust();
  const bob = screen.getByRole("switch", { name: "Bob의 AI가 넘긴 일은 묻지 않기" });
  await waitFor(() => expect(bob.hasAttribute("disabled")).toBe(false));
  fireEvent.click(bob);
  await waitFor(() => expect(bob.getAttribute("aria-checked")).toBe("true"));
  expect(api.setAgentTrustedOwners).toHaveBeenCalledWith("room", "agent", ["human-bob"], null);
  expect(screen.getByRole("switch", { name: "Eve의 AI가 넘긴 일은 묻지 않기" }).getAttribute("aria-checked")).toBe("false");
});

it("talks to the room's own server, never another owner workspace", async () => {
  api.fetchAgentTrustedOwners.mockResolvedValue([]);
  renderTrust("server-b");
  await waitFor(() => expect(api.fetchAgentTrustedOwners).toHaveBeenCalledWith("room", "agent", "server-b"));
});

it("keeps every switch off and says so when the host cannot be read", async () => {
  api.fetchAgentTrustedOwners.mockRejectedValue(new Error("offline"));
  renderTrust();
  expect(await screen.findByRole("status")).toBeTruthy();
  expect(screen.getByRole("switch", { name: "Bob의 AI가 넘긴 일은 묻지 않기" }).hasAttribute("disabled")).toBe(true);
});
