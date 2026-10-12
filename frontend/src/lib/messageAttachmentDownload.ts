import { createResourceUrl, revokeResourceUrl, releaseResourceBlob } from "./remote/remoteResources";
import { isDesktopWebview, saveDesktopMessageAttachment } from "./desktopBridge";

export async function startMessageAttachmentDownload(blob: Blob, filename: string, ownedUrl?: string) {
  if (isDesktopWebview()) {
    try { await saveDesktopMessageAttachment(blob, filename); }
    finally { releaseResourceBlob(blob); }
    return;
  }
  const objectUrl = ownedUrl ?? createResourceUrl(blob);
  try {
    const anchor = document.createElement("a");
    anchor.href = objectUrl;
    anchor.download = filename;
    anchor.click();
  } finally {
    if (!ownedUrl) revokeResourceUrl(objectUrl);
  }
}
