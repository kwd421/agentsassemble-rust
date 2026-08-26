import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import StartupIdentityBoundary from "./StartupIdentityBoundary";

const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(
    () => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  ),
}));

vi.mock("../../lib/desktopBridge", () => ({
  isDesktopWebview: () => true,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: deviceMocks.getOrCreateBrowserCredential,
  hasStartupIdentitySelection: () => true,
  loadRememberedGuestProfile: () => ({ displayName: "Remembered" }),
}));
vi.mock("./StartupIdentityGate", () => ({
  default: () => <main aria-label="authoritative startup gate" />,
}));

afterEach(() => {
  cleanup();
  deviceMocks.getOrCreateBrowserCredential.mockReset();
  deviceMocks.getOrCreateBrowserCredential.mockReturnValue(
    "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  );
  window.history.replaceState({}, "", "/");
});

describe("StartupIdentityBoundary", () => {
  it("never lets legacy invite or guest routes bypass desktop bootstrap", () => {
    window.history.replaceState({}, "", "/join?guest=1#invite=legacy");

    render(
      <StartupIdentityBoundary>
        <main aria-label="product" />
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "authoritative startup gate" })).toBeTruthy();
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
  });

  it("shows a hard stop instead of rendering identity-bound surfaces without durable custody", () => {
    deviceMocks.getOrCreateBrowserCredential.mockImplementation(() => {
      throw new Error("브라우저 저장소를 사용할 수 없습니다.");
    });

    render(
      <StartupIdentityBoundary>
        <main aria-label="product" />
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("저장소");
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
  });
});
