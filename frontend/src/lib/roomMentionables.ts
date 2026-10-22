import type { RoomAgentSession, RoomMember } from "../api";
import type { Mentionable } from "./mentionComposerModel";
import { resolveAttachmentReference } from "./attachmentReference";
import { distinctNames } from "./distinctNames";

function clean(value: unknown) {
  return String(value || "").trim();
}

export function roomMentionables({
  viewerParticipantId,
  sessions,
  members,
  displayResourceBase,
}: {
  viewerParticipantId: string;
  sessions: RoomAgentSession[];
  members: RoomMember[];
  displayResourceBase: string;
}): Mentionable[] {
  const viewerId = clean(viewerParticipantId);
  const sessionByParticipantId = new Map(
    sessions.map((session) => [clean(session.participant_id), session])
  );
  const memberById = new Map(members.map((member) => [clean(member.participant_id), member]));
  const participantIds = new Map<string, string>();

  function append(participantIdValue: unknown) {
    const participantId = clean(participantIdValue);
    const key = participantId.toLowerCase();
    if (!participantId || participantId === viewerId || participantIds.has(key)) return;
    participantIds.set(key, participantId);
  }

  members.forEach((member) => append(member.participant_id));
  const viewerDisplayName = clean(
    memberById.get(viewerId)?.display_name
  );
  const labels = distinctNames(
    Array.from(participantIds.values(), (participantId) => {
      const session = sessionByParticipantId.get(participantId);
      return {
        id: participantId,
        name: clean(session ? session.display_name : memberById.get(participantId)?.display_name),
      };
    }),
    viewerDisplayName ? [viewerDisplayName] : [],
  );

  function mentionableFor(participantId: string): Mentionable {
    const member = memberById.get(participantId);
    if (
      !member ||
      (member.participant_type !== "human" && member.participant_type !== "agent")
    ) {
      throw new Error("Room mention participant has an unsupported canonical type.");
    }
    const session = member.participant_type === "agent"
      ? sessionByParticipantId.get(participantId)
      : undefined;
    // Room Connector is an agent participant without a managed Agent Session.
    // Its canonical member identity remains sufficient for ordinary mentions.
    const participantKind = member.participant_type;
    const ownerId = clean(member.owner_id);
    const ownerDisplayName = clean(memberById.get(ownerId)?.display_name);
    return {
      token: participantId,
      label: labels.get(participantId) || participantId,
      avatarImage: session
        ? resolveAttachmentReference(session.avatar_image_url, displayResourceBase)
        : resolveAttachmentReference(member.avatar_image_url, displayResourceBase),
      participantKind,
      providerKind: clean(session?.provider_kind) || undefined,
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
