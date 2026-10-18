import type { MessageAttachmentAuthority } from "../api/messageAttachments";

export type RoomPostingMode = "host" | "guest";
export type RoomPostingTransport = "host-lobby" | "room-say" | "blocked";

export type RoomPostingState = {
  mode: RoomPostingMode;
  canPost: boolean;
  transport: RoomPostingTransport;
  sessionToken: string;
  disabledReason: string;
};

export function roomPostingState({
  guestLocked,
  guestReadOnly,
  sessionToken,
}: {
  guestLocked: boolean;
  guestReadOnly: boolean;
  sessionToken?: string;
}): RoomPostingState {
  const cleanSessionToken = String(sessionToken || "").trim();
  if (!guestLocked) {
    return {
      mode: "host",
      canPost: true,
      transport: "host-lobby",
      sessionToken: "",
      disabledReason: "",
    };
  }
  if (guestReadOnly) {
    return {
      mode: "guest",
      canPost: false,
      transport: "blocked",
      sessionToken: cleanSessionToken,
      disabledReason: "읽기 전용 초대예요. 이 방은 보기만 가능해요.",
    };
  }
  if (!cleanSessionToken) {
    return {
      mode: "guest",
      canPost: false,
      transport: "blocked",
      sessionToken: "",
      disabledReason: "메시지를 보내려면 유효한 초대 세션이 필요해요.",
    };
  }
  return {
    mode: "guest",
    canPost: true,
    transport: "room-say",
    sessionToken: cleanSessionToken,
    disabledReason: "",
  };
}

/**
 * Attachment authority for a room: a remote (member) room always resolves
 * through its own session, never through this computer's local host
 * authority. Without a session the room has no authority at all (undefined):
 * attachment clients probe room settings on creation, and a local request
 * would read or write another host's room.
 */
export function messageAttachmentAuthorityForRoom({
  roomOrigin,
  postingMode,
  sessionToken,
  deviceToken,
}: {
  roomOrigin?: string;
  postingMode: RoomPostingMode;
  sessionToken?: string;
  deviceToken?: string;
}): MessageAttachmentAuthority | undefined {
  if (postingMode === "guest") {
    if (!sessionToken && roomOrigin === "remote_server") return undefined;
    return { kind: "remote", sessionToken: sessionToken || "", deviceToken };
  }
  if (roomOrigin === "remote_server") return undefined;
  return { kind: "local" };
}
