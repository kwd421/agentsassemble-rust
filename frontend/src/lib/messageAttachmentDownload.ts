import { isDesktopWebview, saveDesktopMessageAttachment } from "./desktopBridge";

export async function startMessageAttachmentDownload(blob: Blob, filename: string) {
  if (isDesktopWebview()) {
    await saveDesktopMessageAttachment(blob, filename);
    return;
  }
  const objectUrl = URL.createObjectURL(blob);
  try {
    const anchor = document.createElement("a");
    anchor.href = objectUrl;
    anchor.download = filename;
    anchor.click();
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
}
