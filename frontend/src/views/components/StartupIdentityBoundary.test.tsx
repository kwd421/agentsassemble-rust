import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import StartupIdentityBoundary from "./StartupIdentityBoundary";

vi.mock("../../lib/desktopBridge", () => ({
  isDesktopWebview: () => true,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateDeviceToken: () => "device-test",
  hasStartupIdentitySelection: () => true,
  loadRememberedGuestProfile: () => ({ displayName: "Remembered" }),
}));
vi.mock("./StartupIdentityGate", () => ({
  default: () => <main aria-label="authoritative startup gate" />,
}));

afterEach(() => {
  cleanup();
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
});
