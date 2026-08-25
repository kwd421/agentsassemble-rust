import type { LucideIcon } from "lucide-react";
import type { LiveAgent, RoomAgentSession, RoomMember } from "../../../api";
import type { ParticipantRole } from "../../../types/generated/ParticipantRole";

export type RoleId = ParticipantRole;

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
  fastMode?: boolean;
  ultraMode?: boolean;
  statusLabel?: string;
  role: RoleId;
  owner: boolean;
  ownedByViewer: boolean;
  ownerId?: string;
  ownerDisplayName?: string;
  agentDisplayName?: string;
  avatarImage?: string;
  providerKind?: string;
  active: boolean;
  muted: boolean;
  meetingId: string;
  canViewQuota: boolean;
  icon: LucideIcon;
};
