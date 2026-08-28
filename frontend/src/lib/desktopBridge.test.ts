import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  fetchDesktopCentralRegistration,
  fetchDesktopHumanInviteCreate,
  fetchDesktopHumanInviteRevoke,
  requestDesktopAppearanceBoundReadTicket,
  requestDesktopAppearancePendingReadTicket,
  requestDesktopAppearanceUploadTicket,
  fetchDesktopOperatorRuntime,
  requestDesktopHumanInviteCreateTicket,
  requestDesktopHostProductSurface,
} from "./desktopBridge";

const hostCommands = [
  "host_product_surface",
  "runtime_appearance_bound_read_ticket",
  "runtime_appearance_pending_read_ticket",
  "runtime_appearance_upload_ticket",
  "runtime_central_registration_ticket",
  "runtime_human_invite_create_ticket",
  "runtime_human_invite_revoke_ticket",
  "runtime_operator_ticket",
];

const managerAuthority = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002",
  room_id: "general",
  room_uid: "30000000-0000-4000-8000-0000000000ab",
};

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

  it("rejects coercible central-registration fields before network dispatch", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);
    const canonicalGrant = {
      ticket: "a".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49154",
      server_id: "0198f492-c76a-7000-8000-000000000001",
      host_public_key_x: "A".repeat(43),
      host_key_fingerprint: "B".repeat(43),
    };

    for (const field of ["ticket", "host_public_key_x", "host_key_fingerprint"] as const) {
      const malformedGrant = {
        ...canonicalGrant,
        [field]: [canonicalGrant[field]],
      };
      const invoke = vi
        .fn()
        .mockResolvedValueOnce({
          revision: PRODUCT_SURFACE_REVISION,
          digest: "2".repeat(64),
          commands: hostCommands,
        })
        .mockResolvedValueOnce(malformedGrant);
      Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

      await requestDesktopHostProductSurface();
      await expect(
        fetchDesktopCentralRegistration({ method: "POST", body: "{}" })
      ).rejects.toThrow("권위");
    }

    expect(fetchMock).not.toHaveBeenCalled();
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
    await fetchDesktopHumanInviteCreate(managerAuthority, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"meeting_id":"general"}',
    });
    await fetchDesktopHumanInviteRevoke(managerAuthority, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: '{"meeting_id":"general","invite_id":"invite-1"}',
    });

    expect(invoke).toHaveBeenNthCalledWith(
      2,
      "runtime_human_invite_create_ticket",
      { authority: managerAuthority }
    );
    expect(invoke).toHaveBeenNthCalledWith(
      3,
      "runtime_human_invite_revoke_ticket",
      { authority: managerAuthority }
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

  it("keeps appearance upload and exact reads on separate native grants", async () => {
    const assetId = `ra_${"a".repeat(32)}`;
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "2".repeat(64),
        commands: hostCommands,
      })
      .mockResolvedValue({
        ticket: "b".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49154",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await requestDesktopHostProductSurface();
    await requestDesktopAppearanceUploadTicket(managerAuthority);
    await requestDesktopAppearancePendingReadTicket(managerAuthority, assetId);
    await requestDesktopAppearanceBoundReadTicket(managerAuthority, assetId);

    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_appearance_upload_ticket", {
      authority: managerAuthority,
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_appearance_pending_read_ticket", {
      authority: managerAuthority,
      assetId,
    });
    expect(invoke).toHaveBeenNthCalledWith(4, "runtime_appearance_bound_read_ticket", {
      authority: managerAuthority,
      assetId,
    });
  });

  it("rejects malformed appearance asset IDs before native invocation", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    for (const assetId of ["", "ra_1234", `RA_${"a".repeat(32)}`, `ra_${"g".repeat(32)}`]) {
      expect(() =>
        requestDesktopAppearancePendingReadTicket(managerAuthority, assetId)
      ).toThrow("자산 식별자");
      expect(() =>
        requestDesktopAppearanceBoundReadTicket(managerAuthority, assetId)
      ).toThrow("자산 식별자");
    }
    expect(invoke).not.toHaveBeenCalled();
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
      fetchDesktopHumanInviteCreate(managerAuthority, { method: "GET" })
    ).rejects.toThrow("POST");
    await expect(
      fetchDesktopHumanInviteRevoke(managerAuthority, { method: "" })
    ).rejects.toThrow("POST");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("runs the invite dispatch guard after grant issuance and before fetch", async () => {
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
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    const retired = new Error("retired before fetch");
    await expect(
      fetchDesktopHumanInviteCreate(
        managerAuthority,
        { method: "POST" },
        () => {
          expect(invoke).toHaveBeenCalledTimes(2);
          expect(fetchMock).not.toHaveBeenCalled();
          throw retired;
        }
      )
    ).rejects.toBe(retired);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("rejects malformed manager room authority before native invocation", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await expect(
      fetchDesktopHumanInviteCreate(
        { ...managerAuthority, room_uid: managerAuthority.room_uid.toUpperCase() },
        { method: "POST" }
      )
    ).rejects.toThrow("관리자 권위");
    await expect(
      fetchDesktopHumanInviteRevoke(
        { ...managerAuthority, room_id: " general" },
        { method: "POST" }
      )
    ).rejects.toThrow("관리자 권위");
    for (const room_id of ["..", "a/b", "a\\b", "a\nb", "\u0085room", "a".repeat(129)]) {
      await expect(
        fetchDesktopHumanInviteCreate(
          { ...managerAuthority, room_id },
          { method: "POST" }
        )
      ).rejects.toThrow("관리자 권위");
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("passes a Rust-canonical U+FEFF room ID to native unchanged", async () => {
    const exactAuthority = { ...managerAuthority, room_id: "\ufeffgeneral" };
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "2".repeat(64),
        commands: hostCommands,
      })
      .mockResolvedValueOnce({
        ticket: "d".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49154",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await requestDesktopHostProductSurface();
    await requestDesktopHumanInviteCreateTicket(exactAuthority);

    expect(invoke).toHaveBeenNthCalledWith(
      2,
      "runtime_human_invite_create_ticket",
      { authority: exactAuthority }
    );
  });

  it("dispatches concurrent operator requests only to their own grant base", async () => {
    let resolveFirst: ((value: unknown) => void) | undefined;
    let resolveSecond: ((value: unknown) => void) | undefined;
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "2".repeat(64),
        commands: hostCommands,
      })
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveFirst = resolve; })
      )
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveSecond = resolve; })
      );
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn().mockResolvedValue(new Response(null, { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    const first = fetchDesktopOperatorRuntime("/api/rooms?request=first");
    const second = fetchDesktopOperatorRuntime("/api/rooms?request=second");
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledTimes(3));
    resolveFirst?.({
      ticket: "d".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49161",
    });
    resolveSecond?.({
      ticket: "e".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49162",
    });
    await Promise.all([first, second]);

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      "http://127.0.0.1:49161/api/rooms?request=first",
      expect.objectContaining({ headers: expect.any(Headers) })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      "http://127.0.0.1:49162/api/rooms?request=second",
      expect.objectContaining({ headers: expect.any(Headers) })
    );
  });
});
