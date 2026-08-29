import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  upload: vi.fn(),
  pending: vi.fn(),
  bound: vi.fn(),
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  requestDesktopAppearanceUploadTicket: bridge.upload,
  requestDesktopAppearancePendingReadTicket: bridge.pending,
  requestDesktopAppearanceBoundReadTicket: bridge.bound,
}));

import {
  fetchRoomAppearanceBlob,
  uploadRoomAppearance,
} from "./roomAppearance";
import { MAX_ATTACHMENT_BYTES } from "../types/generated/ASSET_SAFETY_WIRE";

const manager = {
  server_id: "10000000-0000-4000-8000-000000000001",
  authority_lineage_id: "20000000-0000-4000-8000-000000000002",
  room_id: "general",
  room_uid: "30000000-0000-4000-8000-000000000003",
};
const assetId = `ra_${"a".repeat(32)}`;
const reference = `/api/attachments/${assetId}?view=1`;
const PNG_BASE64 =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==";

function pngBytes() {
  return Uint8Array.from(atob(PNG_BASE64), (character) => character.charCodeAt(0));
}

function grant(ticket: string) {
  return {
    ticket,
    ttl_seconds: 30,
    http_base_url: "http://127.0.0.1:49154",
  };
}

function pngResponse(body: BodyInit = pngBytes()) {
  return new Response(body, {
    status: 200,
    headers: {
      "Cache-Control": "private, no-store",
      "Content-Type": "image/png",
    },
  });
}

function jsonResponse(value: unknown) {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: {
      "Cache-Control": "private, no-store",
      "Content-Type": "application/json",
    },
  });
}

describe("room appearance HTTP contract", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("uploads only the canonical appearance payload through its manager grant", async () => {
    bridge.upload.mockResolvedValue(grant("b".repeat(64)));
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({
        attachment: {
          id: assetId,
          filename: "banner.png",
          content_type: "image/png",
          size: 3,
          is_image: true,
          url: reference,
        },
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    const uploaded = await uploadRoomAppearance(
      new File(["png"], "banner.png", { type: "image/png" }),
      manager
    );

    expect(uploaded.reference).toEqual({ assetId, url: reference });
    expect(bridge.upload).toHaveBeenCalledWith(manager);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:49154/api/attachments");
    expect(init.method).toBe("POST");
    expect((init.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"b".repeat(64)}`
    );
    expect(JSON.parse(String(init.body))).toEqual({
      purpose: "room_appearance",
      filename: "banner.png",
      content_type: "image/png",
      data_base64: "cG5n",
    });
  });

  it("rejects substituted upload metadata instead of accepting a generic attachment", async () => {
    bridge.upload.mockResolvedValue(grant("b".repeat(64)));
    const canonical = {
      id: assetId,
      filename: "banner.png",
      content_type: "image/png",
      size: 3,
      is_image: true,
      url: reference,
    };

    for (const attachment of [
      { ...canonical, id: `ra_${"c".repeat(32)}` },
      { ...canonical, content_type: "image/jpeg" },
      { ...canonical, url: `${reference}&download=1` },
      { ...canonical, ignored: true },
    ]) {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(jsonResponse({ attachment }))
      );
      await expect(
        uploadRoomAppearance(
          new File(["png"], "banner.png", { type: "image/png" }),
          manager
        )
      ).rejects.toThrow("응답 계약");
    }
  });

  it("rejects upload metadata without the private no-store response contract", async () => {
    bridge.upload.mockResolvedValue(grant("b".repeat(64)));
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ attachment: {} }), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        })
      )
    );

    await expect(
      uploadRoomAppearance(
        new File(["png"], "banner.png", { type: "image/png" }),
        manager
      )
    ).rejects.toThrow("응답 계약");
  });

  it("keeps local pending and bound reads on distinct exact-purpose grants", async () => {
    bridge.pending.mockResolvedValue(grant("c".repeat(64)));
    bridge.bound.mockResolvedValue(grant("d".repeat(64)));
    const fetchMock = vi.fn().mockImplementation(async () => pngResponse());
    vi.stubGlobal("fetch", fetchMock);

    await fetchRoomAppearanceBlob(reference, { kind: "local", manager }, "pending");
    await fetchRoomAppearanceBlob(reference, { kind: "local", manager }, "bound");

    expect(bridge.pending).toHaveBeenCalledWith(manager, assetId);
    expect(bridge.bound).toHaveBeenCalledWith(manager, assetId);
    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `http://127.0.0.1:49154${reference}`,
      expect.objectContaining({ cache: "no-store", headers: expect.any(Headers) })
    );
    const firstHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const secondHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(firstHeaders.get("Authorization")).toBe(`Bearer ${"c".repeat(64)}`);
    expect(secondHeaders.get("Authorization")).toBe(`Bearer ${"d".repeat(64)}`);
  });

  it("exchanges a remote session once and reads only the bound canonical asset", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({ ticket: "e".repeat(64), ttl_seconds: 30 })
      )
      .mockResolvedValueOnce(pngResponse());
    vi.stubGlobal("fetch", fetchMock);

    await fetchRoomAppearanceBlob(
      reference,
      { kind: "remote", sessionToken: "aas1.session" },
      "bound"
    );

    expect(fetchMock).toHaveBeenNthCalledWith(
      1,
      `/api/session-tickets/room-appearance/${assetId}`,
      expect.objectContaining({ method: "POST", headers: expect.any(Headers) })
    );
    expect(fetchMock).toHaveBeenNthCalledWith(
      2,
      reference,
      expect.objectContaining({ headers: expect.any(Headers) })
    );
    const exchangeHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const readHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(exchangeHeaders.get("Authorization")).toBe("Bearer aas1.session");
    expect(readHeaders.get("Authorization")).toBe(`Bearer ${"e".repeat(64)}`);
    await expect(
      fetchRoomAppearanceBlob(
        reference,
        { kind: "remote", sessionToken: "aas1.session" },
        "pending"
      )
    ).rejects.toThrow("pending");
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("rejects noncanonical references and non-PNG reads without fallback", async () => {
    bridge.bound.mockResolvedValue(grant("d".repeat(64)));
    const fetchMock = vi.fn().mockResolvedValue(
      new Response("jpeg", {
        status: 200,
        headers: {
          "Cache-Control": "private, no-store",
          "Content-Type": "image/jpeg",
        },
      })
    );
    vi.stubGlobal("fetch", fetchMock);

    await expect(
      fetchRoomAppearanceBlob(
        `${reference}&extra=1`,
        { kind: "local", manager },
        "bound"
      )
    ).rejects.toThrow("참조");
    await expect(
      fetchRoomAppearanceBlob(reference, { kind: "local", manager }, "bound")
    ).rejects.toThrow("응답 계약");
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("rejects non-private, invalid-signature, and oversized PNG responses", async () => {
    bridge.bound.mockResolvedValue(grant("d".repeat(64)));
    const oversized = new Uint8Array(MAX_ATTACHMENT_BYTES + 1);
    oversized.set(pngBytes().slice(0, 8));
    const responses = [
      new Response(pngBytes(), {
        status: 200,
        headers: { "Content-Type": "image/png" },
      }),
      new Response(pngBytes(), {
        status: 200,
        headers: {
          "Cache-Control": "public, max-age=3600",
          "Content-Type": "image/png",
        },
      }),
      pngResponse("not-png"),
      pngResponse(oversized),
    ];
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    for (const response of responses) {
      fetchMock.mockResolvedValueOnce(response);
      await expect(
        fetchRoomAppearanceBlob(reference, { kind: "local", manager }, "bound")
      ).rejects.toThrow("응답 계약");
    }
    expect(fetchMock).toHaveBeenCalledTimes(responses.length);
  });
});
