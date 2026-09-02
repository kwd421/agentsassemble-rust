import type { LobbyEvent, RoomAgentSession, RoomEvent, RoomMember } from "../api";
import type {
  ProviderCatalogSnapshot,
  RoomSocketAuth,
} from "../roomSocketClient";
import { ROOM_HISTORY_MAX_EVENTS } from "../types/generated/ROOM_HISTORY_WIRE";
import { resolveAttachmentReference } from "./attachmentReference";
import { isParticipantRole } from "./participantRole";
import {
  agentCreationProjectionFromEvent,
  joinedParticipantFromEvent,
} from "./participantEventContract";

export type CanonicalRoomHistoryState = {
  initialized: boolean;
  oldestSeq: number;
  lastSeq: number;
  hasMoreBefore: boolean;
  resumeGap: boolean;
  windowRevision: number;
};

export const EMPTY_CANONICAL_HISTORY: CanonicalRoomHistoryState = {
  initialized: false,
  oldestSeq: 0,
  lastSeq: 0,
  hasMoreBefore: false,
  resumeGap: false,
  windowRevision: 0,
};

export const EMPTY_PROVIDER_CATALOG: ProviderCatalogSnapshot = {
  status: "loading",
  catalog_revision: "",
  providers: [],
};

export type CanonicalParticipantProfile = {
  displayName?: string;
  avatarImageUrl?: string;
  providerKind?: string;
  role?: string;
};

export function applyCanonicalParticipantProfiles(
  events: LobbyEvent[],
  profiles: Record<string, CanonicalParticipantProfile>,
) {
  return events.map((event) => {
    const profile = profiles[event.actor_id || ""];
    if (!profile) return event;
    const next = {
      ...event,
      name: profile.displayName || event.name,
      avatar_image_url: profile.avatarImageUrl,
      provider_kind: profile.providerKind || event.provider_kind,
      role: profile.role || event.role,
    };
    return next.name === event.name &&
      next.avatar_image_url === event.avatar_image_url &&
      next.provider_kind === event.provider_kind &&
      next.role === event.role
      ? event
      : next;
  });
}

export function canonicalParticipantProfiles(
  sessions: RoomAgentSession[],
  participants: RoomMember[],
  displayResourceBase: string,
): Record<string, CanonicalParticipantProfile> {
  const profiles: Record<string, CanonicalParticipantProfile> = {};
  participants.forEach((participant) => {
    if (!participant.participant_id) return;
    profiles[participant.participant_id] = {
      displayName: participant.display_name,
      avatarImageUrl: resolveAttachmentReference(
        participant.avatar_image_url,
        displayResourceBase,
      ),
      providerKind: participant.provider_kind,
      role: participant.role,
    };
  });
  sessions.forEach((session) => {
    if (!session.participant_id) return;
    profiles[session.participant_id] = {
      displayName: session.display_name,
      avatarImageUrl: undefined,
      providerKind: session.provider_kind,
      role: profiles[session.participant_id]?.role,
    };
  });
  return profiles;
}

export function mergeRoomEvents(
  current: RoomEvent[],
  incoming: RoomEvent[],
  replace: boolean,
) {
  const byId = new Map(
    (replace ? [] : current).map((event) => [event.id, event]),
  );
  incoming.forEach((event) => {
    if (event.id) byId.set(event.id, event);
  });
  return [...byId.values()].sort(
    (left, right) => Number(left.seq || 0) - Number(right.seq || 0),
  ).slice(-ROOM_HISTORY_MAX_EVENTS);
}

export function upsertAgentSessions(
  current: RoomAgentSession[],
  incoming: RoomAgentSession[],
) {
  const byId = new Map(current.map((session) => [session.session_id, session]));
  incoming.forEach((session) => byId.set(session.session_id, session));
  return [...byId.values()];
}

export function agentSessionUpdatesFromEvents(
  incoming: RoomEvent[],
): RoomAgentSession[] {
  return incoming.flatMap((event) => {
    const agentSession = (
      event as unknown as { agent_session?: RoomAgentSession }
    ).agent_session;
    if (event.type === "agent_session_state" && agentSession) {
      return [agentSession];
    }
    if (event.type === "agent_session_created") {
      return [agentCreationProjectionFromEvent(event).agentSession];
    }
    return [];
  });
}

export function normalizeRoomParticipant(
  participant: RoomMember,
  roomId: string,
): RoomMember {
  if (!isParticipantRole(participant.role)) {
    throw new Error("Room participant has an unsupported canonical role.");
  }
  return {
    ...participant,
    meeting_id: participant.meeting_id || roomId,
    provider_kind: participant.provider_kind || "",
    connection_kind: participant.connection_kind || "",
    source:
      participant.source ||
      (participant.participant_type === "human" ? "room" : "agent_session"),
    created_at: participant.created_at || "",
    updated_at: participant.updated_at || "",
    avatar_image_url: participant.avatar_image_url,
  };
}

export function participantIsActive(participant: RoomMember) {
  return !["left", "kicked"].includes(String(participant.status || ""));
}

export function normalizeActiveRoomParticipants(
  participants: RoomMember[],
  roomId: string,
): RoomMember[] {
  return participants
    .filter(participantIsActive)
    .map((participant) => normalizeRoomParticipant(participant, roomId));
}

export function upsertRoomParticipants(
  current: RoomMember[],
  incoming: RoomMember[],
  roomId: string,
) {
  const byId = new Map(
    current.map((participant) => [participant.participant_id, participant]),
  );
  incoming.forEach((participant) => {
    const existing = byId.get(participant.participant_id);
    byId.set(
      participant.participant_id,
      normalizeRoomParticipant({ ...existing, ...participant }, roomId),
    );
  });
  return [...byId.values()];
}

export function applyParticipantEvents(
  current: RoomMember[],
  incoming: RoomEvent[],
) {
  const byId = new Map(
    current.map((participant) => [participant.participant_id, participant]),
  );
  let changed = false;
  for (const event of incoming) {
    const participantId = String(event.participant_id || "");
    if (event.type === "agent_session_created") {
      const created = agentCreationProjectionFromEvent(event).participant;
      byId.set(participantId, normalizeRoomParticipant(created, event.room_id));
      changed = true;
      continue;
    }
    if (event.type === "participant_joined") {
      const joined = joinedParticipantFromEvent(event);
      byId.set(participantId, normalizeRoomParticipant(joined, event.room_id));
      changed = true;
      continue;
    }
    if (event.type === "participant_left") {
      changed = byId.delete(participantId) || changed;
      continue;
    }
    if (event.type === "participant_muted") {
      const participant = byId.get(participantId);
      if (!participant) continue;
      if (typeof event.muted !== "boolean") {
        throw new Error("participant_muted event has no canonical mute state.");
      }
      byId.set(participantId, {
        ...participant,
        muted: event.muted,
        updated_at: event.created_at || participant.updated_at,
      });
      changed = true;
      continue;
    }
    if (event.type !== "participant_updated") continue;
    const participant = byId.get(participantId);
    if (!participant) continue;
    const role = "role" in event ? event.role : participant.role;
    if (!isParticipantRole(role)) {
      throw new Error(
        "participant_updated event has an unsupported canonical role.",
      );
    }
    byId.set(participantId, {
      ...participant,
      display_name: String(event.display_name || participant.display_name),
      role,
      avatar_image_url:
        "avatar_image_url" in event
          ? String(event.avatar_image_url || "") || undefined
          : participant.avatar_image_url,
      updated_at: event.created_at || participant.updated_at,
    });
    changed = true;
  }
  return changed ? [...byId.values()] : current;
}

export function canonicalRoomAuthKey(auth?: RoomSocketAuth): string {
  if (!auth) return "";
  return auth.kind === "host"
    ? `host:${auth.meetingId}`
    : `session:${auth.sessionToken}`;
}

export function canonicalRoomProjectionScopeKey(
  roomId: string,
  auth: RoomSocketAuth | undefined,
  viewerParticipantId: string,
  origin = typeof window !== "undefined" ? window.location.origin : "",
): string {
  const authKey = canonicalRoomAuthKey(auth);
  return roomId && authKey
    ? JSON.stringify([origin, roomId, authKey, viewerParticipantId])
    : "";
}
