import { resolveRoomHttpAuthority } from "../api";
import { useRoomMessageSearch } from "../views/useRoomMessageSearch";
import type { ChannelSearchScope } from "../views/components/ChannelHeader";

export function useAppMessageSearch({
  roomId,
  roomUid,
  selectedChannelId,
  scope,
  sessionToken,
  deviceToken,
  localAvailable,
}: {
  roomId: string;
  roomUid: string;
  selectedChannelId: string;
  scope: ChannelSearchScope;
  sessionToken: string;
  deviceToken?: string;
  localAvailable: boolean;
}) {
  const channelId = scope === "all" ? "all" : selectedChannelId;
  const roomHttpAuthority = resolveRoomHttpAuthority(sessionToken, localAvailable, deviceToken);
  const roomMessageSearch = useRoomMessageSearch({
    roomId,
    roomUid,
    channelId,
    authority: roomHttpAuthority,
  });
  return { roomHttpAuthority, roomMessageSearch };
}
