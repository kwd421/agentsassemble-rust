import { afterEach, describe, expect, it, vi } from "vitest";

import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import {
  requestDesktopHostProductSurface,
  requestDesktopPreferencesReadTicket,
} from "./desktopBridge";

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
});
