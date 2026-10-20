import { fetchUserProfile } from "../api";
import { resolveAttachmentReference } from "./attachmentReference";
import { isDesktopWebview } from "./desktopBridge";

// The join screen offers the person's current picture as the friend room's
// avatar. It travels as the same square PNG the guest crop produces.
const AVATAR_SIDE = 256;
export const JOIN_AVATAR_MAX_BYTES = 1_400_000;

/** The picture this person shows today: their own computer's profile on the
 * desktop app, otherwise their account picture. Undefined when there is none. */
export async function currentProfilePictureUrl(accountPictureUrl?: string | null): Promise<string | undefined> {
  if (isDesktopWebview()) {
    try {
      const own = await fetchUserProfile({});
      const local = resolveAttachmentReference(own.profile.avatarImage, own.displayResourceBase || "");
      if (local) return local;
    } catch { /* fall back to the account picture */ }
  }
  return accountPictureUrl || undefined;
}

/** A centred square PNG data URL of the picture, or undefined when it cannot be read. */
export async function pictureAsJoinAvatar(url: string): Promise<string | undefined> {
  try {
    const image = new Image();
    image.crossOrigin = "anonymous";
    image.decoding = "async";
    image.src = url;
    await image.decode();
    const side = Math.min(image.naturalWidth, image.naturalHeight);
    if (!side) return undefined;
    const canvas = document.createElement("canvas");
    canvas.width = AVATAR_SIDE;
    canvas.height = AVATAR_SIDE;
    const context = canvas.getContext("2d");
    if (!context) return undefined;
    context.drawImage(image, (image.naturalWidth - side) / 2, (image.naturalHeight - side) / 2, side, side,
      0, 0, AVATAR_SIDE, AVATAR_SIDE);
    const dataUrl = canvas.toDataURL("image/png");
    return dataUrl.length * 0.75 <= JOIN_AVATAR_MAX_BYTES ? dataUrl : undefined;
  } catch {
    return undefined;
  }
}
