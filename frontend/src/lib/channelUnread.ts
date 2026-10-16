import type { ChannelSettings, RoomEvent } from "../api";
import { channelMessageCursorsAreValid } from "./roomSocketValidation";

export type ChannelMessageCursors = Record<string, number>;

// Snapshot cursors are the floor the host knows: the latest real message
// sequence per channel, including history outside the bounded event tail.
// Older hosts omit the field; channels without a known sequence read as
// read, never all unread.
export function parseChannelMessageCursors(value: unknown): ChannelMessageCursors {
  if (!channelMessageCursorsAreValid(value)) return {};
  return Object.fromEntries(
    value.map((entry) => [entry.channel_id, entry.last_seq])
  );
}

export function channelReadSequence(lastReadAt?: string): number {
  const match = /^seq:(\d+)$/.exec(lastReadAt || "");
  return match ? Number(match[1]) : 0;
}

// Mirrors the host cursor rule: lobby system lines are history, not unread;
// custom channel records are always real messages.
function isRealChannelMessage(event: RoomEvent, channelId: string): boolean {
  if (channelId === "lobby") {
    return event.type === "message_final" && event.message_kind !== "system";
  }
  return (
    event.type === "channel_message_final" &&
    (event.channel_id as unknown) === channelId
  );
}

export function latestChannelSequence(
  channelId: string,
  cursors: ChannelMessageCursors | undefined,
  eventLists: Array<readonly RoomEvent[] | undefined>
): number {
  let latest = cursors?.[channelId] || 0;
  for (const events of eventLists) {
    if (!events) continue;
    for (const event of events) {
      if (isRealChannelMessage(event, channelId) && event.seq > latest) {
        latest = event.seq;
      }
    }
  }
  return latest;
}

// Each channel compares only its own latest sequence against its own read
// marker. Muted channels never show unread.
export function projectChannelUnread({
  channelIds,
  cursors,
  eventLists,
  settings,
}: {
  channelIds: readonly string[];
  cursors: ChannelMessageCursors | undefined;
  eventLists: Array<readonly RoomEvent[] | undefined>;
  settings: Record<string, ChannelSettings>;
}): Record<string, boolean> {
  const unread: Record<string, boolean> = {};
  for (const channelId of channelIds) {
    const setting = settings[channelId];
    unread[channelId] =
      setting?.notifications !== "mute" &&
      latestChannelSequence(channelId, cursors, eventLists) >
        channelReadSequence(setting?.lastReadAt);
  }
  return unread;
}

// A collapsed category keeps the open channel and unread channels visible.
export function filterCollapsedChannels<T extends { id: string }>(
  channels: readonly T[],
  openChannelId: string,
  unread: Record<string, boolean>
): T[] {
  return channels.filter(
    (item) => item.id === openChannelId || unread[item.id] === true
  );
}
