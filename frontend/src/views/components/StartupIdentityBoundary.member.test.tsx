import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import StartupIdentityBoundary from "./StartupIdentityBoundary";
import { closeRemoteWorkspace, installRemoteWorkspace } from "../../lib/remote/remoteWorkspace";

const deviceMocks = vi.hoisted(() => ({
  getOrCreateBrowserCredential: vi.fn(() => "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
  getOrCreateClientId: vi.fn(() => "client-1"),
}));
const boundaryMocks = vi.hoisted(() => ({ desktop: false, bundled: false }));

vi.mock("../../lib/desktopBridge", () => ({
  isDesktopWebview: () => boundaryMocks.desktop,
  isBundledDesktopWebview: () => boundaryMocks.desktop && boundaryMocks.bundled,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  getOrCreateBrowserCredential: deviceMocks.getOrCreateBrowserCredential,
  getOrCreateClientId: deviceMocks.getOrCreateClientId,
}));
vi.mock("./StartupIdentityGate", () => ({
  default: () => <main aria-label="authoritative startup gate" />,
}));
vi.mock("./CentralOwnerConnectGate", () => ({
  default: () => <main aria-label="central owner gate" />,
}));

function fakeMemberTransport() {
  const listeners = new Set<() => void>();
  const transport = {
    hello: { origin: "https://hihi.test", server_id: "hihi-server", channel_id: "chan-1" },
    active: true,
    onClose: (listener: () => void) => {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    },
    close: () => undefined,
    die: () => {
      transport.active = false;
      for (const listener of [...listeners]) listener();
    },
  };
  return transport;
}

describe("member-only workspace install", () => {
  beforeEach(() => {
    closeRemoteWorkspace();
    window.localStorage.clear();
    window.sessionStorage.clear();
    window.history.replaceState({}, "", "/join");
  });
  afterEach(() => {
    cleanup();
    closeRemoteWorkspace();
    window.history.replaceState({}, "", "/");
  });

  it("keeps the app content mounted and surfaces the end overlay as a sibling", () => {
    let mounts = 0;
    function Product({ deviceToken }: { deviceToken: string }) {
      const [mount] = useState(() => {
        mounts += 1;
        return mounts;
      });
      return <main aria-label="product" data-mount={mount} data-device-token={deviceToken} />;
    }
    render(
      <StartupIdentityBoundary
        memberReturn={{
          record: {
            invite_token: "invite", meeting_id: "room", server_id: "server", registration_epoch: "epoch",
            challenge_id: "challenge", challenge_hash: "a".repeat(43), handoff_state: "b".repeat(43), expires_at: 9_999_999_999,
          },
        }}
      >
        {(identity) => <Product deviceToken={identity.deviceToken} />}
      </StartupIdentityBoundary>
    );
    expect(screen.getByRole("main", { name: "product" })).toBeTruthy();
    expect(mounts).toBe(1);

    const transport = fakeMemberTransport();
    act(() => {
      installRemoteWorkspace({
        transport: transport as never,
        owner: null,
        member: { sessionToken: "member-token", serverSurface: { server_id: "hihi-server" } } as never,
        deviceToken: "member-device",
        clientId: "member-client",
      });
    });
    // Installing the member host must not remount the app: selection,
    // admission and sockets survive the install.
    const content = screen.getByRole("main", { name: "product" });
    expect(mounts).toBe(1);
    expect(content.dataset.deviceToken).toBe("member-device");
    expect(screen.queryByRole("dialog", { name: "서버 연결 종료" })).toBeNull();

    // A dead member channel surfaces its end-state overlay without unmounting.
    act(() => { transport.die(); });
    expect(screen.getByRole("main", { name: "product" })).toBe(content);
    expect(mounts).toBe(1);
    expect(screen.getByRole("dialog", { name: "서버 연결 종료" })).toBeTruthy();
  });
});
