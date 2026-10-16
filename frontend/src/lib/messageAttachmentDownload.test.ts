import { beforeEach, describe, expect, it, vi } from "vitest";

const desktopMock = vi.hoisted(() => ({
  isDesktop: vi.fn(),
  save: vi.fn(),
}));

vi.mock("./desktopBridge", () => ({
  isDesktopWebview: desktopMock.isDesktop,
  saveDesktopMessageAttachment: desktopMock.save,
}));

import { createResourceUrl, revokeResourceUrl } from "./remote/remoteResources";
import { installRemoteWorkspace, closeRemoteWorkspace } from "./remote/remoteWorkspace";
import type { RemoteTransport } from "./remote/remoteTransport";

import { startMessageAttachmentDownload } from "./messageAttachmentDownload";

describe("messageAttachmentDownload", () => {
  const createObjectURL = vi.fn<(blob: Blob) => string>();
  const revokeObjectURL = vi.fn<(url: string) => void>();
  let anchorClick: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.resetAllMocks();
    vi.stubGlobal("URL", { createObjectURL, revokeObjectURL });
    anchorClick = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  });

  it("uses the browser-owned URL lifecycle outside the desktop webview", async () => {
    desktopMock.isDesktop.mockReturnValue(false);
    createObjectURL.mockReturnValue("blob:browser");

    await startMessageAttachmentDownload(new Blob(["file"]), "file.txt");

    expect(anchorClick).toHaveBeenCalledOnce();
    expect(desktopMock.save).not.toHaveBeenCalled();
    expect(revokeObjectURL).toHaveBeenCalledWith("blob:browser");
  });

  it("uses only the native save boundary in the desktop webview", async () => {
    const blob = new Blob(["file"]);
    desktopMock.isDesktop.mockReturnValue(true);
    desktopMock.save.mockResolvedValue(true);

    await startMessageAttachmentDownload(blob, "file.txt");

    expect(desktopMock.save).toHaveBeenCalledWith(blob, "file.txt");
    expect(createObjectURL).not.toHaveBeenCalled();
    expect(anchorClick).not.toHaveBeenCalled();
  });
  it("downloads an already displayed 50 MiB original without duplicating its resource reservation", async () => {
    const callbacks = new Set<() => void>();
    const transport = { hello: { origin: "https://download.test" }, active: true,
      onClose: (callback: () => void) => { callbacks.add(callback); return () => callbacks.delete(callback); },
      close: () => { transport.active = false; for (const callback of callbacks) callback(); }
    };
    installRemoteWorkspace({ transport: transport as unknown as RemoteTransport,
      owner: { sessionToken: "download-owner", serverId: "download" } as never, deviceToken: "device", clientId: "client" });
    desktopMock.isDesktop.mockReturnValue(false);
    createObjectURL.mockReturnValue("blob:displayed-original");
    try {
      const blob = new Blob([new Uint8Array(50 * 1024 * 1024)]);
      const url = createResourceUrl(blob);
      await startMessageAttachmentDownload(blob, "original.png", url);
      expect(anchorClick).toHaveBeenCalledOnce();
      expect(createObjectURL).toHaveBeenCalledOnce();
      expect(revokeObjectURL).not.toHaveBeenCalled();
      revokeResourceUrl(url);
    } finally { closeRemoteWorkspace(); }
  });

});
