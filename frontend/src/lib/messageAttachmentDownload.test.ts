import { beforeEach, describe, expect, it, vi } from "vitest";

const desktopMock = vi.hoisted(() => ({
  isDesktop: vi.fn(),
  save: vi.fn(),
}));

vi.mock("./desktopBridge", () => ({
  isDesktopWebview: desktopMock.isDesktop,
  saveDesktopMessageAttachment: desktopMock.save,
}));

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
});
