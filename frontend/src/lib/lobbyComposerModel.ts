import { MAX_MESSAGE_ATTACHMENTS_PER_EVENT } from "../types/generated/MESSAGE_ATTACHMENTS_WIRE";

export const MAX_ATTACHMENTS_PER_EVENT = MAX_MESSAGE_ATTACHMENTS_PER_EVENT;
// A selection that does not fit is refused whole; attaching only some of the files
// would silently drop the rest.
export function selectLobbyAttachmentFiles<T>(currentCount: number, selectedItems: T[]) {
  const fits = currentCount + selectedItems.length <= MAX_ATTACHMENTS_PER_EVENT;
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
