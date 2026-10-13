import { installRemoteWorkspace, closeRemoteWorkspace, fetchPlainProductTransport } from "../lib/remote/remoteWorkspace";
import { releaseResourceBlob } from "../lib/remote/remoteResources";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const bridge = vi.hoisted(() => ({
  upload: vi.fn(),
  read: vi.fn(),
  settings: vi.fn(),
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>(
    "../lib/desktopBridge"
  )),
  requestDesktopMessageAttachmentUploadTicket: bridge.upload,
  requestDesktopMessageAttachmentReadTicket: bridge.read,
}));

vi.mock("./room", () => ({ fetchRoomSettings: bridge.settings }));

import {
  fetchMessageAttachmentBlob,
  uploadMessageAttachment,
  type LobbyAttachmentRef,
} from "./messageAttachments";
import { OutgoingMessages } from "../lib/outgoingMessages";
import type { MessageAttachmentUploadClient } from "./messageAttachmentTransfers";

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
  afterEach(() => closeRemoteWorkspace());
  beforeEach(() => {
    vi.resetAllMocks();
    bridge.settings.mockResolvedValue({ attachmentFileLimitBytes: 10485760 });
    vi.unstubAllGlobals();
  });

  it("preflights against freshly resolved room policy before reading file bytes", async () => {
    const file = new File([new Uint8Array(25 * 1024 * 1024)], "large.bin");
    const fetchMock = vi.fn().mockResolvedValue(jsonResponse({ attachment: { ...attachment, filename: file.name,
      content_type: "application/octet-stream", size: file.size } }));
    vi.stubGlobal("fetch", fetchMock);
    const authority = { kind: "remote" as const, sessionToken: "aas1.session" };
    await expect(uploadMessageAttachment(file, "general", authority)).rejects.toThrow("파일은 최대 10MB까지 올릴 수 있어요.");
    expect(fetchMock).not.toHaveBeenCalled();
    expect(bridge.settings).toHaveBeenCalledWith("general", { sessionToken: "aas1.session", deviceToken: undefined });
    bridge.settings.mockResolvedValueOnce({ attachmentFileLimitBytes: 52428800 });
    await expect(uploadMessageAttachment(file, "general", authority)).resolves.toMatchObject({ size: file.size });
    bridge.settings.mockRejectedValueOnce(new Error("settings unavailable"));
    await expect(uploadMessageAttachment(file, "general", authority)).rejects.toThrow("settings unavailable");
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("downloads retained large files through the encrypted resource budget", async () => {
    const bytes = new Uint8Array(25 * 1024 * 1024);
    const fetch = vi.fn().mockResolvedValue(attachmentResponse(bytes));
    const transport = { hello: { origin: "https://host.test" }, active: true, onClose: () => () => {}, close() {}, fetch };
    installRemoteWorkspace({ transport: transport as unknown as RemoteTransport,
      owner: { sessionToken: "resource-owner" } as never, deviceToken: "device", clientId: "client" });
    const blob = await fetchMessageAttachmentBlob({ ...attachment, size: bytes.length }, "general",
      { kind: "remote", sessionToken: "resource-owner" }, "download");
    expect(blob.size).toBe(bytes.length);
    releaseResourceBlob(blob);
    expect(bridge.settings).not.toHaveBeenCalled();
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
    expect(url).toBe("http://127.0.0.1:49154/api/message-attachments");
    expect((init.headers as Headers).get("Authorization")).toBe(
      `Bearer ${"a".repeat(64)}`
    );
    expect(JSON.parse(String(init.body))).toEqual({
      filename: "notes.txt",
      content_type: "text/plain",
      data_base64: "bm90ZXM=",
    });
  });

  it.each([
    { sessionToken: "aas1.session", deviceToken: undefined },
    { sessionToken: "aops1.paired", deviceToken: "paired-device" },
  ])("sends the reusable remote session directly to the message target ($sessionToken)", async ({ sessionToken, deviceToken }) => {
    const fetchMock = vi.fn().mockResolvedValueOnce(jsonResponse({ attachment }));
    vi.stubGlobal("fetch", fetchMock);

    await uploadMessageAttachment(
      new File(["notes"], "notes.txt", { type: "text/plain" }),
      "general",
      { kind: "remote", sessionToken, deviceToken }
    );

    expect(fetchMock).toHaveBeenCalledOnce();
    expect(fetchMock.mock.calls[0]?.[0]).toBe("/api/message-attachments");
    const targetHeaders = fetchMock.mock.calls[0]?.[1]?.headers as Headers;
    expect(targetHeaders.get("Authorization")).toBe(`Bearer ${sessionToken}`);
    expect(targetHeaders.get("X-Device-Token")).toBe(deviceToken ?? null);
  });

  it("surfaces a remote target denial without fallback", async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(
      jsonResponse(
        { code: "permission_denied", message: "This room session cannot upload." },
        403
      )
    );
    vi.stubGlobal("fetch", fetchMock);
    await expect(
      uploadMessageAttachment(
        new File(["notes"], "notes.txt", { type: "text/plain" }),
        "general",
        { kind: "remote", sessionToken: "aas1.session" }
      )
    ).rejects.toThrow("This room session cannot upload.");
    expect(fetchMock).toHaveBeenCalledOnce();
  });

  it("checks currentness before either transfer dispatch", async () => {
    const uploadFetch = vi.fn();
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
    expect(uploadFetch).not.toHaveBeenCalled();

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

  it("does not dispatch a local upload after a delayed grant is retired", async () => {
    let resolveGrant!: (value: ReturnType<typeof grant>) => void;
    bridge.upload.mockReturnValueOnce(new Promise((resolve) => {
      resolveGrant = resolve;
    }));
    const targetFetch = vi.fn();
    vi.stubGlobal("fetch", targetFetch);
    const controller = new AbortController();
    const beforeDispatch = vi.fn();
    const request = uploadMessageAttachment(
      new File(["notes"], "notes.txt", { type: "text/plain" }),
      "general",
      { kind: "local" },
      beforeDispatch,
      controller.signal
    );
    await vi.waitFor(() => expect(bridge.upload).toHaveBeenCalledOnce());

    controller.abort();
    resolveGrant(grant("b".repeat(64)));

    await expect(request).rejects.toMatchObject({ name: "AbortError" });
    expect(beforeDispatch).not.toHaveBeenCalled();
    expect(targetFetch).not.toHaveBeenCalled();
  });

  it("does not request or dispatch a local read after retirement", async () => {
    const alreadyRetired = new AbortController();
    alreadyRetired.abort();
    await expect(
      fetchMessageAttachmentBlob(
        attachment,
        "general",
        { kind: "local" },
        "view",
        alreadyRetired.signal
      )
    ).rejects.toMatchObject({ name: "AbortError" });
    expect(bridge.read).not.toHaveBeenCalled();

    let resolveGrant!: (value: ReturnType<typeof grant>) => void;
    bridge.read.mockReturnValueOnce(new Promise((resolve) => {
      resolveGrant = resolve;
    }));
    const targetFetch = vi.fn();
    vi.stubGlobal("fetch", targetFetch);
    const controller = new AbortController();
    const request = fetchMessageAttachmentBlob(
      attachment,
      "general",
      { kind: "local" },
      "view",
      controller.signal
    );
    await vi.waitFor(() => expect(bridge.read).toHaveBeenCalledOnce());

    controller.abort();
    resolveGrant(grant("c".repeat(64)));

    await expect(request).rejects.toMatchObject({ name: "AbortError" });
    expect(targetFetch).not.toHaveBeenCalled();
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

  it("refuses a legacy receipt of another size before any publication and accepts the host's canonical name and type", async () => {
    const authority = { roomId: "general", participantId: "person", generation: 1 };
    const remote = { kind: "remote" as const, sessionToken: "legacy-session" };
    const cases = [
      { file: new File(["notes"], "notes.txt", { type: "text/plain" }), receipt: { ...attachment, size: 6 }, valid: false },
      // The host canonicalises name and type; whatever it returns for the right size is accepted.
      { file: new File(["notes"], "dir\\\u0085notes\0.txt\u0085", { type: "Text/Plain; charset=UTF-8" }), receipt: attachment, valid: true },
      { file: new File(["notes"], ".."), receipt: { ...attachment, filename: "attachment.bin", content_type: "application/octet-stream" }, valid: true },
    ];
    for (const { file, receipt, valid } of cases) {
      vi.stubGlobal("fetch", vi.fn().mockResolvedValue(jsonResponse({ attachment: receipt })));
      const publish = vi.fn(async () => ({ op: "ack" as const, accepted: true as const, request_id: "request",
        action: "message.send", resolution: "committed" as const, deduplicated: false }));
      const model = new OutgoingMessages({ hostId: "legacy-host", personId: "person", publish });
      const unexpectedTransfer = async (): Promise<never> => { throw new Error("legacy host used transfer route"); };
      model.admit(authority, { transfer: false, limit: 8, fileLimit: 10485760, preflight() {},
        begin: unexpectedTransfer, data: unexpectedTransfer, finish: unexpectedTransfer,
        status: unexpectedTransfer, cancel: unexpectedTransfer,
        legacy: (selected: File, signal: AbortSignal) => uploadMessageAttachment(selected, "general", remote, undefined, signal),
      } satisfies MessageAttachmentUploadClient);
      model.send(authority, { text: "", files: [file] });
      await vi.waitFor(() => expect(model.snapshot()[0].phase).toBe(valid ? "sent" : "failed"));
      if (valid) expect(publish).toHaveBeenCalledOnce();
      else {
        expect(model.snapshot()[0].failure).toEqual({ code: "invalid_upload_response", retryable: false });
        expect(model.snapshot()[0].files[0].attachment).toBeUndefined();
        expect(publish).not.toHaveBeenCalled();
      }
      model.close();
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

  it("uses an exact local read grant and the direct reusable remote session", async () => {
    bridge.read.mockResolvedValue(grant("c".repeat(64)));
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(attachmentResponse())
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
    expect(fetchMock.mock.calls[1]?.[0]).toBe(attachment.url);
    const remoteTargetHeaders = fetchMock.mock.calls[1]?.[1]?.headers as Headers;
    expect(remoteTargetHeaders.get("Authorization")).toBe("Bearer aas1.session");
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


it.each([false, true])("uses preview only on an advertised encrypted channel (%s)", async supported => {
  const image = { ...attachment, filename: "image.png", content_type: "image/png", size: 400000, is_image: true };
  const fetch = vi.fn().mockResolvedValue(attachmentResponse(
    new Uint8Array(supported ? 100 : image.size), supported ? "image/webp" : "image/png"));
  const transport = { hello: { origin: "https://preview.test" }, active: true,
    supportsImagePreview: () => supported, onClose: () => () => {}, close() {}, fetch };
  installRemoteWorkspace({ transport: transport as unknown as RemoteTransport,
    owner: { sessionToken: "preview-owner" } as never, deviceToken: "device", clientId: "client" });
  try {
    const blob = await fetchMessageAttachmentBlob(image, "general", { kind: "remote", sessionToken: "preview-owner" }, "preview");
    expect(fetch).toHaveBeenCalledOnce();
    expect(fetch.mock.calls[0][0]).toBe(image.url.replace("?view=1", supported ? "?preview=1" : "?view=1"));
    releaseResourceBlob(blob);
  } finally { closeRemoteWorkspace(); }
});

it("learns plain desktop preview capability only from the settings probe", async () => {
  const image = { ...attachment, content_type: "image/png", size: 400000, is_image: true };
  const origin = "http://127.0.0.1:49255";
  bridge.read.mockResolvedValue({ ...grant("preview-local"), http_base_url: origin });
  const originalResponse = attachmentResponse(new Uint8Array(image.size), "image/png");
  originalResponse.headers.set("x-agentsassemble-image-preview", "1");
  const settings = new Response("{}", { headers: { "x-agentsassemble-image-preview": "1" } });
  const fetch = vi.fn().mockResolvedValueOnce(originalResponse).mockResolvedValueOnce(settings)
    .mockResolvedValueOnce(attachmentResponse(new Uint8Array(100), "image/webp"));
  vi.stubGlobal("fetch", fetch);
  const original = await fetchMessageAttachmentBlob(image, "general", { kind: "local" }, "preview");
  await fetchPlainProductTransport(`${origin}/api/room-settings?room_id=general`);
  const preview = await fetchMessageAttachmentBlob(image, "general", { kind: "local" }, "preview");
  expect(fetch.mock.calls.map(call => call[0])).toEqual([
    `${origin}${image.url}`, `${origin}/api/room-settings?room_id=general`, `${origin}${image.url.replace("?view=1", "?preview=1")}`,
  ]);
  releaseResourceBlob(original); releaseResourceBlob(preview);
  vi.unstubAllGlobals();
});
