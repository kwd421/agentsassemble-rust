import type { RoomEvent } from "../api";
import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type { CanonicalParticipantProfile } from "./canonicalRoomProjection";
import { DEPARTED_USER_NAME, PARTICIPANT_ANONYMIZED_EVENT_TYPE } from "../types/generated/PARTICIPANT_ANONYMIZATION_WIRE";
import { assertExactKeys } from "./strictJsonContract";

export function anonymizedParticipant(event: RoomEvent): string {
  if (event.type !== PARTICIPANT_ANONYMIZED_EVENT_TYPE) return "";
  assertExactKeys(event, ["v", "id", "seq", "created_at", "room_id", "type", "actor", "participant_id", "participant_type", "actor_id", "actor_type", "display_name", "avatar_image_url", "avatar_label"], "탈퇴한 사용자 변경");
  if (!event.participant_id || event.participant_type !== "human" ||
      event.actor.participant_id !== event.participant_id || event.actor.participant_type !== "human" ||
      event.actor_id !== event.participant_id || event.actor_type !== "human" ||
      event.display_name !== DEPARTED_USER_NAME || event.avatar_image_url !== "" || event.avatar_label !== "") {
    throw new Error("탈퇴한 사용자 변경 정보가 올바르지 않아요.");
  }
  return event.participant_id;
}

export function anonymousAuthorProfiles(current: Record<string, CanonicalParticipantProfile>, events: RoomEvent[]) {
  const next = { ...current };
  for (const event of events) {
    const id = anonymizedParticipant(event);
    if (id) next[id] = { displayName: DEPARTED_USER_NAME, avatarImageUrl: "", avatarLabel: "", participantType: "human" };
  }
  return next;
}

export function scrubHistoricalAuthors(events: RoomEvent[], overrides: Record<string, CanonicalParticipantProfile>): RoomEvent[] {
  return events.map((event) => overrides[event.actor.participant_id]
    ? { ...event, display_name: DEPARTED_USER_NAME, avatar_image_url: "", avatar_label: "" }
    : event);
}

export function anonymousAuthorRevision(profiles: Record<string, CanonicalParticipantProfile>): string {
  return JSON.stringify(Object.keys(profiles).filter((id) => profiles[id].displayName === DEPARTED_USER_NAME).sort());
}

/** Historical authors outlive active participants and the bounded event window. */
export function useAnonymousAuthors(scope: string) {
  const current = useRef({ scope, authors: {} as Record<string, Record<string, CanonicalParticipantProfile>> });
  const [view, setView] = useState(current.current);
  useLayoutEffect(() => {
    if (current.current.scope !== scope) {
      current.current = { scope, authors: {} };
      setView(current.current);
    }
  }, [scope]);
  const apply = useCallback((roomId: string, incoming: RoomEvent[]) => {
    const overrides = anonymousAuthorProfiles(current.current.authors[roomId] || {}, incoming);
    current.current = { ...current.current, authors: { ...current.current.authors, [roomId]: overrides } };
    setView(current.current);
    return overrides;
  }, []);
  return { apply, authors: view.scope === scope ? view.authors : {} };
}
