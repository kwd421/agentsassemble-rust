import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";

import { LEGACY_MESSAGE_ATTACHMENT_LIMIT } from "./messageAttachmentCapabilities";

export const MAX_ATTACHMENTS_PER_EVENT = LEGACY_MESSAGE_ATTACHMENT_LIMIT;
// A selection that does not fit is refused whole; attaching only some of the files
// would silently drop the rest.
export function selectLobbyAttachmentFiles<T>(currentCount: number, selectedItems: T[], hostLimit = MAX_ATTACHMENTS_PER_EVENT) {
  const fits = currentCount + selectedItems.length <= Math.min(hostLimit, MAX_MESSAGE_ATTACHMENTS_PER_EVENT);
  return { accepted: fits ? selectedItems : ([] as T[]), tooMany: !fits };
}

export function lobbySubmitSuccessDraft<T>() {
  return { message: "", pendingAttachments: [] as T[] };
}

export function lobbySubmitFailureDraft<T>(
  draftMessage: string,
  draftAttachments: T[],
  errorMessage: string
) {
  return {
    message: draftMessage,
    pendingAttachments: draftAttachments,
    error: errorMessage,
  };
}
