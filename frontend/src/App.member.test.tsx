import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";

const state = vi.hoisted(() => ({ guestJoinToken: "invite" }));
vi.mock("./app/useAppController", () => ({ useAppController: () => ({
  guestJoinToken: state.guestJoinToken, centralDirectory: { status: "authentication-required" },
  refreshCentralDirectory: vi.fn(), canonicalRoom: { connectionState: "disconnected" },
}) }));
vi.mock("./app/AppView", () => ({ default: () => <p>초대 입장 오류와 재시도</p> }));
vi.mock("./views/components/StartupIdentityGate", () => ({ default: () => <p>서버 시작 로그인</p> }));
vi.mock("./views/components/FrontendUpdateNotice", () => ({ default: () => null }));
vi.mock("./lib/desktopBridge", () => ({ isDesktopWebview: () => true }));
afterEach(cleanup);

it("retains the invite retry screen after the central client clears a rejected session", () => {
  state.guestJoinToken = "invite";
  render(<App deviceToken="device" clientId="client" />);
  expect(screen.getByText("초대 입장 오류와 재시도")).toBeTruthy();
  expect(screen.queryByText("서버 시작 로그인")).toBeNull();
});
it("preserves the existing startup login outside invite admission", () => {
  state.guestJoinToken = "";
  render(<App deviceToken="device" clientId="client" />);
  expect(screen.getByText("서버 시작 로그인")).toBeTruthy();
});
