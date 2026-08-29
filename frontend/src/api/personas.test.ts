import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  fetchOperator: vi.fn(),
}));

vi.mock("../lib/desktopBridge", () => ({
  fetchDesktopOperatorRuntime: bridge.fetchOperator,
  fetchDesktopRuntime: vi.fn(),
  isDesktopWebview: () => true,
}));

import {
  fetchPersonaAssets,
  fetchPersonaThumbnail,
  importPersonaAsset,
} from "./personas";

const summary = {
  id: "guide",
  display_name: "Night Guide",
  asset_kind: "card",
  source_kind: "ccv3",
  lorebook_count: 1,
  asset_count: 1,
  ignored_feature_count: 0,
  tag_count: 0,
  thumbnail_url: "/api/personas/guide/thumbnail",
};

function jsonResponse(value: unknown) {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: {
      "Cache-Control": "private, no-store",
      "Content-Type": "application/json",
    },
  });
}

function pngResponse(bytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10, 0])) {
  return new Response(bytes, {
    status: 200,
    headers: {
      "Cache-Control": "private, no-store",
      "Content-Type": "image/png",
    },
  });
}

describe("persona local-operator API", () => {
  beforeEach(() => {
    bridge.fetchOperator.mockReset();
  });

  it("uses a fresh desktop operator exchange for list and import", async () => {
    bridge.fetchOperator
      .mockResolvedValueOnce(jsonResponse({ items: [summary] }))
      .mockResolvedValueOnce(jsonResponse({ persona: summary }));

    await expect(fetchPersonaAssets()).resolves.toEqual([summary]);
    await expect(
      importPersonaAsset(new File(["card"], "guide.json", { type: "application/json" }))
    ).resolves.toEqual(summary);

    expect(bridge.fetchOperator).toHaveBeenNthCalledWith(1, "/api/personas", {}, undefined);
    const [path, init] = bridge.fetchOperator.mock.calls[1] as [string, RequestInit];
    expect(path).toBe("/api/personas/import");
    expect(init.method).toBe("POST");
    expect(JSON.parse(String(init.body))).toEqual({
      filename: "guide.json",
      data_base64: "Y2FyZA==",
    });
  });

  it("constructs the fixed thumbnail path and validates private PNG bytes", async () => {
    bridge.fetchOperator.mockResolvedValue(pngResponse());
    const controller = new AbortController();

    const blob = await fetchPersonaThumbnail("Harbor Guide", controller.signal);

    expect(blob.size).toBe(9);
    expect(bridge.fetchOperator).toHaveBeenCalledWith(
      "/api/personas/Harbor%20Guide/thumbnail",
      { cache: "no-store", signal: controller.signal }
    );
  });

  it("rejects a thumbnail without the exact safe-raster contract", async () => {
    bridge.fetchOperator.mockResolvedValue(
      new Response(new Uint8Array([1, 2, 3]), {
        status: 200,
        headers: {
          "Cache-Control": "private, no-store",
          "Content-Type": "image/png",
        },
      })
    );

    await expect(fetchPersonaThumbnail("guide")).rejects.toThrow("응답 계약");
  });
});
