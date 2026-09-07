import { resolveRoomHttpAuthority } from "../api";
import { useRoomMessageSearch } from "../views/useRoomMessageSearch";
import type { ChannelSearchScope } from "../views/components/ChannelHeader";

export function useAppMessageSearch({
  roomId,
  scope,
  sessionToken,
  deviceToken,
  localAvailable,
}: {
  roomId: string;
  scope: ChannelSearchScope;
  sessionToken: string;
  deviceToken?: string;
  localAvailable: boolean;
}) {
  const channelId = scope === "all" ? "all" : "lobby";
  const roomHttpAuthority = resolveRoomHttpAuthority(sessionToken, localAvailable, deviceToken);
  const roomMessageSearch = useRoomMessageSearch({
    roomId,
    channelId,
    authority: roomHttpAuthority,
  });
  return { roomHttpAuthority, roomMessageSearch };
}
