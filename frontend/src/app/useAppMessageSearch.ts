import { resolveRoomHttpAuthority } from "../api";
import { useRoomMessageSearch } from "../views/useRoomMessageSearch";
import type { ChannelSearchScope } from "../views/components/ChannelHeader";

export function useAppMessageSearch({
  roomId,
  selectedChannel,
  customChannelSelected,
  scope,
  sessionToken,
  localAvailable,
}: {
  roomId: string;
  selectedChannel: string;
  customChannelSelected: boolean;
  scope: ChannelSearchScope;
  sessionToken: string;
  localAvailable: boolean;
}) {
  const channelId = scope === "all"
    ? "all"
    : selectedChannel === "lobby" || customChannelSelected
      ? selectedChannel
      : "lobby";
  const roomHttpAuthority = resolveRoomHttpAuthority(sessionToken, localAvailable);
  const roomMessageSearch = useRoomMessageSearch({
    roomId,
    channelId,
    authority: roomHttpAuthority,
  });
  return { roomHttpAuthority, roomMessageSearch };
}
