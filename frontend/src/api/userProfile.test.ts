import { afterEach, describe, expect, it, vi } from "vitest";

import { DEFAULT_USER_PROFILE } from "../lib/userProfileModel";
import { requestDesktopHostProductSurface } from "../lib/desktopBridge";
import { PRODUCT_SURFACE_REVISION } from "../types/generated/PRODUCT_SURFACE_REVISION";
import { fetchUserProfile, saveUserProfile } from "./userProfile";

const PROFILE = {
  revision: 1,
  display_name: "Canonical",
  handle: "canonical",
  status: "online",
  custom_status: "Ready",
  avatar_label: "CA",
  avatar_image_url: "/api/attachments/avatar_1234?view=1",
  banner_preset: "default",
  accent_color: "#5865f2",
  mic_muted: false,
  deafened: true,
  created_at: "2026-08-28T00:00:00Z",
  updated_at: "2026-08-28T00:00:00Z",
};

function desktopInvoke() {
  return vi
    .fn()
    .mockResolvedValueOnce({
      revision: PRODUCT_SURFACE_REVISION,
      digest: "4".repeat(64),
      commands: ["host_product_surface", "runtime_operator_ticket"],
    })
    .mockResolvedValueOnce({
      ticket: "f".repeat(64),
      ttl_seconds: 30,
      http_base_url: "http://127.0.0.1:49163",
    });
}

describe("canonical user profile provenance", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("publishes the canonical profile with the same operator grant base", async () => {
    const invoke = desktopInvoke();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ profile: PROFILE }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
    );

    await requestDesktopHostProductSurface();
    const snapshot = await fetchUserProfile();

    expect(snapshot.profile.avatarImage).toBe(
      "/api/attachments/avatar_1234?view=1"
    );
    expect(snapshot.displayResourceBase).toBe("http://127.0.0.1:49163");
  });

  it("serializes a relative avatar unchanged when another profile field changes", async () => {
    const invoke = desktopInvoke();
    Object.assign(window, { __TAURI_INTERNALS__: { invoke } });
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ profile: PROFILE }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    await requestDesktopHostProductSurface();
    await saveUserProfile({
      ...DEFAULT_USER_PROFILE,
      avatarImage: "/api/attachments/avatar_1234?view=1",
      micMuted: false,
    });

    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toEqual(
      expect.objectContaining({
        avatar_image_url: "/api/attachments/avatar_1234?view=1",
        mic_muted: false,
      })
    );
  });

  it("rejects an absolute avatar before requesting authority or dispatching", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      saveUserProfile({
        ...DEFAULT_USER_PROFILE,
        avatarImage: "https://other.example/api/attachments/avatar_1234?view=1",
      })
    ).rejects.toThrow("아바타 참조");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
