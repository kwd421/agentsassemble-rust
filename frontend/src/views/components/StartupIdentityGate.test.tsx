import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../../types/generated/PRODUCT_SURFACE_REVISION";
import StartupIdentityGate from "./StartupIdentityGate";

const centralMocks = vi.hoisted(() => ({
  configured: false,
  login: vi.fn(),
}));
const desktopMocks = vi.hoisted(() => ({
  fetchOperatorRuntime: vi.fn(),
  initializeBootstrap: vi.fn(),
  requestBootstrapStatus: vi.fn(),
  requestHostProductSurface: vi.fn(),
}));
const SERVER_ID = "30000000-0000-4000-8000-000000000001";
const LINEAGE_ID = "30000000-0000-4000-8000-000000000002";
const SERVER_SURFACE = {
  revision: PRODUCT_SURFACE_REVISION,
  digest: "fa9f15121b05ce4e2687d6f36187aa333c1c2f387c2fb42047aad91d5097ada6",
  http_routes: [],
  websocket_streams: ["room_events"],
  websocket_actions: [
    "agent.configure",
    "agent.create",
    "agent.resume",
    "agent.start",
    "agent.stop",
    "message.send",
    "participant.leave",
    "participant.mute",
    "participant.role.update",
    "room.random.choose",
    "room.random.roll",
    "room.settings.update",
  ],
};
const directory = (authority_lineage_id = LINEAGE_ID) => ({
  server_id: SERVER_ID,
  authority_lineage_id,
  server_product_surface: SERVER_SURFACE,
  rooms: [],
});
const desktopProfile = {
  revision: 2,
  display_name: "Desktop User",
  handle: "desktopuser.",
  status: "online",
  custom_status: "AgentsAssemble",
  avatar_label: "DE",
  avatar_image_url: "",
  banner_preset: "default",
  accent_color: "#5865f2",
  mic_muted: true,
  deafened: false,
  created_at: "2026-08-25T00:00:00.000000000Z",
  updated_at: "2026-08-25T00:00:00.000000000Z",
};

vi.mock("../../lib/desktopBridge", () => ({
  fetchDesktopOperatorRuntime: desktopMocks.fetchOperatorRuntime,
  initializeDesktopBootstrap: desktopMocks.initializeBootstrap,
  requestDesktopBootstrapStatus: desktopMocks.requestBootstrapStatus,
  requestDesktopHostProductSurface: desktopMocks.requestHostProductSurface,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  rememberGuestProfile: vi.fn(),
}));
vi.mock("../../lib/centralIdentity", () => ({
  centralIdentityConfigured: () => centralMocks.configured,
  bootstrapCentral: vi.fn(),
  clearPendingCentralRecoveryCode: vi.fn(),
  createCentralGuest: vi.fn(),
  isCentralAuthenticationError: () => false,
  loadCentralSession: () => null,
  loadPendingCentralRecoveryCode: () => "",
  loginCentralGoogle: centralMocks.login,
  recoverCentralGuest: vi.fn(),
  registerLocalServer: vi.fn(),
}));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  centralMocks.configured = false;
  vi.clearAllMocks();
  desktopMocks.requestHostProductSurface.mockResolvedValue({
    revision: PRODUCT_SURFACE_REVISION,
    digest: "1".repeat(64),
    commands: ["host_product_surface"],
  });
});

describe("StartupIdentityGate", () => {
  it("initializes desktop authority before fetching the real empty room directory", async () => {
    desktopMocks.requestBootstrapStatus.mockResolvedValue({
      phase: "empty",
      authority_lineage_id: LINEAGE_ID,
      server_id: SERVER_ID,
      server_product_surface_revision: SERVER_SURFACE.revision,
      server_product_surface_digest: SERVER_SURFACE.digest,
      profile: null,
      deduplicated: false,
    });
    desktopMocks.initializeBootstrap.mockResolvedValue({
      phase: "complete",
      authority_lineage_id: LINEAGE_ID,
      server_id: SERVER_ID,
      server_product_surface_revision: SERVER_SURFACE.revision,
      server_product_surface_digest: SERVER_SURFACE.digest,
      profile: desktopProfile,
      deduplicated: false,
    });
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(
      new Response(JSON.stringify(directory()), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    const onComplete = vi.fn();

    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    await userEvent.type(
      await screen.findByRole("textbox", { name: "게스트 표시 이름" }),
      "Desktop User"
    );
    await userEvent.click(screen.getByRole("button", { name: "게스트로 계속" }));

    await vi.waitFor(() => expect(onComplete).toHaveBeenCalledOnce());
    expect(desktopMocks.initializeBootstrap).toHaveBeenCalledWith(
      expect.any(String),
      "Desktop User"
    );
    expect(desktopMocks.fetchOperatorRuntime).toHaveBeenCalledWith("/api/rooms", {
      cache: "no-store",
    });
  });

  it.each([
    ["missing", {}],
    [
      "detached",
      directory("30000000-0000-4000-8000-000000000099"),
    ],
  ])("rejects a %s zero-room response", async (_case, payload) => {
    desktopMocks.requestBootstrapStatus.mockResolvedValue({
      phase: "complete",
      authority_lineage_id: LINEAGE_ID,
      server_id: SERVER_ID,
      server_product_surface_revision: SERVER_SURFACE.revision,
      server_product_surface_digest: SERVER_SURFACE.digest,
      profile: desktopProfile,
      deduplicated: false,
    });
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(
      new Response(JSON.stringify(payload), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    const onComplete = vi.fn();

    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);

    await screen.findByRole("alert");
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("lets the user cancel a central Google handoff that is still pending", async () => {
    centralMocks.configured = true;
    desktopMocks.requestBootstrapStatus.mockResolvedValue({
      phase: "empty",
      authority_lineage_id: LINEAGE_ID,
      server_id: SERVER_ID,
      server_product_surface_revision: SERVER_SURFACE.revision,
      server_product_surface_digest: SERVER_SURFACE.digest,
      profile: null,
      deduplicated: false,
    });
    centralMocks.login.mockImplementation(
      (_status: (message: string) => void, signal: AbortSignal) =>
        new Promise((_resolve, reject) => {
          signal.addEventListener(
            "abort",
            () => reject(new DOMException("aborted", "AbortError")),
            { once: true }
          );
        })
    );

    render(<StartupIdentityGate deviceToken="device-1" onComplete={vi.fn()} />);

    await userEvent.click(
      await screen.findByRole("button", { name: "Google로 계속" })
    );
    await userEvent.click(
      await screen.findByRole("button", { name: "Google 로그인 취소" })
    );

    await vi.waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "Google 로그인 취소" })
      ).toBeNull();
    });
    expect(screen.getByRole("alert").textContent).toContain("취소");
  });
});
