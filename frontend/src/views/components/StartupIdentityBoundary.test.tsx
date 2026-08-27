import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import StartupIdentityBoundary from "./StartupIdentityBoundary";

const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(
    () => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  ),
  getOrCreateClientId: vi.fn(() => "client-1"),
}));
const boundaryMocks = vi.hoisted(() => ({ desktop: true }));

vi.mock("../../lib/desktopBridge", () => ({
  isDesktopWebview: () => boundaryMocks.desktop,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: deviceMocks.getOrCreateBrowserCredential,
  getOrCreateClientId: deviceMocks.getOrCreateClientId,
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
  deviceMocks.getOrCreateClientId.mockReset();
  deviceMocks.getOrCreateClientId.mockReturnValue("client-1");
  boundaryMocks.desktop = true;
  window.localStorage.clear();
  window.history.replaceState({}, "", "/");
});

describe("StartupIdentityBoundary", () => {
  it("never lets a browser entrance bypass desktop bootstrap", () => {
    window.history.replaceState({}, "", "/join?token=invite-token");

    render(
      <StartupIdentityBoundary>
        {() => <main aria-label="product" />}
      </StartupIdentityBoundary>
    );

    expect(
      screen.getByRole("main", { name: "authoritative startup gate" })
    ).toBeTruthy();
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
  });

  it("keeps direct non-desktop startup unavailable without inventing profile authority", () => {
    boundaryMocks.desktop = false;

    render(
      <StartupIdentityBoundary>
        {() => <main aria-label="product" />}
      </StartupIdentityBoundary>
    );

    expect(
      screen.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })
    ).toBeTruthy();
    expect(
      screen.queryByRole("main", { name: "authoritative startup gate" })
    ).toBeNull();
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
    expect(deviceMocks.getOrCreateBrowserCredential).not.toHaveBeenCalled();
    expect(deviceMocks.getOrCreateClientId).not.toHaveBeenCalled();
  });

  it.each([
    ["invite", "/join?token=invite-token"],
    ["pairing", "/pair?token=aap1_pairing-token"],
    [
      "recovery",
      "/?recover=1&room=friend-room#recovery=ABCD-EFGH-IJKL-MNOP-QRST-UVWX-YZ23-4567",
    ],
  ])("retains the authorized %s browser entrance", (_kind, url) => {
    boundaryMocks.desktop = false;
    window.history.replaceState({}, "", url);

    render(
      <StartupIdentityBoundary>
        {({ deviceToken, clientId }) => (
          <main
            aria-label="product"
            data-device-token={deviceToken}
            data-client-id={clientId}
          />
        )}
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    expect(
      screen.queryByRole("main", { name: "브라우저 직접 시작 사용 불가" })
    ).toBeNull();
    expect(deviceMocks.getOrCreateBrowserCredential).toHaveBeenCalledOnce();
    expect(deviceMocks.getOrCreateClientId).toHaveBeenCalledOnce();
    expect(screen.getByRole("main", { name: "product" }).dataset.deviceToken).toBe(
      "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
    );
    expect(screen.getByRole("main", { name: "product" }).dataset.clientId).toBe(
      "client-1"
    );
  });

  it("retains a one-use browser entrance until durable client-id custody succeeds", () => {
    boundaryMocks.desktop = false;
    window.history.replaceState({}, "", "/join?token=invite-token");
    deviceMocks.getOrCreateClientId.mockImplementation(() => {
      throw new Error("입장 요청 식별자를 영구 저장할 수 없습니다.");
    });
    const renderProduct = vi.fn(() => <main aria-label="product" />);

    render(<StartupIdentityBoundary>{renderProduct}</StartupIdentityBoundary>);

    expect(screen.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeTruthy();
    expect(renderProduct).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?token=invite-token");
  });

  it("retains a one-use browser entrance until durable credential custody succeeds", () => {
    boundaryMocks.desktop = false;
    window.history.replaceState({}, "", "/pair?token=aap1_pairing-token");
    deviceMocks.getOrCreateBrowserCredential.mockImplementation(() => {
      throw new Error("브라우저 저장소를 사용할 수 없습니다.");
    });
    const renderProduct = vi.fn(() => <main aria-label="product" />);

    render(
      <StartupIdentityBoundary>{renderProduct}</StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeTruthy();
    expect(renderProduct).not.toHaveBeenCalled();
    expect(window.location.search).toBe("?token=aap1_pairing-token");
  });

  it("shows a hard stop instead of rendering identity-bound surfaces without durable custody", () => {
    deviceMocks.getOrCreateBrowserCredential.mockImplementation(() => {
      throw new Error("브라우저 저장소를 사용할 수 없습니다.");
    });

    render(
      <StartupIdentityBoundary>
        {() => <main aria-label="product" />}
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "브라우저 신원 사용 불가" })).toBeTruthy();
    expect(screen.getByRole("alert").textContent).toContain("저장소");
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
  });
});
