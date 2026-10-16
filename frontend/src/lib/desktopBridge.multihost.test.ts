import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  chooseDesktopWorkspace,
  requestDesktopHostProductSurface,
  requestDesktopMessagePinsReadTicket,
} from "./desktopBridge";
import {
  closeRemoteWorkspace,
  installRemoteWorkspace,
} from "./remote/remoteWorkspace";
import type { RemoteTransport } from "./remote/remoteTransport";

const DIGEST = "9".repeat(64);
const HOST_COMMANDS = [
  "choose_local_workspace",
  "host_product_surface",
  "runtime_message_pins_read_ticket",
];

function ticketGrant() {
  return {
    ticket: "d".repeat(64),
    ttl_seconds: 30,
    http_base_url: "http://127.0.0.1:49154",
  };
}

function installFriendHost() {
  const callbacks = new Set<() => void>();
  const transport = {
    hello: { origin: "https://friend-host.test" },
    active: true,
    fetch: vi.fn(),
    onClose: (fn: () => void) => {
      callbacks.add(fn);
      return () => {
        callbacks.delete(fn);
      };
    },
    close: () => {
      transport.active = false;
      for (const fn of callbacks) fn();
    },
  };
  installRemoteWorkspace({
    transport: transport as unknown as RemoteTransport,
    owner: null,
    member: {
      sessionToken: "friend-member-token",
      serverSurface: { server_id: "friend-host" },
    } as never,
    deviceToken: "friend-device",
    clientId: "friend-client",
  });
}

describe("desktop host commands with a friend host live", () => {
  afterEach(() => {
    closeRemoteWorkspace();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("allows a host command for a local-room action while a friend host is live", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: DIGEST,
        commands: HOST_COMMANDS,
      })
      .mockResolvedValue(ticketGrant());
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    await requestDesktopHostProductSurface();

    installFriendHost();

    await expect(
      requestDesktopMessagePinsReadTicket("general", { kind: "local" })
    ).resolves.toMatchObject({ ttl_seconds: 30 });
    expect(invoke).toHaveBeenCalledWith("runtime_message_pins_read_ticket", {
      roomId: "general",
    });
  });

  it("allows a host command with no room target while a friend host is live", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: DIGEST,
        commands: HOST_COMMANDS,
      })
      .mockResolvedValueOnce({ selected: true, path: "/tmp/own-workspace" });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    await requestDesktopHostProductSurface();

    installFriendHost();

    await expect(chooseDesktopWorkspace()).resolves.toEqual({
      selected: true,
      path: "/tmp/own-workspace",
    });
    expect(invoke).toHaveBeenCalledWith("choose_local_workspace");
  });

  it("refuses a host command for a remote-room action while a friend host is live", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: DIGEST,
        commands: HOST_COMMANDS,
      })
      .mockResolvedValue(ticketGrant());
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    await requestDesktopHostProductSurface();

    installFriendHost();
    const callsBefore = invoke.mock.calls.length;

    await expect(
      requestDesktopMessagePinsReadTicket("general", { kind: "remote" })
    ).rejects.toThrow("원격 서버에서는 이 컴퓨터의 호스트 권한을 사용할 수 없어요.");
    expect(invoke.mock.calls.length).toBe(callsBefore);
  });
});
