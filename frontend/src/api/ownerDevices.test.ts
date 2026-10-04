import { afterEach, expect, it, vi } from "vitest";
import { listOwnerDevices } from "./ownerDevices";
import { fetchJsonServerOperator } from "./http";

vi.mock("./http", () => ({
  fetchJsonServerOperator: vi.fn(), postJsonServerOperator: vi.fn(),
  responseError: vi.fn(), serverOwnerSessionHeaders: vi.fn(),
}));
afterEach(() => vi.clearAllMocks());

it("keeps the native computer name within the existing UTF-16 host-name contract", async () => {
  const deviceName = "가".repeat(80);
  vi.mocked(fetchJsonServerOperator).mockResolvedValue({ sessions: [{
    session_id: "host", device_name: deviceName, browser: "AgentsAssemble 앱", os: "macos",
    last_connected_at: 1791100000, current: true, connected: true, revocable: false, kind: "host",
  }] });
  expect((await listOwnerDevices({})).sessions[0].device_name).toBe(deviceName);
});

it("rejects an oversized remote device description without hiding it as an unknown device", async () => {
  vi.mocked(fetchJsonServerOperator).mockResolvedValue({ sessions: [{
    session_id: "10000000-0000-4000-8000-000000000001", device_name: "가".repeat(61), browser: "Firefox", os: "Linux",
    last_connected_at: 1791100000, current: false, connected: null, revocable: true, kind: "pairing",
  }] });
  await expect(listOwnerDevices({})).rejects.toThrow("기기 설명이 올바르지");
});
