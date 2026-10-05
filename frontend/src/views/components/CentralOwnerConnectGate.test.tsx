import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import { TEST_SERVER_PRODUCT_SURFACE } from "../../test/serverProductSurface";
import CentralOwnerConnectGate from "./CentralOwnerConnectGate";

const mocks = vi.hoisted(() => ({
  verify: vi.fn(),
  rooms: vi.fn(), exchange: vi.fn(),
}));

vi.mock("../../lib/central/ownerWorkspace", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/central/ownerWorkspace")>()),
  exchangeCentralOwnerSession: mocks.exchange,
  fetchCentralOwnerRooms: mocks.rooms,
}));

vi.mock("../../lib/central/ownerConnect", () => ({ verifyCentralOwnerHost: mocks.verify }));

afterEach(() => {
  cleanup();
  mocks.verify.mockReset();
  mocks.rooms.mockReset();
  mocks.exchange.mockReset();
  sessionStorage.clear();
});

it("retries a failed challenge with the in-memory unexpired grant", async () => {
  mocks.verify.mockRejectedValueOnce(new Error("네트워크 연결 실패"));
  mocks.verify.mockResolvedValueOnce(undefined);
  mocks.rooms.mockResolvedValueOnce({
    server_id: "10000000-0000-4000-8000-000000000001",
    authority_lineage_id: "20000000-0000-4000-8000-000000000002",
    server_product_surface: TEST_SERVER_PRODUCT_SURFACE, rooms: [],
  });
  const connect = {
    grantToken: `aacg1.${"a".repeat(43)}`,
    serverId: "10000000-0000-4000-8000-000000000001",
    generation: 1,
    expiresAt: Math.floor(Date.now() / 1000) + 300,
    hostPublicKeyX: "b".repeat(43),
    hostKeyFingerprint: "c".repeat(43),
  };

  const { grantToken: _grantToken, expiresAt: _expiresAt, ...binding } = connect;
  const session = { ...binding, sessionToken: `aaos1.${"d".repeat(43)}`, sessionId: "30000000-0000-4000-8000-000000000003" };
  mocks.exchange.mockResolvedValue(session);
  const onComplete = vi.fn();
  const view = render(<CentralOwnerConnectGate connect={connect} deviceToken="device-1" onComplete={onComplete} />);
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("네트워크 연결 실패"));
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(onComplete).toHaveBeenCalledOnce());
  expect(sessionStorage.getItem("agentsassemble.central-owner-workspace.v2")).toBeNull();
  expect(mocks.verify).toHaveBeenCalledTimes(2);
  expect(mocks.rooms).toHaveBeenCalledOnce();
  expect(mocks.rooms).toHaveBeenCalledWith(session, "device-1");
  view.unmount();
  render(<CentralOwnerConnectGate connect={{ ...connect, expiresAt: Math.floor(Date.now() / 1000) - 1 }} deviceToken="device-1" onComplete={onComplete} />);
  expect(screen.getByRole("alert").textContent).toContain("접속이 만료");
  expect(screen.queryByRole("button", { name: "다시 시도" })).toBeNull();
  expect(mocks.verify).toHaveBeenCalledTimes(2);
  expect(mocks.rooms).toHaveBeenCalledOnce();
  expect(onComplete).toHaveBeenCalledOnce();
});
