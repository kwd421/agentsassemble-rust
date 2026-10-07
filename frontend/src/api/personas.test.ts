import { closeRemoteWorkspace, installRemoteWorkspace } from "../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

function remoteLibrary(owner = true) {
  const callbacks = new Set<() => void>();
  const remote = { hello: { origin: "https://persona-host.test" }, active: true, fetch: vi.fn(),
    onClose: (fn: () => void) => { callbacks.add(fn); return () => callbacks.delete(fn); },
    close: () => { remote.active = false; for (const fn of callbacks) fn(); } };
  installRemoteWorkspace({ transport: remote as unknown as RemoteTransport,
    owner: owner ? { sessionToken: "root-persona", generation: 17 } as never : null,
    member: owner ? undefined : { sessionToken: "member-persona" } as never,
    deviceToken: "remote-device", clientId: "remote-client" });
  return remote;
}
afterEach(() => closeRemoteWorkspace());
it("uses admitted remote owner custody for list, import and thumbnails in a desktop shell", async () => {
  bridge.fetchOperator.mockReset();
  const remote = remoteLibrary();
  remote.fetch.mockResolvedValueOnce(jsonResponse({ items: [summary] }))
    .mockResolvedValueOnce(jsonResponse({ persona: summary })).mockResolvedValueOnce(pngResponse());
  expect(await fetchPersonaAssets()).toEqual([summary]);
  expect(await importPersonaAsset(new File(["card"], "guide.json"))).toEqual(summary);
  expect((await fetchPersonaThumbnail("guide")).size).toBe(9);
  expect(remote.fetch.mock.calls.map(call => call[0])).toEqual([
    "/api/central-owner/personas", "/api/central-owner/personas/import", "/api/central-owner/personas/guide/thumbnail",
  ]);
  for (const [, init] of remote.fetch.mock.calls) {
    const headers = new Headers(init.headers);
    expect(headers.get("authorization")).toBe("Bearer root-persona");
    expect(headers.get("x-device-token")).toBe("remote-device");
    expect(headers.get("x-central-generation")).toBe("17");
  }
  expect(bridge.fetchOperator).not.toHaveBeenCalled();
});
it("never inherits local authority for a member, closed channel or switched upload", async () => {
  bridge.fetchOperator.mockReset();
  const member = remoteLibrary(false);
  await expect(fetchPersonaAssets()).rejects.toThrow("소유자");
  await expect(fetchPersonaThumbnail("guide")).rejects.toThrow("소유자");
  expect(member.fetch).not.toHaveBeenCalled();
  const owner = remoteLibrary(); owner.close();
  await expect(fetchPersonaAssets()).rejects.toThrow("소유자");
  const source = remoteLibrary();
  const upload = importPersonaAsset(new File(["private card"], "guide.json"));
  closeRemoteWorkspace();
  await expect(upload).rejects.toThrow("서버가 바뀌었어요");
  expect(source.fetch).not.toHaveBeenCalled();
  expect(bridge.fetchOperator).not.toHaveBeenCalled();
});
