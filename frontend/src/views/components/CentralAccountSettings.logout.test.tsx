import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

beforeEach(() => {
  cleanup();
  localStorage.clear();
  sessionStorage.clear();
  vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", "https://central.example.test");
});

afterEach(() => {
  cleanup();
  localStorage.clear();
  sessionStorage.clear();
  vi.unstubAllGlobals();
  vi.unstubAllEnvs();
});

// Quota-blocked session -> logout through the real owner: the revoke stays
// unsent (zero central requests), the local session is cleared and the
// sign-in state returns with no governor line on screen.
it("logs out locally without any central request while the daily quota block is active", async () => {
  vi.resetModules();
  const { noteCentralDailyQuotaExhausted, resetCentralRequestGovernorForTests } =
    await import("../../lib/central/centralRequestGovernor");
  resetCentralRequestGovernorForTests();
  const fetcher = vi.fn(async () => Response.json({ ok: true }));
  vi.stubGlobal("fetch", fetcher);
  const { saveMemberAnchor, loadMemberAnchors } = await import("../../lib/central/memberAnchors");
  saveMemberAnchor("fixture-person", {
    server_id: "server-a", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
    endpoint_origin: "https://host.test", endpoint_generation: 3,
    rooms: [{ room_id: "general", name: "일반" }], hidden: false, updated_at: 1,
  });
  localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
    token: "fixture-session", expires_at: 9_999_999_999, device_id: "fixture-device",
    person: { person_id: "fixture-person", identity_kind: "google", display_name: "Name" },
  }));
  noteCentralDailyQuotaExhausted("fixture-person");
  const { default: CentralAccountSettings } = await import("./CentralAccountSettings");
  const { loadCentralSession, centralSessionLoggedOut } = await import("../../lib/central/identity");
  await act(async () => { render(<CentralAccountSettings disabled={false} />); });
  expect(loadCentralSession()?.token).toBe("fixture-session");
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "로그아웃" }));
  await act(async () => { await Promise.resolve(); });
  // Zero central requests while blocked; logout still completed locally.
  expect(fetcher).not.toHaveBeenCalled();
  expect(loadCentralSession()).toBeNull();
  expect(centralSessionLoggedOut()).toBe(true);
  expect(loadMemberAnchors("fixture-person")).toEqual([]);
  // Sign-in state: the logout control is gone and no error line is shown.
  expect(screen.queryByRole("button", { name: "로그아웃" })).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  resetCentralRequestGovernorForTests();
});
