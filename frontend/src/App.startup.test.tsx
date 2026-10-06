import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import App from "./App";

const state = vi.hoisted(() => ({ controller: {} as Record<string, unknown> }));
vi.mock("./app/useAppController", () => ({ useAppController: () => state.controller }));
vi.mock("./app/AppView", () => ({ default: () => <main>연결된 방</main> }));
vi.mock("./views/components/StartupIdentityGate", () => ({ default: () => <main>로그인</main> }));
vi.mock("./views/components/FrontendUpdateNotice", () => ({ default: () => null }));
afterEach(cleanup);

it.each(["guestSession", "ownerProfileSession"])("keeps %s connected when display directory needs login", (session) => {
  state.controller = { centralDirectory: { status: "authentication-required" },
    canonicalRoom: { connectionState: "connected" }, [session]: {} };
  render(<App deviceToken="" clientId="test" />);
  expect(screen.getByText("연결된 방")).toBeTruthy();
});
it("still requires login when no admitted session exists", () => {
  state.controller = { centralDirectory: { status: "authentication-required" } };
  render(<App deviceToken="" clientId="test" />);
  expect(screen.getByText("로그인")).toBeTruthy();
});
