import { postJsonHost, postJsonModerator } from "./http";

export function archiveRoom(roomId: string, archived: boolean) {
  return postJsonModerator<{ status: string; room_id: string }>("/api/rooms/archive", {
    room_id: roomId,
    archived,
  });
}

export function claimHostDevice(params: { deviceToken: string; displayName?: string }) {
  return postJsonHost<{ status: string; user_id: string; participant_id: string; operator: boolean }>("/api/host/claim", {
    device_token: params.deviceToken,
    display_name: params.displayName || "",
  });
}
