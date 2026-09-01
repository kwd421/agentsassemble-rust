import { useMemo, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { Bot, Search, Volume2, VolumeX } from "lucide-react";
import type {
  LiveAgent,
  RoomAgentSession,
  RoomMember,
} from "../../api";

import type {
  AgentQuotaVisibilityViewer,
} from "../../lib/agentQuotaVisibility";
import ProviderTruthChips from "./ProviderTruthChips";
import type {
  AgentSessionControlAction,
} from "./AgentSessionDetails";
import MemberDetailModal from "./member/MemberDetailModal";
import MemberRow from "./member/MemberRow";
import { buildMemberOwnerGroups } from "./member/memberOwnerGroups";
import { useMemberEntries } from "./member/useMemberEntries";
import type { MemberEntry, RoleId } from "./member/memberTypes";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";

export type { RoleId };

export default function MemberList({
  agents,
  members = [],
  viewerParticipantId = "operator-local",
  displayResourceBase = "",
  roomName,
  onRoleChange,
  canEditRoles = true,
  onSessionActionComplete,
  quotaViewer,
  searchQuery,
  onSearchQueryChange,
  hideSearch = false,
  canModerate = false,
  onParticipantMute,
  agentSessions = [],
  onAgentControl,
  availableProviders = [],
  onAgentConfigure,
  agentActivityVisibility = {},
  onAgentActivityVisibilityChange,
}: {
  agents: LiveAgent[];
  members?: RoomMember[];
  viewerParticipantId?: string;
  displayResourceBase?: string;
  roomId: string;
  roomName: string;
  onRoleChange?: (memberId: string, role: RoleId) => void | Promise<void>;
  canEditRoles?: boolean;
  onSessionActionComplete?: () => void;
  quotaViewer?: AgentQuotaVisibilityViewer;
  searchQuery?: string;
  onSearchQueryChange?: (query: string) => void;
  hideSearch?: boolean;
  canModerate?: boolean;
  onParticipantMute?: (participantId: string, muted: boolean) => void | Promise<void>;
  agentSessions?: RoomAgentSession[];
  onAgentControl?: (
    session: RoomAgentSession,
    action: AgentSessionControlAction
  ) => void | Promise<void>;
  availableProviders?: NativeCliProviderAvailability[];
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>
  ) => void | Promise<void>;
  agentActivityVisibility?: Record<string, boolean>;
  onAgentActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
}) {
  const [localQuery, setLocalQuery] = useState("");
  const [detailEntryId, setDetailEntryId] = useState("");
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});
  const [memberMenu, setMemberMenu] = useState<{ x: number; y: number; entry: MemberEntry } | null>(null);
  const [muteBusy, setMuteBusy] = useState(false);
  const [roleChangeError, setRoleChangeError] = useState("");
  const query = searchQuery ?? localQuery;
  const { entries, contextBadges } = useMemberEntries({
    agents,
    members,
    viewerParticipantId,
    displayResourceBase,
    agentSessions,
    quotaViewer,
    canEditRoles,
  });
  const detailEntry = useMemo(
    () => entries.find((entry) => entry.id === detailEntryId) || null,
    [detailEntryId, entries]
  );
  const ownerGroups = useMemo(
    () => buildMemberOwnerGroups(entries, viewerParticipantId, query),
    [entries, query, viewerParticipantId]
  );

  function handleMemberContextMenu(entry: MemberEntry, event: ReactMouseEvent<HTMLElement>) {
    // Host-only moderation: right-clicking a participant opens the mute menu.
    // Self and any participant without a room scope can't be muted.
    if (
      !canModerate ||
      !onParticipantMute ||
      entry.owner ||
      !entry.meetingId
    ) return;
    event.preventDefault();
    setMemberMenu({ x: event.clientX, y: event.clientY, entry });
  }

  async function handleToggleMute(entry: MemberEntry) {
    if (!entry.meetingId || !onParticipantMute) return;
    setMuteBusy(true);
    try {
      await onParticipantMute(entry.id, !entry.muted);
      onSessionActionComplete?.();
    } catch (error) {
      window.alert(`뮤트 변경 실패: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      setMuteBusy(false);
      setMemberMenu(null);
    }
  }

  async function handleRoleChange(memberId: string, role: RoleId) {
    if (onRoleChange) {
      setRoleChangeError("");
      try {
        await onRoleChange(memberId, role);
      } catch (error) {
        setRoleChangeError(
          error instanceof Error ? error.message : "역할을 변경하지 못했습니다."
        );
      }
      return;
    }
    setRoleChangeError("방 역할 권위를 사용할 수 없습니다.");
  }

  function toggleGroup(groupId: string) {
    setCollapsedGroups((previous) => ({ ...previous, [groupId]: !previous[groupId] }));
  }

  function openMemberDetails(entry: MemberEntry) {
    setDetailEntryId(entry.id);
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {!hideSearch && (
      <div className="dc-member-search shrink-0">
        <label className="dc-member-search-box">
          <span className="sr-only">{roomName} 멤버 검색</span>
          <input
            type="search"
            value={query}
            onChange={(event) => {
              const nextQuery = event.target.value;
              if (onSearchQueryChange) {
                onSearchQueryChange(nextQuery);
              } else {
                setLocalQuery(nextQuery);
              }
            }}
            placeholder={`${roomName} 검색`}
          />
          <Search size={15} aria-hidden />
        </label>
      </div>
      )}
      {roleChangeError && (
        <p className="dc-room-play-error mx-2 mt-2 preserve-words" role="alert">
          {roleChangeError}
        </p>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-3 chat-scroll">
        {agents.length === 0 && members.length === 0 && (
          <p className="mb-2 px-2 text-[13px] text-text-muted preserve-words">
            {roomName}에는 아직 멤버가 없습니다.
          </p>
        )}
        {ownerGroups.map((group) => {
          const collapseId = `owner:${group.id}`;
          return (
            <section key={group.id} className="dc-person-member-group">
              {group.person ? (
                <MemberRow
                  entry={group.person}
                  onOpenDetails={openMemberDetails}
                  onRoleChange={handleRoleChange}
                  onContextMenu={handleMemberContextMenu}
                  canEditRoles={canEditRoles}
                />
              ) : (
                <p className="dc-person-owner-label preserve-words">{group.label}</p>
              )}
              {group.agents.length > 0 && (
                <details
                  className="dc-owner-agent-group"
                  open={!collapsedGroups[collapseId]}
                  onToggle={(event) => {
                    const open = event.currentTarget.open;
                    setCollapsedGroups((previous) => ({ ...previous, [collapseId]: !open }));
                  }}
                >
                  <summary
                    className="dc-owner-agent-heading"
                    onClick={(event) => {
                      event.preventDefault();
                      toggleGroup(collapseId);
                    }}
                  >
                    <Bot size={12} />
                    에이전트 — {group.agents.length}
                  </summary>
                  <div className="dc-owner-agent-list">
                    {group.agents.map((entry) => (
                      <MemberRow
                        key={entry.id}
                        entry={entry}
                        onOpenDetails={openMemberDetails}
                        onRoleChange={handleRoleChange}
                        onContextMenu={handleMemberContextMenu}
                        canEditRoles={canEditRoles}
                      />
                    ))}
                  </div>
                </details>
              )}
            </section>
          );
        })}
        {contextBadges.length > 0 && (
          <details className="dc-member-context mt-3 px-2" aria-label="참가자 맥락 요약">
            <summary className="cursor-pointer list-none text-[11px] font-bold text-text-muted hover:text-text-secondary">
              고급 연결 요약
            </summary>
            <ProviderTruthChips badges={contextBadges} compact />
          </details>
        )}
      </div>
      {detailEntry && (
        <MemberDetailModal
          entry={detailEntry}
          onClose={() => setDetailEntryId("")}
          onAgentControl={onAgentControl}
          availableProviders={availableProviders}
          onAgentConfigure={onAgentConfigure}
          activityVisible={
            detailEntry.agentSession
              ? agentActivityVisibility[detailEntry.agentSession.participant_id] === true
              : false
          }
          onActivityVisibilityChange={onAgentActivityVisibilityChange}
        />
      )}
      {memberMenu && (
        <>
          <div
            className="dc-member-menu-backdrop"
            role="presentation"
            onClick={() => setMemberMenu(null)}
            onContextMenu={(event) => {
              event.preventDefault();
              setMemberMenu(null);
            }}
          />
          <div
            className="dc-member-context-menu"
            role="menu"
            style={{ top: memberMenu.y, left: memberMenu.x }}
          >
            <p className="dc-member-context-menu-title preserve-words">{memberMenu.entry.displayName}</p>
            {onParticipantMute && (
              <button
                type="button"
                role="menuitem"
                className="dc-member-context-menu-item"
                disabled={muteBusy}
                onClick={() => void handleToggleMute(memberMenu.entry)}
              >
                {memberMenu.entry.muted ? <Volume2 size={14} /> : <VolumeX size={14} />}
                {memberMenu.entry.muted ? "뮤트 해제" : "뮤트"}
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}
