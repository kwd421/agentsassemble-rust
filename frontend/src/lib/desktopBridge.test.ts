import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  fetchDesktopCentralRegistration,
  fetchDesktopHumanInviteCreate,
  fetchDesktopHumanInviteRevoke,
  requestDesktopHostProductSurface,
} from "./desktopBridge";

const hostCommands = [
  "host_product_surface",
  "runtime_central_registration_ticket",
  "runtime_human_invite_create_ticket",
  "runtime_human_invite_revoke_ticket",
];

describe("desktop exact-purpose HTTP bridge", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("binds its purpose ticket to the exact registration endpoint", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "2".repeat(64),
        commands: hostCommands,
      })
      .mockResolvedValueOnce({
        ticket: "a".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49154",
        server_id: "0198f492-c76a-7000-8000-000000000001",
        host_public_key_x: "A".repeat(43),
        host_key_fingerprint: "B".repeat(43),
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    await fetchDesktopCentralRegistration({
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{}",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "host_product_surface");
    expect(invoke).toHaveBeenNthCalledWith(
      2,
      "runtime_central_registration_ticket"
    );
    expect(fetchMock).toHaveBeenCalledWith(
      "http://127.0.0.1:49154/api/central-directory/registration-proof",
      expect.objectContaining({ method: "POST", headers: expect.any(Headers) })
    );
    const headers = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    expect(headers.get("Authorization")).toBe(`Bearer ${"a".repeat(64)}`);
    expect(headers.get("Content-Type")).toBe("application/json");
  });

  it("keeps invite create and revoke on separate native grants and fixed routes", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "2".repeat(64),
        commands: hostCommands,
      })
      .mockResolvedValueOnce({
        ticket: "b".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49154",
      })
      .mockResolvedValueOnce({
        ticket: "c".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49154",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    await fetchDesktopHumanInviteCreate("general", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"meeting_id":"general"}',
    });
    await fetchDesktopHumanInviteRevoke("general", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"meeting_id":"general","invite_id":"invite-1"}',
    });

    expect(invoke).toHaveBeenNthCalledWith(
      2,
      "runtime_human_invite_create_ticket",
      { roomId: "general" }
    );
    expect(invoke).toHaveBeenNthCalledWith(
      3,
      "runtime_human_invite_revoke_ticket",
      { roomId: "general" }
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49154/api/room-invite/create",
      expect.objectContaining({ method: "POST", cache: "no-store" })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://127.0.0.1:49154/api/room-invite/revoke",
      expect.objectContaining({ method: "POST", cache: "no-store" })
    );
    const createHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const revokeHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(createHeaders.get("Authorization")).toBe(`Bearer ${"b".repeat(64)}`);
    expect(revokeHeaders.get("Authorization")).toBe(`Bearer ${"c".repeat(64)}`);
  });

  it("rejects a non-POST method before requesting native authority", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await expect(
      fetchDesktopCentralRegistration({ method: "GET" })
    ).rejects.toThrow("POST");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("rejects an invite non-POST method before requesting native authority", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await expect(
      fetchDesktopHumanInviteCreate("general", { method: "GET" })
    ).rejects.toThrow("POST");
    expect(invoke).not.toHaveBeenCalled();
  });
});
