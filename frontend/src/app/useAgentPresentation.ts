import { useCallback, useMemo, useState, type Dispatch, type SetStateAction } from "react";
import {
  fetchProviderUsage,
  type LiveAgent,
  type ProviderUsageSnapshot,
  type RoomMember,
} from "../api";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import type { RoomDockItem } from "../lib/roomDockModel";
import {
  agentActivityIsVisible,
  persistAgentActivityVisibility,
  type AgentActivityVisibility,
} from "../lib/agentActivityPreferences";
import type { AgentQuotaVisibilityViewer } from "../lib/agentQuotaVisibility";
import { isActivePresence } from "../lib/presenceStatus";
import { providerUsageAfterFailure } from "../lib/providerUsageState";
import { roomHasAgent } from "../lib/roomDockModel";
import { roomMentionables } from "../lib/roomMentionables";
import { roomTypingIndicators } from "../lib/roomTypingIndicators";
import { useCanonicalRoom } from "../useCanonicalRoom";
import { agentSessionMemberToLiveAgent, providerUsageTarget } from "./appModel";

type AgentPresentationOptions = {
  canonicalRoom: ReturnType<typeof useCanonicalRoom>;
  activeRoom: RoomDockItem;
  activeRoomMembers: RoomMember[];
  guestLocked: boolean;
  guestSession: RoomGuestSession | null;
  agentActivityVisibility: AgentActivityVisibility;
  setAgentActivityVisibility: Dispatch<SetStateAction<AgentActivityVisibility>>;
};

export function useAgentPresentation({
  canonicalRoom,
  activeRoom,
  activeRoomMembers,
  guestLocked,
  guestSession,
  agentActivityVisibility,
  setAgentActivityVisibility,
}: AgentPresentationOptions) {
  const [providerUsage, setProviderUsage] = useState<Record<string, ProviderUsageSnapshot>>({});
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
  const sendParticipantKick = canonicalRoom.sendParticipantKick;
  const sendParticipantMute = canonicalRoom.sendParticipantMute;
  const sendParticipantRole = canonicalRoom.sendParticipantRole;
  const loadProviderUsage = useCallback(async (session: Parameters<typeof providerUsageTarget>[0]) => {
    if (guestLocked) return;
    const target = providerUsageTarget(session);
    if (!target) return;
    try {
      const usage = await fetchProviderUsage(target.providerId, target.model);
      setProviderUsage((previous) => ({ ...previous, [target.key]: usage }));
    } catch {
      setProviderUsage((previous) => ({
        ...previous,
        [target.key]: providerUsageAfterFailure(
          previous[target.key],
          target.providerId
        ),
      }));
    }
  }, [guestLocked]);

  const sessionByParticipantId = new Map(
    activeRoomAgentSessions.map((session) => [session.participant_id, session])
  );
  const agents: LiveAgent[] = activeRoomMembers
    .filter(
      (member) =>
        member.source === "agent_session" && member.participant_type !== "human"
    )
    .map((member) => {
      const session = sessionByParticipantId.get(member.participant_id);
      const usageTarget = providerUsageTarget(session);
      return agentSessionMemberToLiveAgent(
        member,
        session,
        usageTarget ? providerUsage[usageTarget.key] : undefined,
        Boolean(usageTarget)
      );
    });
  const guestOwnedAgentIds = useMemo(() => {
    const agentId = guestSession?.agentId || "";
    return agentId ? [agentId, `${agentId}-ai`] : [];
  }, [guestSession?.agentId]);
  const localProcessAgentIds = useMemo(
    () =>
      guestLocked
        ? []
        : activeRoomAgentSessions
            .filter((session) => !session.external_owned)
            .map((session) => session.participant_id)
            .filter(Boolean),
    [activeRoomAgentSessions, guestLocked]
  );
  const quotaViewer = useMemo<AgentQuotaVisibilityViewer>(
    () => ({
      ownedAgentIds: guestOwnedAgentIds,
      localProcessAgentIds,
      hostCanViewLocalAgentQuotas: !guestLocked,
    }),
    [guestLocked, guestOwnedAgentIds, localProcessAgentIds]
  );

  const scopedAgents = agents.filter((agent) => roomHasAgent(activeRoom, agent));
  const scopedViewerParticipantId = guestSession?.agentId || "operator-local";
  const scopedViewerDisplayName =
    activeRoomMembers.find(
      (member) => member.participant_id === scopedViewerParticipantId
    )?.display_name || guestSession?.displayName || scopedViewerParticipantId;
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
        agents: scopedAgents,
        members: activeRoomMembers,
        sessions: activeRoomAgentSessions,
        progress: canonicalRoom.agentSessionProgress,
      }),
    [canonicalRoom.agentSessionProgress, activeRoomAgentSessions, activeRoomMembers, scopedAgents]
  );

  return {
    activeRoomAgentSessions,
    activeRoomCapabilities,
    activeRoomHistory,
    visibleRoomTimelineEvents,
    loadCanonicalRoomHistory,
    sendAgentControl,
    sendAgentConfigure,
    sendParticipantKick,
    sendParticipantMute,
    sendParticipantRole,
    loadProviderUsage,
    quotaViewer,
    scopedAgents,
    scopedViewerDisplayName,
    changeAgentActivityVisibility,
    scopedMentionables,
    scopedOnlineCount,
    typingIndicators,
  };
}
