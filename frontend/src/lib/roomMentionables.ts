import type { Mentionable } from "./mentionComposerModel";
import { resolveAttachmentReference } from "./attachmentReference";

type AgentMentionIdentity = {
  agent_id: string;
  display_name?: string;
  avatar_image_url?: string;
  owner_id?: string;
  owner_display_name?: string;
  provider_kind?: string;
};

type MemberMentionIdentity = {
  participant_id: string;
  display_name?: string;
  avatar_image_url?: string;
  owner_id?: string;
  participant_type?: string;
  provider_kind?: string;
};

function clean(value: unknown) {
  return String(value || "").trim();
}

export function roomMentionables({
  viewerParticipantId,
  agents,
  members,
  displayResourceBase,
}: {
  viewerParticipantId: string;
  agents: AgentMentionIdentity[];
  members: MemberMentionIdentity[];
  displayResourceBase: string;
}): Mentionable[] {
  const viewerId = clean(viewerParticipantId);
  const agentById = new Map(agents.map((agent) => [clean(agent.agent_id), agent]));
  const memberById = new Map(members.map((member) => [clean(member.participant_id), member]));
  const participantIds = new Map<string, string>();

  function append(participantIdValue: unknown) {
    const participantId = clean(participantIdValue);
    const key = participantId.toLowerCase();
    if (!participantId || participantId === viewerId || participantIds.has(key)) return;
    participantIds.set(key, participantId);
  }

  agents.forEach((agent) => append(agent.agent_id));
  members.forEach((member) => append(member.participant_id));
  const viewerDisplayName = clean(
    memberById.get(viewerId)?.display_name
  );
  const displayNameCounts = new Map<string, number>();
  participantIds.forEach((participantId) => {
    const agent = agentById.get(participantId);
    const displayName = clean(
      agent
        ? agent.display_name
        : memberById.get(participantId)?.display_name
    );
    const key = displayName.toLowerCase();
    if (key) displayNameCounts.set(key, (displayNameCounts.get(key) || 0) + 1);
  });

  function mentionableFor(participantId: string): Mentionable {
    const agent = agentById.get(participantId);
    const member = memberById.get(participantId);
    const displayName = clean(agent ? agent.display_name : member?.display_name);
    const uniqueDisplayName =
      displayName && displayNameCounts.get(displayName.toLowerCase()) === 1;
    const participantKind =
      agent || (member?.participant_type && member.participant_type !== "human")
        ? "agent"
        : "human";
    const ownerId = clean(member?.owner_id || agent?.owner_id);
    const ownerDisplayName = clean(
      agent?.owner_display_name || memberById.get(ownerId)?.display_name
    );
    return {
      token: participantId,
      label: uniqueDisplayName
        ? displayName
        : displayName
          ? `${displayName} · ${participantId}`
          : participantId,
      avatarImage: resolveAttachmentReference(
        agent ? agent.avatar_image_url : member?.avatar_image_url,
        displayResourceBase
      ),
      participantKind,
      providerKind: clean(agent ? agent.provider_kind : member?.provider_kind) || undefined,
      detail:
        participantKind === "agent"
          ? ownerDisplayName
            ? `${ownerDisplayName}의 에이전트`
            : "에이전트"
          : "사람",
    };
  }

  return [
    ...(viewerId && viewerDisplayName
      ? [{
          token: viewerId,
          label: viewerDisplayName,
          avatarImage: resolveAttachmentReference(
            memberById.get(viewerId)?.avatar_image_url,
            displayResourceBase
          ),
          participantKind: "human" as const,
          detail: "사람",
        }]
      : []),
    ...Array.from(participantIds.values(), mentionableFor),
  ];
}
