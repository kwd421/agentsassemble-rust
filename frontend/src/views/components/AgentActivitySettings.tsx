import { Brain } from "lucide-react";

import type { RoomAgentSession } from "../../api";

export default function AgentActivitySettings({
  session,
  activityVisible,
  onActivityVisibilityChange,
}: {
  session: RoomAgentSession;
  activityVisible: boolean;
  onActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
}) {
  return (
    <ActivitySwitch
      label="생각과 작업 표시"
      description="공개용 생각 요약과 안전하게 정리된 도구 활동만 표시합니다."
      valueLabel={activityVisible ? "켜짐" : "꺼짐"}
      enabled={activityVisible}
      disabled={!onActivityVisibilityChange}
      onToggle={() => onActivityVisibilityChange?.(session, !activityVisible)}
    />
  );
}

function ActivitySwitch({
  label,
  description,
  valueLabel,
  enabled,
  disabled,
  onToggle,
}: {
  label: string;
  description: string;
  valueLabel: string;
  enabled: boolean;
  disabled: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="dc-agent-activity-setting">
      <div className="dc-agent-activity-copy">
        <div>
          <Brain size={15} aria-hidden />
          <span>{label}</span>
        </div>
        <p>{description}</p>
      </div>
      <button
        type="button"
        className="dc-agent-activity-toggle"
        role="switch"
        aria-label={label}
        aria-checked={enabled}
        data-on={enabled}
        disabled={disabled}
        onClick={onToggle}
      >
        <span className="dc-agent-activity-switch" aria-hidden="true">
          <i />
        </span>
        <span>{valueLabel}</span>
      </button>
    </div>
  );
}
