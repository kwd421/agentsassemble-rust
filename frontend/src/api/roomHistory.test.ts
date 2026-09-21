import { afterEach, describe, expect, it, vi } from "vitest";

import { uploadLobbyAttachment } from "./roomHistory";

describe("pre-join profile attachment", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("does not send avatar bytes before admission", async () => {
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      uploadLobbyAttachment(new File(["avatar"], "guest.png", { type: "image/png" }), {
        purpose: "profile_avatar",
        deviceToken: "aad1_current-browser",
      })
    ).rejects.toThrow("프로필 사진은 입장할 때 저장됩니다.");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
