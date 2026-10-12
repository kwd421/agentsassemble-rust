import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  fetchDesktopRoomPreferences,
  requestDesktopHostProductSurface,
  requestDesktopPreferencesReadTicket,
} from "./desktopBridge";

import { supportsMessageAttachmentPreview } from "./remote/remoteWorkspace";

describe("desktop preference grant validation", () => {
  afterEach(() => {
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("rejects coerced ticket and loopback URL representations", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce({
        revision: PRODUCT_SURFACE_REVISION,
        digest: "4".repeat(64),
        commands: [
          "host_product_surface",
          "runtime_preferences_read_ticket",
        ],
      })
      .mockResolvedValueOnce({
        ticket: ["a".repeat(64)],
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49155",
      })
      .mockResolvedValueOnce({
        ticket: "b".repeat(64),
        ttl_seconds: 30,
        http_base_url: "http://127.0.0.1:49155/",
      });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });

    await requestDesktopHostProductSurface();
    await expect(
      requestDesktopPreferencesReadTicket("general")
    ).rejects.toThrow("권위");
    await expect(
      requestDesktopPreferencesReadTicket("general")
    ).rejects.toThrow("정규 형식");

    expect(invoke).toHaveBeenNthCalledWith(2, "runtime_preferences_read_ticket", {
      roomId: "general",
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "runtime_preferences_read_ticket", {
      roomId: "general",
    });
  });
  it("learns preview support from the existing native preferences HTTP flow", async () => {
    const origin = "http://127.0.0.1:49565";
    const ticket = "c".repeat(64);
    const invoke = vi.fn().mockResolvedValueOnce({ revision: PRODUCT_SURFACE_REVISION,
      digest: "4".repeat(64), commands: ["host_product_surface", "runtime_preferences_read_ticket"]
    }).mockResolvedValueOnce({ ticket, ttl_seconds: 30, http_base_url: origin });
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response("{}", {
      headers: { "x-agentsassemble-image-preview": "1" }
    })));
    try {
      expect(supportsMessageAttachmentPreview(ticket, `${origin}/api/attachments/unused?view=1`)).toBe(false);
      await requestDesktopHostProductSurface();
      await fetchDesktopRoomPreferences("general");
      expect(supportsMessageAttachmentPreview(ticket, `${origin}/api/attachments/unused?view=1`)).toBe(true);
    } finally { vi.unstubAllGlobals(); }
  });

});
