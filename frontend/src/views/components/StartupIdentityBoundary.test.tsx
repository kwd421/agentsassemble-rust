import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { centralOwnerServerUrl } from "../../lib/centralOwnerConnect";
import StartupIdentityBoundary from "./StartupIdentityBoundary";

const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(
    () => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  ),
  getOrCreateClientId: vi.fn(() => "client-1"),
}));
const boundaryMocks = vi.hoisted(() => ({ desktop: true, bundled: true, session: null as { expiresAt: string } | null }));

vi.mock("../../lib/desktopBridge", () => ({
  isDesktopWebview: () => boundaryMocks.desktop,
  isBundledDesktopWebview: () => boundaryMocks.desktop && boundaryMocks.bundled,
}));
vi.mock("../../lib/roomGuestSession", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/roomGuestSession")>()),
  loadRoomGuestSession: () => boundaryMocks.session,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: deviceMocks.getOrCreateBrowserCredential,
  getOrCreateClientId: deviceMocks.getOrCreateClientId,
}));
vi.mock("./StartupIdentityGate", () => ({
  default: () => <main aria-label="authoritative startup gate" />,
}));
vi.mock("./CentralOwnerConnectGate", () => ({
  default: ({ deviceToken }: { deviceToken: string }) => (
    <main aria-label="central owner gate" data-device-token={deviceToken} />
  ),
}));

afterEach(() => {
  cleanup();
  vi.unstubAllEnvs();
  deviceMocks.getOrCreateBrowserCredential.mockReset();
  deviceMocks.getOrCreateBrowserCredential.mockReturnValue(
    "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  );
  deviceMocks.getOrCreateClientId.mockReset();
  deviceMocks.getOrCreateClientId.mockReturnValue("client-1");
  boundaryMocks.desktop = true;
  boundaryMocks.bundled = true;
  boundaryMocks.session = null;
  window.localStorage.clear();
  window.history.replaceState({}, "", "/");
});

describe("StartupIdentityBoundary", () => {
  it("shows central login only on the configured central origin without minting local authority", async () => {
    boundaryMocks.desktop = false;
    vi.resetModules();
    vi.stubEnv("VITE_AGENTSASSEMBLE_CENTRAL_URL", window.location.origin);
    const { default: WebBoundary } = await import("./StartupIdentityBoundary");
    render(<WebBoundary>{() => <main aria-label="product" />}</WebBoundary>);
    expect(screen.getByRole("main", { name: "authoritative startup gate" })).toBeTruthy();
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
    expect(deviceMocks.getOrCreateBrowserCredential).not.toHaveBeenCalled();
  });
  it("treats central-owner navigation inside a desktop webview as remote browser admission", () => {
    const payload = {
      grantToken: `aacg1.${"a".repeat(43)}`,
      serverId: "10000000-0000-4000-8000-000000000001",
      generation: 7,
      expiresAt: Math.floor(Date.now() / 1000) + 300,
      hostPublicKeyX: "b".repeat(43),
      hostKeyFingerprint: "c".repeat(43),
    };
    const hash = new URL(
      centralOwnerServerUrl("https://room.example.test", payload)
    ).hash;
    window.history.replaceState({}, "", `/app${hash}`);

    render(
      <StartupIdentityBoundary>
        {() => <main aria-label="product" />}
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "central owner gate" })).toBeTruthy();
    expect(
      screen.getByRole("main", { name: "central owner gate" }).dataset.deviceToken
    ).toBe("aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    expect(screen.queryByRole("main", { name: "authoritative startup gate" })).toBeNull();
    expect(window.location.hash).toBe("");
  });

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

  it("restores a remote room session after navigation loses its one-use fragment", () => {
    boundaryMocks.bundled = false;
    boundaryMocks.session = { expiresAt: new Date(Date.now() + 60_000).toISOString() };
    window.history.replaceState({}, "", "/app");

    render(
      <StartupIdentityBoundary>
        {() => <main aria-label="product" />}
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    expect(screen.queryByRole("main", { name: "authoritative startup gate" })).toBeNull();
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
      "/recover?recover=1&room=friend-room#recovery=aagr1.AbCdEfGhIjKlMnOpQrStUvWxYz0123456789_-abcdEfA",
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
