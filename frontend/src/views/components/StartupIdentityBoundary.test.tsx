import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { centralOwnerServerUrl } from "../../lib/central/ownerConnect";
import type { CentralOwnerWorkspace } from "../../lib/central/ownerWorkspace";
import { useContext } from "react";
import { CentralOwnerWorkspaceContext } from "../../lib/central/ownerWorkspaceContext";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";
import StartupIdentityBoundary from "./StartupIdentityBoundary";

const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(
    () => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  ),
  getOrCreateClientId: vi.fn(() => "client-1"),
}));
const boundaryMocks = vi.hoisted(() => ({ desktop: true, bundled: true, session: null as { expiresAt: string | null; centralOwner?: boolean } | null }));

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
  default: ({ deviceToken, connect, onComplete }: { deviceToken: string; connect: { serverId: string; generation: number; hostPublicKeyX: string; hostKeyFingerprint: string }; onComplete: (session: CentralOwnerWorkspace) => void }) => (
    <main aria-label="central owner gate" data-device-token={deviceToken}>
      <button onClick={() => onComplete({ serverId: connect.serverId, generation: connect.generation, hostPublicKeyX: connect.hostPublicKeyX, hostKeyFingerprint: connect.hostKeyFingerprint, sessionToken: `aaos1.${"d".repeat(43)}`, sessionId: "30000000-0000-4000-8000-000000000003" })}>complete owner entry</button>
    </main>
  ),
}));

afterEach(() => {
  cleanup();
  vi.useRealTimers();
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
  window.sessionStorage.clear();
  window.history.replaceState({}, "", "/");
});

describe("StartupIdentityBoundary", () => {
  it("lets a consumed member callback enter the existing product without a token in the URL", () => {
    boundaryMocks.desktop = false;
    window.history.replaceState({}, "", "/join");
    render(<StartupIdentityBoundary memberReturn={{ record: {
      invite_token: "invite", meeting_id: "room", server_id: "server", registration_epoch: "epoch",
      challenge_id: "challenge", challenge_hash: "a".repeat(43), handoff_state: "b".repeat(43), expires_at: 9_999_999_999,
    } }}>{() => <main aria-label="product" />}</StartupIdentityBoundary>);
    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    expect(deviceMocks.getOrCreateBrowserCredential).toHaveBeenCalled();
  });

  it("shows missing callback custody without exposing anonymous admission", () => {
    boundaryMocks.desktop = false;
    render(<StartupIdentityBoundary memberReturn={{ error: "입장 기록이 없어요." }}>{() => <main aria-label="product" />}</StartupIdentityBoundary>);
    expect(screen.getByRole("alert").textContent).toContain("입장 기록이 없어요");
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
    expect(screen.queryByRole("textbox", { name: "이름" })).toBeNull();
  });
  it("requires fresh central entry even when previous root and room credentials were stored", () => {
    boundaryMocks.desktop = false;
    boundaryMocks.session = { centralOwner: true, expiresAt: null };
    sessionStorage.setItem("agentsassemble.central-owner-workspace.v2", JSON.stringify({ sessionToken: "previous-root" }));
    render(<StartupIdentityBoundary>{() => <main aria-label="product" />}</StartupIdentityBoundary>);
    expect(screen.getByRole("main", { name: "브라우저 직접 시작 사용 불가" })).toBeTruthy();
    expect(screen.queryByRole("main", { name: "product" })).toBeNull();
    expect(sessionStorage.getItem("agentsassemble.central-owner-workspace.v2")).toBeNull();
    expect(deviceMocks.getOrCreateBrowserCredential).not.toHaveBeenCalled();
  });
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
    vi.useFakeTimers();
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

    let publish!: (status: CentralOwnerSessionStatus) => void;
    function Product() { publish = useContext(CentralOwnerWorkspaceContext)!.onStatus; return <main aria-label="product" />; }
    render(
      <StartupIdentityBoundary>
        {() => <Product />}
      </StartupIdentityBoundary>
    );

    expect(screen.getByRole("main", { name: "central owner gate" })).toBeTruthy();
    expect(
      screen.getByRole("main", { name: "central owner gate" }).dataset.deviceToken
    ).toBe("aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    expect(screen.queryByRole("main", { name: "authoritative startup gate" })).toBeNull();
    expect(window.location.hash).toBe("");
    fireEvent.click(screen.getByRole("button", { name: "complete owner entry" }));
    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    for (let elapsed = 20; elapsed <= 320; elapsed += 20) {
      act(() => {
        vi.advanceTimersByTime(20_000);
        publish({ state: "active" });
      });
    }
    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    expect(screen.queryByRole("main", { name: "central owner gate" })).toBeNull();
    expect(screen.queryByRole("main", { name: "authoritative startup gate" })).toBeNull();
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
