import { describe, expect, it } from "vitest";

import { roomGlobalSettingsUpdateToApi } from "./room";

describe("room global settings wire update", () => {
  it("preserves an explicit empty appearance reference for server-owned cleanup", () => {
    expect(
      roomGlobalSettingsUpdateToApi({ appearance: { bannerImage: "" } })
    ).toEqual({ appearance: { banner_image_url: "" } });
  });
});
