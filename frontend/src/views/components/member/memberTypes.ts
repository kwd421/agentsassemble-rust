import type { AppIcon } from "../../../ui/icons";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../../api";
import type { ParticipantRole } from "../../../types/generated/ParticipantRole";

export type RoleId = ParticipantRole;

export type MemberSpeed = "fast" | "priority" | "flex";

/** What the AI profile card edits: an app AI's session, or an external AI's
 * room participant, keyed by its participant id. */
export type AgentProfileTarget = Pick<RoomAgentSession,
  "room_id" | "session_id" | "participant_id" | "display_name" | "avatar_image_url" | "provider_kind"> &
  Partial<Pick<RoomAgentSession, "reasoning_effort">>;

export type MemberEntry = {
  id: string;
  agent?: LiveAgent;
  agentSession?: RoomAgentSession;
  member?: RoomMember;
  displayName: string;
  detail: string;
  fullDetail?: string;
  modelLabel?: string;
  reasoningEffort?: string;
  speed?: MemberSpeed | null;
  ultraMode?: boolean;
  statusLabel?: string;
  /** An external AI the server says is working on a message. */
  working?: boolean;
  role: RoleId;
  owner: boolean;
  ownerId?: string;
  ownerDisplayName?: string;
  agentDisplayName?: string;
  avatarImage?: string;
  avatarReference?: string;
  providerKind?: string;
  active: boolean;
  muted: boolean;
  meetingId: string;
  icon: AppIcon;
};

/** An external AI joins through its own program, so it has a room participant and no session. */
export function externalAgentProfile(entry: MemberEntry): AgentProfileTarget | null {
  if (entry.agent || entry.agentSession || !entry.member) return null;
  return externalAgentProfileOf(entry.member);
}

/** External AIs get their participant id from the Room Connector join. */
export const EXTERNAL_AI_ID_PREFIX = "connector-";

/** The profile of an AI that joined through a Room Connector invite. */
export function externalAgentProfileOf(member: RoomMember): AgentProfileTarget | null {
  if (member.participant_type !== "agent" || !member.participant_id.startsWith(EXTERNAL_AI_ID_PREFIX)) return null;
  return {
    room_id: member.room_id,
    session_id: member.participant_id,
    participant_id: member.participant_id,
    display_name: member.display_name || member.participant_id,
    avatar_image_url: member.avatar_image_url || "",
    provider_kind: "",
  };
}
