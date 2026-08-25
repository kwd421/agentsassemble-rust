import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  fetchDesktopCentralRegistration,
  requestDesktopHostProductSurface,
} from "./desktopBridge";

describe("desktop central registration bridge", () => {
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
        commands: [
          "host_product_surface",
          "runtime_central_registration_ticket",
        ],
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

  it("rejects a non-POST method before requesting native authority", async () => {
    const invoke = vi.fn();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await expect(
      fetchDesktopCentralRegistration({ method: "GET" })
    ).rejects.toThrow("POST");
    expect(invoke).not.toHaveBeenCalled();
  });
});
