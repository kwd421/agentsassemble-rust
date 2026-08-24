import { Bell, BellOff, Check, Settings } from "lucide-react";
import type { ChannelNotificationSetting, ChannelSettings } from "../../api";

const CHANNEL_NOTIFICATION_OPTIONS: Array<{
  value: ChannelNotificationSetting;
  label: string;
}> = [
  { value: "default", label: "서버 기본값" },
  { value: "all", label: "모든 메시지" },
  { value: "mentions", label: "@멘션만" },
  { value: "mute", label: "채널 알림 끄기" },
];

export default function ChannelContextMenu({
  channelLabel,
  settings,
  x,
  y,
  onMarkRead,
  onSetNotifications,
  onOpenSettings,
  preferenceStatus,
  preferenceError,
}: {
  channelLabel: string;
  settings?: ChannelSettings;
  x: number;
  y: number;
  onMarkRead: () => void;
  onSetNotifications: (value: ChannelNotificationSetting) => void;
  onOpenSettings: () => void;
  preferenceStatus: "loading" | "ready" | "saving" | "stale" | "error";
  preferenceError: string;
}) {
  const activeNotification = settings?.notifications || "default";
  const preferenceReady = preferenceStatus === "ready";
  const preferenceMessage =
    preferenceStatus === "loading"
      ? "채널 설정을 불러오는 중입니다."
      : preferenceStatus === "saving"
        ? "채널 설정을 저장하는 중입니다."
        : preferenceStatus === "stale"
          ? "저장하지 못해 마지막 서버 값으로 되돌렸습니다."
          : preferenceStatus === "error"
            ? "채널 설정을 불러오지 못했습니다."
            : "";

  return (
    <div
      className="dc-context-menu dc-channel-context-menu"
      style={{ left: x, top: y }}
      role="menu"
      aria-label={`${channelLabel} 채널 메뉴`}
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.preventDefault()}
    >
      <p className="dc-context-title preserve-words">#{channelLabel}</p>
      <button
        type="button"
        role="menuitem"
        onClick={onMarkRead}
        disabled={!preferenceReady}
        style={!preferenceReady ? { cursor: "not-allowed", opacity: 0.45 } : undefined}
      >
        <Check size={16} />
        읽음으로 표시하기
      </button>
      <span className="dc-context-separator" aria-hidden />
      <p className="dc-context-kicker">
        {activeNotification === "mute" ? <BellOff size={14} /> : <Bell size={14} />}
        채널 알림
      </p>
      {CHANNEL_NOTIFICATION_OPTIONS.map((option) => (
        <button
          key={option.value}
          type="button"
          role="menuitemradio"
          aria-checked={activeNotification === option.value}
          data-active={activeNotification === option.value}
          disabled={!preferenceReady}
          style={!preferenceReady ? { cursor: "not-allowed", opacity: 0.45 } : undefined}
          onClick={() => onSetNotifications(option.value)}
        >
          <span className="dc-context-radio-dot" aria-hidden />
          {option.label}
        </button>
      ))}
      {preferenceMessage && (
        <p
          className="preserve-words"
          role={preferenceStatus === "error" || preferenceStatus === "stale" ? "alert" : "status"}
          title={preferenceError || undefined}
          style={{ margin: "5px 8px 3px", color: "var(--color-text-muted)", fontSize: 11, lineHeight: 1.4 }}
        >
          {preferenceMessage}
        </p>
      )}
      <span className="dc-context-separator" aria-hidden />
      <button type="button" role="menuitem" onClick={onOpenSettings}>
        <Settings size={16} />
        채널 설정
      </button>
    </div>
  );
}
