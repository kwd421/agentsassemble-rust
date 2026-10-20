import ResourceImage from "../ResourceImage";
import { executionLocationLabel, reasoningEffortLabel } from "../../../lib/agentLabels";
import { useRef } from "react";
import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from "react";
import { Coins, VolumeX, Zap } from "../../../ui/icons";
import { agentSessionPresenceStatus } from "../AgentSessionDetails";
import ParticipantAvatarContent from "../ParticipantAvatarContent";
import GalaxyAvatar from "./GalaxyAvatar";
import {
  isGalaxyAgent,
  isPrimaryActivationPointer,
  rowPointerMovedTooFar,
  rowTargetIsInteractive,
  ROLE_OPTIONS,
  statusDotClass,
} from "./memberHelpers";
import type { MemberEntry } from "./memberTypes";

export type MemberRowProps = {
  entry: MemberEntry;
  onOpenDetails: (entry: MemberEntry, anchor: DOMRect) => void;
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
  // Agents open their detail window; people open a profile card, as in Discord.
  const canOpenDetails = Boolean(entry.agent || entry.agentSession || entry.member);
  const rowRef = useRef<HTMLDivElement>(null);
  const Icon = entry.icon;
  const executionLocation = executionLocationLabel(entry.agentSession);
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
  const SpeedIcon = entry.speed === "flex" ? Coins : entry.speed ? Zap : undefined;
  const galaxy = isGalaxyAgent(entry.agentSession);
  // Roles and moderation open from the row's context menu. A row without a profile
  // button still takes focus, so the menu key and Shift+F10 reach it from the keyboard.
  const manageable = Boolean(canManageParticipant && (canEditRoles || (!entry.owner && entry.meetingId)));

  function openDetails() {
    if (canOpenDetails && rowRef.current) onOpenDetails(entry, rowRef.current.getBoundingClientRect());
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

  const avatar = (
    <span className="dc-member-avatar">
      {entry.avatarImage ? (
        <ResourceImage className="dc-member-avatar-image" src={entry.avatarImage} alt="" />
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
  );

  return (
    <div
      ref={rowRef}
      className={canOpenDetails ? "dc-member group cursor-pointer" : "dc-member group"}
      tabIndex={manageable && !canOpenDetails ? 0 : undefined}
      data-role={entry.role}
      data-active={entry.active}
      data-ultra={entry.ultraMode}
      data-galaxy={galaxy || undefined}
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
        {galaxy ? <GalaxyAvatar size={32}>{avatar}</GalaxyAvatar> : avatar}
        <span
          className={`absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full border-2 border-chat-bg ${
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
            // host sees 방장 on themselves while other viewers see 나 on themselves.
            <span
              className="rounded bg-accent/20 px-1 py-0.5 text-[10px] font-bold leading-none tracking-wide text-accent"
            >
              {canEditRoles ? "방장" : "나"}
            </span>
          )}
          {entry.muted && (
            <span
              className="dc-member-muted-badge"
              title={`${entry.displayName}은(는) 음소거되어 발언할 수 없어요`}
              aria-label="음소거됨"
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
            {SpeedIcon && (
              <SpeedIcon
                className="dc-member-fast-icon"
                size={11}
                fill="currentColor"
                aria-hidden
              />
            )}
            <span className="truncate preserve-words">
              {modelText || (isAgent ? entry.statusLabel : "")}
            </span>
            {entry.reasoningEffort && !isUltraEffort(entry.reasoningEffort) && (
              <span className="dc-member-effort">
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
        {executionLocation && <p className="dc-member-detail-row preserve-words">{executionLocation}</p>}
      </div>
    </div>
  );
}

// Ultra's galaxy already communicates the tier; the text chip would be a
// second, redundant badge beside the model line.
function isUltraEffort(effort: string): boolean {
  return effort.trim().toLowerCase() === "ultra";
}

function memberModelAccessibleLabel(entry: MemberEntry): string {
  const parts = [entry.modelLabel || entry.detail];
  if (entry.speed === "fast") parts.push("Fast");
  else if (entry.speed === "priority") parts.push("Priority");
  else if (entry.speed === "flex") parts.push("Flex");
  if (entry.reasoningEffort) {
    parts.push(`추론 ${reasoningEffortLabel(entry.reasoningEffort)}`);
  }
  return parts.filter(Boolean).join(", ");
}
