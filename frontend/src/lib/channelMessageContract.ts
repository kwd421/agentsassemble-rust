import type { RoomEvent } from "../types/generated/RoomEvent";
import { CHANNEL_MESSAGE_EVENT_TYPE, MAX_TEXT_CHAT_CHARACTERS } from "../types/generated/TEXT_CHAT_WIRE";
import { assertExactKeys } from "./strictJsonContract";
import { isUnicodeScalarString } from "./unicodeScalarString";
import { isCustomChannelId } from "./customChannelId";

/** Channel-specific fields atop the public room-event envelope. */
export function channelMessageFieldsAreValid(event: RoomEvent): boolean {
  assertExactKeys(event, ["v", "id", "seq", "created_at", "room_id", "type", "actor", "participant_id", "participant_type", "actor_id", "actor_type", "display_name", "content", "message_kind", "channel_id"], "channel message", ["message_deleted"]);
  const deleted = event.message_deleted === true;
  return (event.message_deleted === undefined || deleted) && event.type === CHANNEL_MESSAGE_EVENT_TYPE && isCustomChannelId(event.channel_id) &&
    event.participant_id === event.actor.participant_id && event.actor_id === event.participant_id &&
    event.participant_type === event.actor.participant_type && event.actor_type === event.participant_type &&
    typeof event.display_name === "string" && Boolean(event.display_name) &&
    event.message_kind === "message" && typeof event.content === "string" && (deleted ? event.content === "" : Boolean(event.content)) &&
    isUnicodeScalarString(event.content) && [...event.content].length <= MAX_TEXT_CHAT_CHARACTERS &&
    Number.isFinite(Date.parse(event.created_at));
}
