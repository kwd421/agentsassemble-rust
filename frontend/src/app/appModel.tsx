import { Hash, LoaderCircle, type LucideIcon } from "lucide-react";
import type {
  ChannelNotificationSetting,
  ChannelSettings,
  LiveAgent,
  ProviderUsageId,
  ProviderUsageSnapshot,
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

export async function copyText(value: string) {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(value);
      return true;
    }
  } catch {
    // Fall through to the textarea path when browser permissions reject clipboard writes.
  }
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.select();
  textarea.setSelectionRange(0, value.length);
  const copied = document.execCommand("copy");
  textarea.remove();
  return copied;
}

export function agentSessionMemberToLiveAgent(
  member: RoomMember,
  session?: RoomAgentSession,
  usage?: ProviderUsageSnapshot,
  usageSupported = false
): LiveAgent {
  return {
    agent_id: member.participant_id,
    display_name: member.display_name || member.participant_id,
    avatar_image_url: member.avatar_image_url,
    owner_id: member.owner_id,
    created_by: member.created_by,
    status: member.thinking ? "working" : member.status || member.session_status || "online",
    provider_kind: member.provider_kind || "agent_session",
    connection_kind: member.connection_kind || "agent_session",
    engagement_mode: member.engagement_mode || "agent_session",
    meeting_id: member.meeting_id,
    session_id: member.session_id || member.participant_id,
    model_id: session?.model || member.model_id,
    effort: session?.reasoning_effort || member.effort,
    speed: session?.service_tier,
    fast_mode: ["fast", "priority"].includes(
      String(session?.service_tier || "").toLowerCase()
    ),
    permission_option: member.permission_option,
    sandbox_enforcement: member.sandbox_enforcement || "",
    join_semantics: member.join_semantics || "agent_session",
    execution_mode: member.execution_mode || "agent_session_app_server",
    last_seen_at: member.last_seen_at || member.updated_at,
    last_reply_at: member.updated_at,
    quota_5h: usage?.quota_5h,
    quota_1w: usage?.quota_1w,
    quota_state: usage?.quota_state,
    quota_status: usage?.status || (usageSupported ? "loading" : "unsupported"),
    quota_windows: usage?.quota_windows,
    account_available: usage?.account_available,
    account_balances: usage?.account_balances,
    capabilities: [],
  };
}

export function providerUsageTarget(session?: RoomAgentSession) {
  if (!session) return null;
  const providerByKind: Partial<Record<string, ProviderUsageId>> = {
    claude_code: "claude",
    codex_live_session: "codex",
    antigravity_live_session: "antigravity",
    grok_live_session: "grok",
    deepseek_api: "deepseek",
    opencode_server: "opencode",
  };
  const providerId = providerByKind[session.provider_kind];
  if (!providerId) return null;
  const model =
    providerId === "codex" || providerId === "antigravity" || providerId === "opencode"
      ? String(session.model || "").trim()
      : "";
  return {
    providerId,
    model,
    key: `${providerId}:${model.toLocaleLowerCase()}`,
  };
}
