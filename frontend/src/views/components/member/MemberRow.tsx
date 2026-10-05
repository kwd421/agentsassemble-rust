import { useRef } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { VolumeX, Zap } from "lucide-react";
import { agentSessionPresenceStatus } from "../AgentSessionDetails";
import ParticipantAvatarContent from "../ParticipantAvatarContent";
import {
  isPrimaryActivationPointer,
  rowPointerMovedTooFar,
  rowTargetIsInteractive,
  ROLE_OPTIONS,
  statusDotClass,
} from "./memberHelpers";
import type { MemberEntry } from "./memberTypes";

export type MemberRowProps = {
  entry: MemberEntry;
  onOpenDetails: (entry: MemberEntry) => void;
  onContextMenu: (entry: MemberEntry, event: ReactMouseEvent<HTMLElement>) => void;
  canEditRoles: boolean;
  canManageParticipant?: boolean;
};

export default function MemberRow({
  entry,
  onOpenDetails,
  onContextMenu,
  canEditRoles, canManageParticipant,
}: MemberRowProps) {
  const canOpenDetails = Boolean(entry.agent || entry.agentSession);
  const Icon = entry.icon;
  const pointerStartRef = useRef<{ x: number; y: number } | null>(null);
  // Only an assigned room role is worth a tag; plain people and agents need none.
  const assignedRole = ["director", "implementer", "reviewer"].includes(entry.role)
    ? ROLE_OPTIONS.find((option) => option.id === entry.role)?.label
    : undefined;
  const isAgent = Boolean(entry.agent || entry.agentSession);
  // Like Discord's member list, a person is just a name; agents keep their model line.
  const showDetail = isAgent || !(entry.owner || entry.member?.participant_type === "human");
  // Without a model, an agent's status reads like a Discord activity line instead of a chip.
  const modelText = entry.modelLabel || entry.detail;
  // Roles and moderation open from the row's context menu. A row without a profile
  // button still takes focus, so the menu key and Shift+F10 reach it from the keyboard.
  const manageable = Boolean(canManageParticipant && (canEditRoles || (!entry.owner && entry.meetingId)));

  function openDetails() {
    if (canOpenDetails) onOpenDetails(entry);
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (!canOpenDetails || !isPrimaryActivationPointer(event) || rowTargetIsInteractive(event.target)) {
      pointerStartRef.current = null;
      return;
    }
    pointerStartRef.current = { x: event.clientX, y: event.clientY };
  }

  function handlePointerUp(event: ReactPointerEvent<HTMLDivElement>) {
    if (!canOpenDetails || !isPrimaryActivationPointer(event) || rowTargetIsInteractive(event.target)) return;
    const pointerStart = pointerStartRef.current;
    pointerStartRef.current = null;
    if (!pointerStart) return;
    if (rowPointerMovedTooFar(pointerStart, event)) return;
    openDetails();
  }

  function handlePointerCancel() {
    pointerStartRef.current = null;
  }

  return (
    <div
      className={canOpenDetails ? "dc-member group cursor-pointer" : "dc-member group"}
      tabIndex={manageable && !canOpenDetails ? 0 : undefined}
      data-role={entry.role}
      data-active={entry.active}
      data-ultra={entry.ultraMode}
      data-muted={entry.muted}
      onPointerDown={handlePointerDown}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
      onContextMenu={(event) => onContextMenu(entry, event)}
      onClick={(event) => {
        if (rowTargetIsInteractive(event.target)) return;
        openDetails();
      }}
    >
      <span className="relative shrink-0">
        <span className="dc-member-avatar">
          {entry.avatarImage ? (
            <img className="dc-member-avatar-image" src={entry.avatarImage} alt="" />
          ) : (
            <ParticipantAvatarContent
              participantType={entry.member?.participant_type}
              displayName={entry.displayName}
              avatarLabel={entry.member?.avatar_label}
              providerKind={entry.providerKind}
              size={32}
              fallback={<Icon size={15} />}
            />
          )}
        </span>
        <span
          className={`absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full border-2 border-sidebar ${
            statusDotClass(
              entry.agentSession
                ? agentSessionPresenceStatus(
                    entry.agentSession.runtime_status || entry.agentSession.status,
                    entry.agentSession.recovery_required
                  )
                : entry.agent?.status || entry.member?.status || "online"
            )
          }`}
          aria-hidden
        />
      </span>
      <div className="min-w-0 flex-1">
        <div className="dc-member-name-row">
          <p className="dc-member-name truncate preserve-words">
            {canOpenDetails ? <button type="button" className="truncate preserve-words cursor-pointer" style={{ maxWidth: "100%", textAlign: "left" }}
              aria-label={`${entry.displayName} 프로필 보기`} onClick={(event) => { event.stopPropagation(); openDetails(); }}>{entry.displayName}</button> : entry.displayName}
          </p>
          {assignedRole && <span className="dc-member-role-tag">{assignedRole}</span>}
          {entry.owner && (
            // canEditRoles is only true for the room host's own view, so the
            // host sees HOST on themselves while guests see YOU on themselves.
            <span
              className="rounded bg-accent/20 px-1 py-0.5 text-[9px] font-black text-accent"
              title={canEditRoles ? "이 방의 호스트(방장)" : "나"}
            >
              {canEditRoles ? "HOST" : "YOU"}
            </span>
          )}
          {entry.muted && (
            <span
              className="dc-member-muted-badge"
              title={`${entry.displayName}은(는) 뮤트되어 발언할 수 없어요`}
              aria-label="뮤트됨"
            >
              <VolumeX size={11} />
            </span>
          )}
        </div>
        {showDetail && <div className="dc-member-detail-row">
          <div
            className="dc-member-model-line"
            aria-label={memberModelAccessibleLabel(entry)}
            title={entry.fullDetail || entry.detail}
          >
            {entry.fastMode && (
              <Zap
                className="dc-member-fast-icon"
                size={11}
                fill="currentColor"
                aria-hidden
              />
            )}
            <span className="truncate preserve-words">
              {modelText || (isAgent ? entry.statusLabel : "")}
            </span>
            {entry.reasoningEffort && (
              <span
                className="dc-member-effort"
                data-ultra={entry.ultraMode}
              >
                {reasoningEffortLabel(entry.reasoningEffort)}
              </span>
            )}
          </div>
          {isAgent && modelText && entry.statusLabel && (
            <span
              className="dc-member-status-chip preserve-words"
              data-state={entry.active ? "active" : "idle"}
            >
              {entry.statusLabel}
            </span>
          )}
        </div>}
      </div>
    </div>
  );
}

function reasoningEffortLabel(value: string): string {
  const normalized = value.trim().toLowerCase();
  const labels: Record<string, string> = {
    low: "Low",
    medium: "Medium",
    high: "High",
    xhigh: "Extra High",
    max: "Max",
    ultra: "Ultra",
    ultracode: "UltraCode",
  };
  return labels[normalized] || value;
}

function memberModelAccessibleLabel(entry: MemberEntry): string {
  const parts = [entry.modelLabel || entry.detail];
  if (entry.fastMode) parts.push("Fast");
  if (entry.reasoningEffort) {
    parts.push(`추론 ${reasoningEffortLabel(entry.reasoningEffort)}`);
  }
  return parts.filter(Boolean).join(", ");
}
