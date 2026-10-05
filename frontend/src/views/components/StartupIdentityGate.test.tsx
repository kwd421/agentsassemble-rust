import { CentralTemporaryError } from "../../lib/central/connectionError";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { TEST_SERVER_PRODUCT_SURFACE } from "../../test/serverProductSurface";
import { PRODUCT_SURFACE_REVISION } from "../../types/generated/PRODUCT_SURFACE_REVISION";
import StartupIdentityGate from "./StartupIdentityGate";

const centralMocks = vi.hoisted(() => ({
  configured: false,
  loggedOut: false,
  login: vi.fn(),
  openServer: vi.fn(),
  session: null as null | { person: { display_name: string } },
  bootstrap: vi.fn(),
  register: vi.fn(),
  create: vi.fn(),
  recover: vi.fn(),
  pending: "",
}));
const desktopMocks = vi.hoisted(() => ({
  fetchOperatorRuntime: vi.fn(),
  initializeBootstrap: vi.fn(),
  requestBootstrapStatus: vi.fn(),
  requestHostProductSurface: vi.fn(),
  requestHostDeviceInfo: vi.fn().mockResolvedValue({ server_id: null, host_name: "Test Mac", host_os: "macos" }),
}));
const SERVER_ID = "30000000-0000-4000-8000-000000000001";
const LINEAGE_ID = "30000000-0000-4000-8000-000000000002";
const SERVER_SURFACE = TEST_SERVER_PRODUCT_SURFACE;
const directory = (authority_lineage_id = LINEAGE_ID) => ({
  server_id: SERVER_ID,
  authority_lineage_id,
  server_product_surface: SERVER_SURFACE,
  profile_revision: 2,
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
const completedBootstrap = {
  phase: "complete",
  authority_lineage_id: LINEAGE_ID,
  server_id: SERVER_ID,
  server_product_surface_revision: SERVER_SURFACE.revision,
  server_product_surface_digest: SERVER_SURFACE.digest,
  profile: desktopProfile,
  deduplicated: false,
};

vi.mock("../../lib/desktopBridge", () => ({
  fetchDesktopOperatorRuntime: desktopMocks.fetchOperatorRuntime,
  initializeDesktopBootstrap: desktopMocks.initializeBootstrap,
  requestDesktopBootstrapStatus: desktopMocks.requestBootstrapStatus,
  requestDesktopHostProductSurface: desktopMocks.requestHostProductSurface,
  requestDesktopHostDeviceInfo: desktopMocks.requestHostDeviceInfo,
}));
vi.mock("../../lib/deviceIdentity", () => ({
  rememberGuestProfile: vi.fn(),
}));
vi.mock("../../lib/central/identity", () => ({
  CENTRAL_SESSION_CLEARED_EVENT: "agentsassemble:central-session-cleared",
  centralIdentityConfigured: () => centralMocks.configured,
  isCentralWebEntry: () => false,
  centralSessionLoggedOut: () => centralMocks.loggedOut,
  bootstrapCentral: centralMocks.bootstrap,
  clearPendingCentralRecoveryCode: vi.fn(),
  createCentralGuest: centralMocks.create,
  isCentralAuthenticationError: () => false,
  loadCentralSession: () => centralMocks.session,
  loadPendingCentralRecoveryCode: () => centralMocks.pending,
  logoutCentral: vi.fn(),
  loginCentralGoogle: centralMocks.login,
  openCentralOwnedServer: centralMocks.openServer,
  recoverCentralGuest: centralMocks.recover,
  registerLocalServer: centralMocks.register,
}));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  centralMocks.configured = false;
  centralMocks.loggedOut = false;
  centralMocks.session = null;
  centralMocks.pending = "";
  vi.clearAllMocks();
  desktopMocks.requestHostProductSurface.mockResolvedValue({
    revision: PRODUCT_SURFACE_REVISION,
    digest: "1".repeat(64),
    commands: ["host_product_surface"],
  });
});

describe("StartupIdentityGate", () => {
  it("requires central login even when a saved local profile is complete", async () => {
    centralMocks.configured = true;
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    await screen.findByRole("button", { name: "Google로 계속" });
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("opens this device with a saved account during a central network outage", async () => {
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Cached name" } };
    centralMocks.bootstrap.mockRejectedValue(new CentralTemporaryError("Failed to fetch"));
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    await userEvent.click(await screen.findByRole("button", { name: /이 기기 서버 열기/ }));
    await vi.waitFor(() => expect(onComplete).toHaveBeenCalledOnce());
    expect(centralMocks.register).not.toHaveBeenCalled();
    expect(desktopMocks.initializeBootstrap).not.toHaveBeenCalled();
  });

  it("keeps a completed local profile out of the app when central validation fails", async () => {
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Google name" } };
    centralMocks.bootstrap.mockRejectedValue(new Error("central session was revoked"));
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    expect((await screen.findByRole("alert")).textContent).toContain("central session was revoked");
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("does not enter with a cached profile if the central session disappears during validation", async () => {
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Cached Google name" } };
    centralMocks.bootstrap.mockResolvedValue(null);
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    await screen.findByRole("button", { name: "Google로 계속" });
    expect(onComplete).not.toHaveBeenCalled();
  });

  it("opens the saved local rooms only after explicit hosting with an edited profile", async () => {
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Google account name" } };
    centralMocks.bootstrap.mockResolvedValue({ person: { person_id: "per_fixture", identity_kind: "google", display_name: "Google account name", avatar_url: null }, servers: [], server_time: 1 });
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    const host = await screen.findByRole("button", { name: /이 기기 서버 열기/ });
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();
    await userEvent.click(host);
    await vi.waitFor(() => expect(onComplete).toHaveBeenCalledOnce());
    expect(centralMocks.register).toHaveBeenCalledWith("device-1");
    expect(desktopMocks.initializeBootstrap).not.toHaveBeenCalled();
  });

  it("opens the selected remote server even when local identity inspection fails", async () => {
    const remoteServer = {
      server_id: "30000000-0000-4000-8000-000000000099",
      relation: "owner" as const,
      alias: "Mac의 방",
      host_public_key_jwk: {
        kty: "OKP",
        crv: "Ed25519",
        x: "A".repeat(43),
      },
      host_key_fingerprint: "B".repeat(43),
      endpoint: {
        origin: "https://mac-room.example.test",
        generation: 7,
        lease_expires_at: Math.floor(Date.now() / 1000) + 600,
        status: "likely_online" as const,
      },
    };
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Google account name" } };
    centralMocks.bootstrap.mockResolvedValue({
      person: {
        person_id: "per_fixture",
        identity_kind: "google",
        display_name: "Google account name",
        avatar_url: null,
      },
      servers: [remoteServer],
      server_time: 1,
    });
    desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
    desktopMocks.fetchOperatorRuntime.mockResolvedValue(Response.json(directory()));
    const onComplete = vi.fn();

    desktopMocks.requestHostDeviceInfo.mockRejectedValueOnce(new Error("local database is unreadable"));
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);

    const remoteButton = await screen.findByRole("button", { name: "Mac의 방 서버 열기" });
    expect(screen.queryByRole("button", { name: /이 기기/ })).toBeNull();
    expect(screen.getByRole("alert").textContent).toContain("local database is unreadable");
    expect(onComplete).not.toHaveBeenCalled();
    expect(desktopMocks.fetchOperatorRuntime).not.toHaveBeenCalled();
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();

    await userEvent.click(remoteButton);

    await vi.waitFor(() => expect(centralMocks.openServer).toHaveBeenCalledWith(remoteServer));
    expect(onComplete).not.toHaveBeenCalled();
    expect(desktopMocks.fetchOperatorRuntime).not.toHaveBeenCalled();
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();
  });

  it.each(["google", "guest", "recover", "pending"])("%s entry reaches the chooser without a room runtime", async (entry) => {
    centralMocks.configured = true;
    const person = { display_name: "Account" };
    centralMocks.bootstrap.mockResolvedValue({ person, servers: [], server_time: 1 });
    centralMocks.login.mockResolvedValue({ person });
    centralMocks.create.mockResolvedValue({ person, recovery_code: "NEW-CODE" });
    centralMocks.recover.mockResolvedValue({ person, recovery_code: "NEW-CODE" });
    if (entry === "pending") centralMocks.pending = "NEW-CODE";
    render(<StartupIdentityGate deviceToken="device-1" onComplete={vi.fn()} />);
    if (entry === "google") {
      await userEvent.click(await screen.findByRole("button", { name: "Google로 계속" }));
    } else {
      if (entry === "guest") {
        await userEvent.click(await screen.findByRole("button", { name: "새 게스트로 계속" }));
        await userEvent.type(screen.getByRole("textbox", { name: "표시 이름" }), "Guest");
        await userEvent.click(screen.getByRole("button", { name: "게스트 만들기" }));
      } else if (entry === "recover") {
        await userEvent.click(await screen.findByRole("button", { name: "이미 복구 코드가 있습니다" }));
        await userEvent.type(screen.getByRole("textbox", { name: "게스트 복구 코드" }), "OLD-CODE");
        await userEvent.click(screen.getByRole("button", { name: "같은 게스트로 로그인" }));
      }
      await userEvent.click(await screen.findByRole("checkbox"));
      await userEvent.click(screen.getByRole("button", { name: "계속" }));
    }
    await screen.findByText(/등록된 서버가 없습니다/);
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(desktopMocks.initializeBootstrap).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();
  });

  it("retains offline hosts and remote errors without local fallback, and allows refresh", async () => {
    centralMocks.configured = true;
    centralMocks.session = { person: { display_name: "Account" } };
    const remote = { server_id: "remote", relation: "owner", alias: "Main", endpoint: null };
    const account = { person: centralMocks.session.person, servers: [remote], server_time: 1 };
    centralMocks.bootstrap.mockResolvedValue(account);
    render(<StartupIdentityGate deviceToken="device-1" onComplete={vi.fn()} />);
    const offline = await screen.findByRole("button", { name: "Main 서버 열기" });
    expect((offline as HTMLButtonElement).disabled).toBe(true);
    const online = { ...remote, endpoint: { status: "likely_online", lease_expires_at: Date.now() / 1000 + 600 } };
    centralMocks.bootstrap.mockResolvedValue({ ...account, servers: [online] });
    await userEvent.click(screen.getByRole("button", { name: "서버 목록 새로고침" }));
    centralMocks.openServer.mockRejectedValueOnce(new Error("host unavailable"));
    await userEvent.click(await screen.findByRole("button", { name: "Main 서버 열기" }));
    expect((await screen.findByRole("alert")).textContent).toContain("host unavailable");
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();
  });

  it("does not reopen completed local authority after explicit central logout", async () => {
    centralMocks.configured = true;
    centralMocks.loggedOut = true;
    desktopMocks.requestBootstrapStatus.mockResolvedValue({ phase: "complete", profile: desktopProfile });
    const onComplete = vi.fn();
    render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
    await screen.findByRole("button", { name: "Google로 계속" });
    expect(onComplete).not.toHaveBeenCalled();
    expect(desktopMocks.fetchOperatorRuntime).not.toHaveBeenCalled();
    expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
    expect(centralMocks.register).not.toHaveBeenCalled();
  });

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

  it("keeps the native cause when a desktop command rejects with its error string", async () => {
    desktopMocks.requestHostProductSurface.mockRejectedValue(
      "runtime tickets are available only to the bundled desktop UI"
    );

    render(<StartupIdentityGate deviceToken="device-1" onComplete={vi.fn()} />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("로컬 신원 권위를 확인하지 못했습니다.");
    expect(alert.textContent).toContain(
      "runtime tickets are available only to the bundled desktop UI"
    );
  });
});

it("requires login after authentication rejection even with an existing local host", async () => {
  centralMocks.configured = true;
  centralMocks.session = { person: { display_name: "Cached name" } };
  centralMocks.bootstrap.mockImplementation(async () => {
    centralMocks.session = null;
    throw new Error("중앙 로그인이 만료됐습니다. 다시 로그인해 주세요.");
  });
  desktopMocks.requestBootstrapStatus.mockResolvedValue(completedBootstrap);
  const onComplete = vi.fn();
  render(<StartupIdentityGate deviceToken="device-1" onComplete={onComplete} />);
  await screen.findByRole("button", { name: "Google로 계속" });
  expect(screen.queryByRole("button", { name: /이 기기 서버 열기/ })).toBeNull();
  expect(desktopMocks.requestBootstrapStatus).not.toHaveBeenCalled();
  expect(onComplete).not.toHaveBeenCalled();
});
