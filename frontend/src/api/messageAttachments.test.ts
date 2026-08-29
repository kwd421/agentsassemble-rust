import { beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  upload: vi.fn(),
  read: vi.fn(),
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  requestDesktopMessageAttachmentUploadTicket: bridge.upload,
  requestDesktopMessageAttachmentReadTicket: bridge.read,
}));

import {
  fetchMessageAttachmentBlob,
  uploadMessageAttachment,
  type LobbyAttachmentRef,
} from "./messageAttachments";

const attachmentId = `ma_${"a".repeat(32)}`;
const attachment: LobbyAttachmentRef = {
  id: attachmentId,
  filename: "notes.txt",
  content_type: "text/plain",
  size: 5,
  is_image: false,
  url: `/api/attachments/${attachmentId}?view=1`,
  download_url: `/api/attachments/${attachmentId}?download=1`,
};

function grant(ticket: string) {
  return {
    ticket,
    ttl_seconds: 30,
    http_base_url: "http://127.0.0.1:49154",
  };
}

function jsonResponse(value: unknown, status = 200, cacheControl = "private, no-store") {
  return new Response(JSON.stringify(value), {
    status,
    headers: {
      "Cache-Control": cacheControl,
      "Content-Type": "application/json",
    },
  });
}

function attachmentResponse(
  body: BodyInit = "notes",
  contentType = "text/plain",
  cacheControl = "private, no-store"
) {
  return new Response(body, {
    status: 200,
    headers: {
      "Cache-Control": cacheControl,
      "Content-Type": contentType,
    },
  });
}

describe("lobby message-attachment HTTP authority", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.unstubAllGlobals();
  });

  it("uploads through one fresh local grant with only the canonical message payload", async () => {
    bridge.upload.mockResolvedValue(grant("a".repeat(64)));
    const fetchMock = vi.fn().mockResolvedValue(
      jsonResponse({ attachment })
    );
    vi.stubGlobal("fetch", fetchMock);
    const beforeDispatch = vi.fn();

    await expect(
      uploadMessageAttachment(
        new File(["notes"], "notes.txt", { type: "text/plain" }),
        "general",
        { kind: "local" },
        beforeDispatch
      )
    ).resolves.toEqual(attachment);

    expect(bridge.upload).toHaveBeenCalledWith("general");
    expect(beforeDispatch).toHaveBeenCalledOnce();
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:49154/api/attachments");
    expect((init.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"a".repeat(64)}`
    );
    expect(JSON.parse(String(init.body))).toEqual({
      purpose: "room_attachment",
      filename: "notes.txt",
      content_type: "text/plain",
      data_base64: "bm90ZXM=",
    });
  });

  it("exchanges a remote session before upload and never sends it to the target", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        jsonResponse({ ticket: "b".repeat(64), ttl_seconds: 30 })
      )
      .mockResolvedValueOnce(jsonResponse({ attachment }));
    vi.stubGlobal("fetch", fetchMock);

    await uploadMessageAttachment(
      new File(["notes"], "notes.txt", { type: "text/plain" }),
      "general",
      { kind: "remote", sessionToken: "aas1.session" }
    );

    const exchangeHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    const targetHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      "/api/session-tickets/message-attachment-upload"
    );
    expect(exchangeHeaders.get("Authorization")).toBe("Bearer aas1.session");
    expect(fetchMock.mock.calls[1]?.[0]).toBe("/api/attachments");
    expect(targetHeaders.get("Authorization")).toBe(
      `Bearer ${"b".repeat(64)}`
    );
    expect(JSON.stringify(fetchMock.mock.calls[1]?.[1])).not.toContain(
      "aas1.session"
    );
  });

  it("does not dispatch an upload after denied, non-private, or malformed exchange", async () => {
    const responses = [
      jsonResponse(
        { code: "permission_denied", message: "This room session cannot upload." },
        403
      ),
      jsonResponse({ ticket: "b".repeat(64), ttl_seconds: 30 }, 200, "public"),
      jsonResponse({ ticket: "b".repeat(64), ttl_seconds: 30, ignored: true }),
    ];
    for (const response of responses) {
      const fetchMock = vi.fn().mockResolvedValueOnce(response);
      vi.stubGlobal("fetch", fetchMock);
      await expect(
        uploadMessageAttachment(
          new File(["notes"], "notes.txt", { type: "text/plain" }),
          "general",
          { kind: "remote", sessionToken: "aas1.session" }
        )
      ).rejects.toThrow();
      expect(fetchMock).toHaveBeenCalledOnce();
    }
  });

  it("checks currentness after a grant and before either transfer dispatch", async () => {
    const uploadFetch = vi.fn().mockResolvedValueOnce(
      jsonResponse({ ticket: "b".repeat(64), ttl_seconds: 30 })
    );
    vi.stubGlobal("fetch", uploadFetch);
    await expect(
      uploadMessageAttachment(
        new File(["notes"], "notes.txt", { type: "text/plain" }),
        "general",
        { kind: "remote", sessionToken: "aas1.session" },
        () => {
          throw new Error("retired");
        }
      )
    ).rejects.toThrow("retired");
    expect(uploadFetch).toHaveBeenCalledOnce();

    bridge.read.mockResolvedValue(grant("c".repeat(64)));
    const readFetch = vi.fn();
    vi.stubGlobal("fetch", readFetch);
    await expect(
      fetchMessageAttachmentBlob(
        attachment,
        "general",
        { kind: "local" },
        "view",
        undefined,
        () => {
          throw new Error("retired");
        }
      )
    ).rejects.toThrow("retired");
    expect(readFetch).not.toHaveBeenCalled();
  });

  it("does not dispatch a target upload after a delayed grant is retired", async () => {
    let resolveExchange!: (response: Response) => void;
    const exchange = new Promise<Response>((resolve) => {
      resolveExchange = resolve;
    });
    const fetchMock = vi.fn().mockReturnValueOnce(exchange);
    vi.stubGlobal("fetch", fetchMock);
    const controller = new AbortController();
    const beforeDispatch = vi.fn();
    const request = uploadMessageAttachment(
      new File(["notes"], "notes.txt", { type: "text/plain" }),
      "general",
      { kind: "remote", sessionToken: "aas1.session" },
      beforeDispatch,
      controller.signal
    );
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalledOnce());

    controller.abort();
    resolveExchange(jsonResponse({ ticket: "b".repeat(64), ttl_seconds: 30 }));

    await expect(request).rejects.toMatchObject({ name: "AbortError" });
    expect(beforeDispatch).not.toHaveBeenCalled();
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("rejects substituted upload metadata instead of accepting a generic attachment", async () => {
    bridge.upload.mockResolvedValue(grant("a".repeat(64)));
    const malformed = [
      { ...attachment, id: `ma_${"b".repeat(32)}` },
      { ...attachment, url: `${attachment.url}&extra=1` },
      { ...attachment, content_type: "Text/Plain" },
      { ...attachment, filename: " ../notes.txt" },
      { ...attachment, is_image: "yes" },
      { ...attachment, size: 0 },
      { ...attachment, ignored: true },
    ];
    for (const value of malformed) {
      vi.stubGlobal(
        "fetch",
        vi.fn().mockResolvedValue(jsonResponse({ attachment: value }))
      );
      await expect(
        uploadMessageAttachment(
          new File(["notes"], "notes.txt", { type: "text/plain" }),
          "general",
          { kind: "local" }
        )
      ).rejects.toThrow("응답 계약");
    }
  });

  it("consumes the server-owned safe-image classification without a MIME mirror", async () => {
    bridge.upload.mockResolvedValue(grant("a".repeat(64)));
    const classified = { ...attachment, is_image: true };
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(jsonResponse({ attachment: classified }))
    );

    await expect(
      uploadMessageAttachment(
        new File(["notes"], "notes.txt", { type: "text/plain" }),
        "general",
        { kind: "local" }
      )
    ).resolves.toEqual(classified);
  });

  it("uses exact local and remote read grants bound to the canonical attachment", async () => {
    bridge.read.mockResolvedValue(grant("c".repeat(64)));
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(attachmentResponse())
      .mockResolvedValueOnce(
        jsonResponse({ ticket: "d".repeat(64), ttl_seconds: 30 })
      )
      .mockResolvedValueOnce(attachmentResponse());
    vi.stubGlobal("fetch", fetchMock);

    await fetchMessageAttachmentBlob(
      attachment,
      "general",
      { kind: "local" },
      "download"
    );
    await fetchMessageAttachmentBlob(
      attachment,
      "general",
      { kind: "remote", sessionToken: "aas1.session" },
      "view"
    );

    expect(bridge.read).toHaveBeenCalledWith("general", attachmentId);
    expect(fetchMock.mock.calls[0]?.[0]).toBe(
      `http://127.0.0.1:49154${attachment.download_url}`
    );
    expect(fetchMock.mock.calls[1]?.[0]).toBe(
      `/api/session-tickets/message-attachment/${attachmentId}`
    );
    expect(fetchMock.mock.calls[2]?.[0]).toBe(attachment.url);
    const remoteTargetHeaders = fetchMock.mock.calls[2]?.[1]?.headers as Headers;
    expect(remoteTargetHeaders.get("Authorization")).toBe(
      `Bearer ${"d".repeat(64)}`
    );
    expect(remoteTargetHeaders.get("Authorization")).not.toContain("aas1.session");
  });

  it("rejects non-private, wrong-type, and wrong-size reads without fallback", async () => {
    bridge.read.mockResolvedValue(grant("c".repeat(64)));
    const responses = [
      attachmentResponse("notes", "text/plain", "public"),
      attachmentResponse("notes", "application/octet-stream"),
      attachmentResponse("longer"),
    ];
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    for (const response of responses) {
      fetchMock.mockResolvedValueOnce(response);
      await expect(
        fetchMessageAttachmentBlob(
          attachment,
          "general",
          { kind: "local" },
          "view"
        )
      ).rejects.toThrow("응답 계약");
    }
    expect(fetchMock).toHaveBeenCalledTimes(responses.length);
  });

  it("rejects invalid room or metadata before consuming a read grant", async () => {
    await expect(
      fetchMessageAttachmentBlob(
        attachment,
        " general",
        { kind: "local" },
        "view"
      )
    ).rejects.toThrow("방 식별자");
    await expect(
      fetchMessageAttachmentBlob(
        { ...attachment, download_url: `${attachment.download_url}&extra=1` },
        "general",
        { kind: "local" },
        "download"
      )
    ).rejects.toThrow("응답 계약");
    expect(bridge.read).not.toHaveBeenCalled();
  });
});
