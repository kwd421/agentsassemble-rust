import { useCallback, useMemo, type Dispatch, type SetStateAction } from "react";
import type { LiveAgent, RoomMember } from "../api";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import type { RoomDockItem } from "../lib/roomDockModel";
import {
  agentActivityIsVisible,
  persistAgentActivityVisibility,
  type AgentActivityVisibility,
} from "../lib/agentActivityPreferences";
import { isActivePresence } from "../lib/presenceStatus";
import { roomHasAgent } from "../lib/roomDockModel";
import { roomMentionables } from "../lib/roomMentionables";
import { roomTypingIndicators } from "../lib/roomTypingIndicators";
import { useCanonicalRoom } from "../useCanonicalRoom";
import { agentSessionMemberToLiveAgent } from "./appModel";

type AgentPresentationOptions = {
  canonicalRoom: ReturnType<typeof useCanonicalRoom>;
  activeRoom: RoomDockItem;
  activeRoomMembers: RoomMember[];
  guestSession: RoomGuestSession | null;
  agentActivityVisibility: AgentActivityVisibility;
  setAgentActivityVisibility: Dispatch<SetStateAction<AgentActivityVisibility>>;
};

export function useAgentPresentation({
  canonicalRoom,
  activeRoom,
  activeRoomMembers,
  guestSession,
  agentActivityVisibility,
  setAgentActivityVisibility,
}: AgentPresentationOptions) {
  const activeRoomAgentSessions = canonicalRoom.agentSessions;
  const activeRoomCapabilities = canonicalRoom.capabilities;
  const activeRoomHistory = canonicalRoom.history;
  const activeRoomTimelineEvents = canonicalRoom.timelineEvents;
  const visibleRoomTimelineEvents = useMemo(
    () =>
      activeRoomTimelineEvents.filter(
        (event) =>
          event.kind !== "thinking" ||
          agentActivityIsVisible(agentActivityVisibility, event.actor_id || "")
      ),
    [activeRoomTimelineEvents, agentActivityVisibility]
  );
  const loadCanonicalRoomHistory = canonicalRoom.loadHistory;
  const sendAgentControl = canonicalRoom.sendAgentControl;
  const sendAgentConfigure = canonicalRoom.sendAgentConfigure;
  const sendParticipantMute = canonicalRoom.sendParticipantMute;
  const sendParticipantRole = canonicalRoom.sendParticipantRole;
  const participantById = new Map(
    activeRoomMembers.map((member) => [member.participant_id, member])
  );
  const agents: LiveAgent[] = activeRoomAgentSessions.flatMap((session) => {
    const member = participantById.get(session.participant_id);
    return member?.participant_type === "agent"
      ? [agentSessionMemberToLiveAgent(member, session)]
      : [];
  });
  const scopedAgents = agents.filter((agent) => roomHasAgent(activeRoom, agent));
  const scopedViewerParticipantId = guestSession?.agentId || "operator-local";
  const changeAgentActivityVisibility = useCallback(
    (session: { participant_id: string }, visible: boolean) => {
      setAgentActivityVisibility((previous) => {
        const next = { ...previous, [session.participant_id]: visible };
        persistAgentActivityVisibility(next);
        return next;
      });
    },
    [setAgentActivityVisibility]
  );
  const scopedMentionables = useMemo(
    () =>
      roomMentionables({
        viewerParticipantId: scopedViewerParticipantId,
        agents: scopedAgents,
        members: activeRoomMembers,
        displayResourceBase: canonicalRoom.displayResourceBase,
      }),
    [
      activeRoomMembers,
      canonicalRoom.displayResourceBase,
      scopedAgents,
      scopedViewerParticipantId,
    ]
  );
  const scopedOnlineCount = scopedAgents.filter((agent) => isActivePresence(agent.status)).length;
  const typingIndicators = useMemo(
    () =>
      roomTypingIndicators({
        members: activeRoomMembers,
        sessions: activeRoomAgentSessions,
        progress: canonicalRoom.agentSessionProgress,
      }),
    [canonicalRoom.agentSessionProgress, activeRoomAgentSessions, activeRoomMembers]
  );

  return {
    activeRoomAgentSessions,
    activeRoomCapabilities,
    activeRoomHistory,
    visibleRoomTimelineEvents,
    loadCanonicalRoomHistory,
    sendAgentControl,
    sendAgentConfigure,
    sendParticipantMute,
    sendParticipantRole,
    scopedAgents,
    changeAgentActivityVisibility,
    scopedMentionables,
    scopedOnlineCount,
    typingIndicators,
  };
}
