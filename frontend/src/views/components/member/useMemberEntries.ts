import { useMemo } from "react";
import { Bot, UserCheck } from "../../../ui/icons";
import type {
  LiveAgent,
  RoomAgentSession,
  RoomMember,
} from "../../../api";
import { providerExecutionLabel, roomContextSummaryBadges } from "../../../lib/agentLabels";
import { participantTypeMeta } from "../../../lib/participantTypes";
import { resolveAttachmentReference } from "../../../lib/attachmentReference";
import {
  agentSessionIsPresent,
  agentSessionStatusLabel,
} from "../AgentSessionDetails";
import {
  inferAgentRole,
  isActive,
  memberActive,
  memberRole,
  memberSpeed,
  memberStatusLabel,
  ROLE_OPTIONS,
} from "./memberHelpers";
import type { MemberEntry } from "./memberTypes";

const NOT_WORKING: readonly string[] = [];

type MemberEntriesOptions = {
  agents: LiveAgent[];
  members: RoomMember[];
  viewerParticipantId: string;
  agentSessions: RoomAgentSession[];
  displayResourceBase: string;
  /** External AIs working on a message; they have no session status. */
  working?: readonly string[];
};

export function useMemberEntries({
  agents,
  members,
  viewerParticipantId,
  agentSessions,
  displayResourceBase,
  working = NOT_WORKING,
}: MemberEntriesOptions): {
  entries: MemberEntry[];
  contextBadges: ReturnType<typeof roomContextSummaryBadges>;
} {
  const contextBadges = roomContextSummaryBadges(agents);
  const entries = useMemo<MemberEntry[]>(() => {
    const memberById = new Map(members.map((member) => [member.participant_id, member]));
    const sessionByParticipantId = new Map(
      agentSessions.map((session) => [session.participant_id, session])
    );
    const mutedById = new Map(members.map((member) => [member.participant_id, Boolean(member.muted)]));
    const viewerMember = memberById.get(viewerParticipantId);
    const viewerEntryId = viewerMember?.participant_id || viewerParticipantId;
    const viewerDisplayName = String(viewerMember?.display_name || "").trim();
    const human: MemberEntry | null = viewerMember ? {
      id: viewerEntryId,
      member: viewerMember,
      displayName: viewerDisplayName,
      detail: "사람",
      statusLabel: viewerMember ? memberStatusLabel(viewerMember) : undefined,
      role: memberRole(viewerMember),
      owner: true,
      active: viewerMember ? memberActive(viewerMember) : true,
      muted: Boolean(viewerMember?.muted),
      meetingId: viewerMember.room_id,
      ownerId: viewerEntryId,
      ownerDisplayName: viewerDisplayName,
      avatarImage: resolveAttachmentReference(
        viewerMember.avatar_image_url,
        displayResourceBase
      ),
      avatarReference: viewerMember.avatar_image_url,
      icon: UserCheck,
    } : null;
    const agentEntries = agents.map((agent) => {
      const member = memberById.get(agent.agent_id);
      const agentSession = sessionByParticipantId.get(agent.agent_id);
      const inferredRole = inferAgentRole(agent);
      const role = member ? memberRole(member) : inferredRole;
      const ownerId = String(
        member ? member.owner_id || "" : agent.owner_id || ""
      ).trim();
      const ownedByViewer = Boolean(ownerId && ownerId === viewerParticipantId);
      const ownerDisplayName = String(
        memberById.get(ownerId)?.display_name ||
          (!member ? agent.owner_display_name : "") ||
          (ownedByViewer ? viewerDisplayName : "소유자 정보 없음")
      ).trim();
      const canonicalIdentity = agentSession || agent;
      const agentDisplayName = String(
        canonicalIdentity.display_name || agent.agent_id
      ).trim();
      const avatarReference = agentSession ? agentSession.avatar_image_url : agent.avatar_image_url;
      const avatarImage = resolveAttachmentReference(
        avatarReference,
        displayResourceBase
      );
      const executionDetail = providerExecutionLabel(agent);
      const modelLabel = String(agentSession?.model || agent.model_id || "").trim();
      const reasoningEffort = String(
        agentSession?.reasoning_effort || agent.effort || ""
      ).trim();
      const serviceTier = String(
        agentSession?.service_tier || agent.speed || ""
      ).trim().toLowerCase();
      const speed = memberSpeed(serviceTier) ?? (agent.fast_mode ? "fast" : null);
      const detail = [
        executionDetail === "Agent Session" ? "" : executionDetail,
        modelLabel,
      ]
        .filter(Boolean)
        .join(" · ");
      const runtimeStatus = agentSession?.runtime_status || agentSession?.status;
      return {
        id: agent.agent_id,
        agent,
        agentSession,
        member,
        displayName: agentDisplayName,
        detail,
        modelLabel,
        reasoningEffort,
        speed,
        ultraMode: ["ultra", "ultracode"].includes(
          reasoningEffort.toLowerCase()
        ),
        fullDetail: [detail, agentSession?.runtime_kind].filter(Boolean).join(" · "),
        statusLabel: agentSession
          ? agentSessionStatusLabel(runtimeStatus, agentSession.recovery_required)
          : working.includes(agent.agent_id)
            ? "응답 중"
            : member
              ? memberStatusLabel(member)
              : undefined,
        working: !agentSession && working.includes(agent.agent_id),
        role,
        owner: false,
        active: agentSession ? agentSessionIsPresent(runtimeStatus) : working.includes(agent.agent_id) || isActive(agent),
        muted: mutedById.get(agent.agent_id) ?? false,
        meetingId: member?.room_id || agent.meeting_id || "",
        ownerId: ownerId || undefined,
        ownerDisplayName,
        agentDisplayName,
        avatarImage,
        avatarReference,
        providerKind: String(
          canonicalIdentity.provider_kind || ""
        ),
        icon: ROLE_OPTIONS.find((option) => option.id === role)?.icon || Bot,
      } satisfies MemberEntry;
    });
    const agentIds = new Set(agentEntries.map((entry) => entry.id));
    const invitedEntries = members
      .filter(
        (member) =>
          member.participant_id &&
          member.participant_id !== viewerParticipantId &&
          !agentIds.has(member.participant_id)
      )
      .map((member) => {
        const agentSession = sessionByParticipantId.get(member.participant_id);
        const role = memberRole(member);
        const typeMeta = participantTypeMeta(member.participant_type);
        const fullDetail = typeMeta.label;
        const detail = typeMeta.label;
        const ownerId =
          member.participant_type === "human"
            ? member.participant_id
            : String(member.owner_id || "").trim();
        return {
          id: member.participant_id,
          agentSession,
          member,
          displayName: agentSession
            ? agentSession.display_name || member.participant_id
            : member.display_name || member.participant_id,
          detail: [detail, agentSession?.model].filter(Boolean).join(" · "),
          modelLabel: String(agentSession?.model || "").trim(),
          reasoningEffort: String(agentSession?.reasoning_effort || "").trim(),
          speed: memberSpeed(agentSession?.service_tier),
          ultraMode: ["ultra", "ultracode"].includes(
            String(agentSession?.reasoning_effort || "").trim().toLowerCase()
          ),
          fullDetail: [fullDetail, agentSession?.runtime_kind].filter(Boolean).join(" · "),
          statusLabel: agentSession
            ? agentSessionStatusLabel(agentSession.runtime_status || agentSession.status, agentSession.recovery_required)
            : working.includes(member.participant_id)
              ? "응답 중"
              : memberStatusLabel(member),
          working: !agentSession && working.includes(member.participant_id),
          role,
          owner: false,
          active: agentSession
            ? agentSessionIsPresent(agentSession.runtime_status || agentSession.status)
            : working.includes(member.participant_id) || memberActive(member),
          muted: Boolean(member.muted),
          meetingId: member.room_id,
          ownerId: ownerId || undefined,
          ownerDisplayName: String(
            memberById.get(String(member.owner_id || ""))?.display_name ||
              ""
          ).trim() || undefined,
          avatarImage: resolveAttachmentReference(
            agentSession ? agentSession.avatar_image_url : member.avatar_image_url,
            displayResourceBase
          ),
          avatarReference: agentSession
            ? agentSession.avatar_image_url
            : member.avatar_image_url,
          providerKind: agentSession?.provider_kind,
          icon: ROLE_OPTIONS.find((option) => option.id === role)?.icon || typeMeta.icon,
        } satisfies MemberEntry;
      });
    return [...(human ? [human] : []), ...agentEntries, ...invitedEntries];
  }, [
    agentSessions,
    agents,
    displayResourceBase,
    members,
    viewerParticipantId,
    working,
  ]);

  return { entries, contextBadges };
}
