import { Plus } from "lucide-react";
import {
  type ChannelNotificationSetting,
  type LiveAgent,
  type RoomMember,
  type RoomAgentSession,
} from "../../api";
import type { AgentQuotaVisibilityViewer } from "../../lib/agentQuotaVisibility";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import MemberList, { type RoleId } from "./MemberList";

type RoomSummary = {
  id: string;
  label: string;
  meetingId: string;
  topic: string;
  tone: string;
};

type RoomConnectionPanelProps = {
  room: RoomSummary;
  agents: LiveAgent[];
  members: RoomMember[];
  viewerParticipantId?: string;
  displayResourceBase?: string;
  onRoleChange?: (memberId: string, role: RoleId) => void;
  guestLocked?: boolean;
  channelNotifications?: Record<string, { notifications: ChannelNotificationSetting; lastReadAt?: string }>;
  onSessionActionComplete?: () => void;
  quotaViewer?: AgentQuotaVisibilityViewer;
  onStartAddAgent?: () => void;
  agentSessions?: RoomAgentSession[];
  capabilities?: Record<string, boolean>;
  onAgentControl?: (
    session: RoomAgentSession,
    action: "start" | "pause" | "stop" | "resume" | "interrupt"
  ) => void | Promise<void>;
  onParticipantMute?: (participantId: string, muted: boolean) => void | Promise<void>;
  availableProviders?: NativeCliProviderAvailability[];
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>
  ) => void | Promise<void>;
  agentActivityVisibility?: Record<string, boolean>;
  onAgentActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
};

function mutedChannelCount(
  channelNotifications?: RoomConnectionPanelProps["channelNotifications"]
): number {
  return Object.values(channelNotifications || {}).filter((setting) => setting.notifications === "mute").length;
}

export default function RoomConnectionPanel({
  room,
  agents,
  members,
  viewerParticipantId = "operator-local",
  displayResourceBase = "",
  onRoleChange,
  guestLocked = false,
  channelNotifications,
  onSessionActionComplete,
  quotaViewer,
  onStartAddAgent,
  agentSessions = [],
  capabilities = {},
  onAgentControl,
  onParticipantMute,
  availableProviders = [],
  onAgentConfigure,
  agentActivityVisibility = {},
  onAgentActivityVisibilityChange,
}: RoomConnectionPanelProps) {
  const mutedCount = mutedChannelCount(channelNotifications);

  return (
    <div className="dc-room-connection-panel">
      {!guestLocked && onStartAddAgent && (
        <div className="dc-room-agent-add-row">
          <button type="button" className="dc-agent-add-entry" onClick={onStartAddAgent}>
            <Plus size={16} />
            에이전트 추가
          </button>
          {mutedCount > 0 && <span className="dc-room-muted-count">{mutedCount} muted</span>}
        </div>
      )}
      <MemberList
        agents={agents}
        members={members}
        viewerParticipantId={viewerParticipantId}
        displayResourceBase={displayResourceBase}
        roomId={room.id}
        roomName={room.label}
        onRoleChange={onRoleChange}
        canEditRoles={Boolean(capabilities["room.manage"])}
        canModerate={Boolean(capabilities["participant.mute"])}
        onParticipantMute={capabilities["participant.mute"] ? onParticipantMute : undefined}
        onSessionActionComplete={onSessionActionComplete}
        quotaViewer={quotaViewer}
        hideSearch
        agentSessions={agentSessions}
        onAgentControl={capabilities["agent.control"] ? onAgentControl : undefined}
        availableProviders={availableProviders}
        onAgentConfigure={capabilities["agent.control"] ? onAgentConfigure : undefined}
        agentActivityVisibility={agentActivityVisibility}
        onAgentActivityVisibilityChange={onAgentActivityVisibilityChange}
      />
    </div>
  );
}
