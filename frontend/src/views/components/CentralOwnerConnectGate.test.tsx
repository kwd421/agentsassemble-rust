import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";

import CentralOwnerConnectGate from "./CentralOwnerConnectGate";

const mocks = vi.hoisted(() => ({
  verify: vi.fn(),
  rooms: vi.fn(),
}));

vi.mock("../../lib/centralOwnerConnect", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/centralOwnerConnect")>()),
  verifyCentralOwnerHost: mocks.verify,
  fetchCentralOwnerRooms: mocks.rooms,
}));

afterEach(() => {
  cleanup();
  mocks.verify.mockReset();
  mocks.rooms.mockReset();
});

it("retries a failed challenge with the in-memory unexpired grant", async () => {
  mocks.verify.mockRejectedValueOnce(new Error("네트워크 연결 실패"));
  mocks.verify.mockResolvedValueOnce(undefined);
  mocks.rooms.mockResolvedValueOnce([
    { roomId: "room-1", roomUid: "uid-1", label: "내 방", topic: "", status: "active" },
  ]);
  const connect = {
    grantToken: `aacg1.${"a".repeat(43)}`,
    serverId: "server-1",
    generation: 1,
    expiresAt: Math.floor(Date.now() / 1000) + 300,
    hostPublicKeyX: "b".repeat(43),
    hostKeyFingerprint: "c".repeat(43),
  };

  render(<CentralOwnerConnectGate connect={connect} deviceToken="device-1" onComplete={vi.fn()} />);
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("네트워크 연결 실패"));
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(screen.getByRole("button", { name: /내 방/ })).toBeTruthy());
  expect(mocks.verify).toHaveBeenCalledTimes(2);
  expect(mocks.rooms).toHaveBeenCalledOnce();
  expect(mocks.rooms).toHaveBeenCalledWith(connect, "device-1");
});
