import { afterEach, describe, expect, it, vi } from "vitest";

import type { LobbyAttachmentRef } from "./messageAttachments";
import { uploadLobbyAttachment } from "./roomHistory";

const attachment: LobbyAttachmentRef = {
  id: "avatar_12345678",
  filename: "guest.png",
  content_type: "image/png",
  size: 6,
  is_image: true,
  url: "/api/attachments/avatar_12345678?view=1",
  download_url: "/api/attachments/avatar_12345678?download=1",
};

describe("pre-join profile attachment authority", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("authenticates in bounded headers before sending avatar bytes", async () => {
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(JSON.stringify({ attachment }), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      uploadLobbyAttachment(new File(["avatar"], "guest.png", { type: "image/png" }), {
        purpose: "profile_avatar",
        inviteToken: "aaj1_current-invite",
        deviceToken: "aad1_current-browser",
      })
    ).resolves.toEqual(attachment);

    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const headers = new Headers(init.headers);
    expect(url).toBe("/api/attachments");
    expect(headers.get("X-Invite-Token")).toBe("aaj1_current-invite");
    expect(headers.get("X-Device-Token")).toBe("aad1_current-browser");
    expect(JSON.parse(String(init.body))).toEqual({
      purpose: "profile_avatar",
      filename: "guest.png",
      content_type: "image/png",
      data_base64: "YXZhdGFy",
    });
  });
});
