import { Hash, LoaderCircle, type LucideIcon } from "lucide-react";
import type {
  ChannelNotificationSetting,
  ChannelSettings,
  LiveAgent,
  RoomAgentSession,
  RoomMember,
} from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";

export type Channel = "lobby";

export type ChannelConfig = {
  id: Channel;
  label: string;
  icon: LucideIcon;
};

export type ChannelMenuState = {
  channelId: Channel;
  x: number;
  y: number;
} | null;

export type RoomSettingsSectionId =
  | "settings-overview"
  | "settings-appearance"
  | "settings-channels"
  | "settings-notify"
  | "settings-invite";

export type RoomSettingsState = {
  roomId: string;
  initialSectionId?: RoomSettingsSectionId;
} | null;

export function DeferredViewFallback() {
  return (
    <div
      className="flex min-h-0 flex-1 items-center justify-center text-text-muted"
      role="status"
      aria-label="화면 불러오는 중"
    >
      <LoaderCircle className="animate-spin" size={22} aria-hidden="true" />
    </div>
  );
}

export const CHANNELS: ChannelConfig[] = [
  { id: "lobby", label: "general", icon: Hash },
];

export const CHANNEL_SECTIONS: Array<{
  id: string;
  label: string;
  channels: Channel[];
}> = [{ id: "conversation", label: "Text Channels", channels: ["lobby"] }];

const CHANNEL_NOTIFICATION_LABELS: Record<ChannelNotificationSetting, string> = {
  default: "서버 기본 알림",
  all: "모든 메시지 알림",
  mentions: "@멘션만 알림",
  mute: "알림 끔",
};

export const EMPTY_ROOM: RoomDockItem = {
  id: "no-room",
  label: "방 없음",
  meetingId: "",
  topic: "새 방을 만들어 대화를 시작하세요.",
  shortLabel: "",
  icon: Hash,
  createdAt: "",
  tone: "fresh",
};

export function channelNotificationSummary(setting?: ChannelSettings): string {
  return `현재 알림: ${CHANNEL_NOTIFICATION_LABELS[setting?.notifications || "default"]}`;
}

export function channelLastReadSummary(setting?: ChannelSettings): string {
  if (!setting?.lastReadAt) return "아직 이 채널을 읽음으로 표시하지 않았습니다.";
  if (setting.lastReadAt.startsWith("seq:")) {
    return "이 채널의 읽음 위치가 기기 간 동기화됩니다.";
  }
  try {
    const readAt = new Date(setting.lastReadAt).toLocaleString("ko-KR", {
      dateStyle: "short",
      timeStyle: "short",
    });
    return `마지막 읽음 표시: ${readAt}`;
  } catch {
    return "마지막 읽음 표시 시간이 올바르지 않습니다.";
  }
}

export function agentSessionMemberToLiveAgent(
  member: RoomMember,
  session: RoomAgentSession
): LiveAgent {
  const status = session.runtime_status === "busy" ||
    session.runtime_status === "starting" ||
    session.runtime_status === "stopping"
    ? "working"
    : session.runtime_status === "idle"
      ? "online"
      : session.runtime_status === "paused" || session.runtime_status === "available"
        ? "idle"
        : session.runtime_status === "error"
          ? "error"
          : "offline";
  return {
    agent_id: member.participant_id,
    display_name: session.display_name,
    owner_id: member.owner_id,
    owner_participant_id: member.owner_id,
    status,
    provider_kind: session.provider_kind,
    connection_kind: session.connection_kind,
    meeting_id: member.room_id,
    session_id: session.session_id,
    model_id: session.model,
    effort: session.reasoning_effort,
    speed: session.service_tier,
    fast_mode: ["fast", "priority"].includes(
      session.service_tier.toLowerCase()
    ),
    permission_option: session.permission_mode,
    persona_card_id: session.persona_card_id,
    execution_mode: session.execution_harness,
  };
}
