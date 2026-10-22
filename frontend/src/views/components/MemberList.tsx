import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { Check, ChevronDown, ChevronRight, Pause, Play, Search, Square, Volume2, VolumeX } from "../../ui/icons";
import type {
  LiveAgent,
  RoomAgentSession,
  RoomMember,
} from "../../api";

import ProviderTruthChips from "./ProviderTruthChips";
import type {
  AgentSessionControlAction,
} from "./AgentSessionDetails";
import { agentSessionCanBulkStop, agentSessionCanPause, agentSessionCanResume } from "./AgentSessionDetails";
import MemberDetailModal from "./member/MemberDetailModal";
import MemberProfileCard from "./member/MemberProfileCard";
import ParticipantRemovalControls, { type ParticipantRemovalAction } from "./member/ParticipantRemovalControls";
import MemberRow from "./member/MemberRow";
import { ROLE_OPTIONS } from "./member/memberHelpers";
import { buildMemberOwnerGroups } from "./member/memberOwnerGroups";
import { useMemberEntries } from "./member/useMemberEntries";
import { externalAgentProfile, type MemberEntry, type RoleId, type AgentProfileTarget } from "./member/memberTypes";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { userFacingError } from "../../lib/apiErrors";
import { useRoomActivity } from "../../app/roomActivity";

export type { RoleId };

const BULK_FAILURE_VERB = { pause: "일시정지", resume: "재개", stop: "중지" } as const;

export default function MemberList({
  agents,
  members = [],
  viewerParticipantId = "operator-local",
  displayResourceBase = "",
  roomName,
  disconnected = false,
  loading = false,
  onRoleChange,
  canEditRoles = true,
  onSessionActionComplete,
  searchQuery,
  onSearchQueryChange,
  hideSearch = false,
  canModerate = false,
  onParticipantMute, onParticipantRemove,
  agentSessions = [],
  onAgentControl,
  availableProviders = [],
  localProviderActions = false,
  onAgentConfigure,
  onAgentProfileUpdate,
  onAgentAvatarUpdate,
  agentActivityVisibility = {},
  onAgentActivityVisibilityChange,
}: {
  agents: LiveAgent[];
  members?: RoomMember[];
  viewerParticipantId?: string;
  displayResourceBase?: string;
  roomId: string;
  roomName: string;
  /** The room's server is unreachable, so the member list is unknown rather than empty. */
  disconnected?: boolean;
  /** The room's first snapshot has not arrived yet: members are unknown, not absent. */
  loading?: boolean;
  onRoleChange?: (memberId: string, role: RoleId) => void | Promise<void>;
  canEditRoles?: boolean;
  onSessionActionComplete?: () => void;
  searchQuery?: string;
  onSearchQueryChange?: (query: string) => void;
  hideSearch?: boolean;
  canModerate?: boolean;
  onParticipantRemove?: ParticipantRemovalAction;
  onParticipantMute?: (participantId: string, muted: boolean) => void | Promise<void>;
  agentSessions?: RoomAgentSession[];
  onAgentControl?: (
    session: RoomAgentSession,
    action: AgentSessionControlAction
  ) => void | Promise<void>;
  availableProviders?: NativeCliProviderAvailability[];
  localProviderActions?: boolean;
  onAgentAvatarUpdate?: (session: AgentProfileTarget, file: File, displayName: string, signal: AbortSignal) => Promise<void>;
  onAgentProfileUpdate?: (session: AgentProfileTarget, settings: Record<string, string>) => void | Promise<void>;
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>,
    fullAccessAcknowledged?: true
  ) => void | Promise<void>;
  agentActivityVisibility?: Record<string, boolean>;
  onAgentActivityVisibilityChange?: (session: RoomAgentSession, visible: boolean) => void;
}) {
  const [localQuery, setLocalQuery] = useState("");
  const [detailEntryId, setDetailEntryId] = useState("");
  const [profileCard, setProfileCard] = useState<{ entryId: string; anchor: DOMRect } | null>(null);
  const [collapsedGroups, setCollapsedGroups] = useState<Record<string, boolean>>({});
  const [memberMenu, setMemberMenu] = useState<{ x: number; y: number; entry: MemberEntry } | null>(null);
  const [muteBusy, setMuteBusy] = useState(false);
  const [roleChangeError, setRoleChangeError] = useState("");
  const [groupControlBusy, setGroupControlBusy] = useState<Record<string, boolean>>({});
  const [groupControlError, setGroupControlError] = useState<Record<string, string>>({});
  const query = searchQuery ?? localQuery;
  const memberMenuOpen = memberMenu !== null;
  // A right-click leaves focus on the row, so Escape is handled at the window like other popouts.
  useEffect(() => {
    if (!memberMenuOpen) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setMemberMenu(null);
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [memberMenuOpen]);
  const working = useRoomActivity();
  const { entries, contextBadges } = useMemberEntries({
    agents,
    members,
    viewerParticipantId,
    displayResourceBase,
    agentSessions,
    working,
  });
  const profileCardEntry = profileCard ? entries.find((entry) => entry.id === profileCard.entryId) || null : null;
  const closeProfileCard = useCallback(() => setProfileCard(null), []);
  const detailEntry = useMemo(
    () => entries.find((entry) => entry.id === detailEntryId) || null,
    [detailEntryId, entries]
  );
  const ownerGroups = useMemo(
    () => buildMemberOwnerGroups(entries, query),
    [entries, query]
  );
  // Discord's member list is split into 온라인 / 오프라인 sections; a person's
  // agents stay under that person, and the section follows the person (or,
  // for an ownerless group, any active agent).
  const memberSections = useMemo(() => {
    const online = ownerGroups.filter((group) => group.person ? group.person.active : group.agents.some((agent) => agent.active));
    const offline = ownerGroups.filter((group) => !online.includes(group));
    const count = (groups: typeof ownerGroups) => groups.reduce((total, group) => total + (group.person ? 1 : 0) + group.agents.length, 0);
    return [
      { label: "온라인", groups: online, count: count(online) },
      { label: "오프라인", groups: offline, count: count(offline) },
    ];
  }, [ownerGroups]);

  const roleMenuAvailable = Boolean(canEditRoles && onRoleChange);

  function canModerateEntry(entry: MemberEntry) {
    return Boolean(
      (canModerate || onParticipantRemove) &&
      (onParticipantMute || onParticipantRemove) &&
      !entry.owner &&
      entry.meetingId
    );
  }

  function handleMemberContextMenu(entry: MemberEntry, event: ReactMouseEvent<HTMLElement>) {
    // Room roles and moderation share one context menu, as Discord's member menu does.
    if (!roleMenuAvailable && !canModerateEntry(entry)) return;
    event.preventDefault();
    const anchor = event.currentTarget.getBoundingClientRect();
    const x = event.clientX || anchor.right;
    const y = event.clientY || anchor.bottom;
    setMemberMenu({ x: Math.max(16, Math.min(x, window.innerWidth - 256)), y: Math.max(16, Math.min(y, window.innerHeight - 180)), entry });
  }

  async function handleToggleMute(entry: MemberEntry) {
    if (!entry.meetingId || !onParticipantMute) return;
    setMuteBusy(true);
    try {
      await onParticipantMute(entry.id, !entry.muted);
      onSessionActionComplete?.();
    } catch (error) {
      window.alert(userFacingError(error, "음소거를 변경하지 못했어요."));
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
          userFacingError(error, "역할을 변경하지 못했어요.")
        );
      }
      return;
    }
    setRoleChangeError("방 역할 권위를 사용할 수 없습니다.");
  }

  function toggleGroup(groupId: string) {
    setCollapsedGroups((previous) => ({ ...previous, [groupId]: !previous[groupId] }));
  }

  async function runGroupControl(groupId: string, sessions: RoomAgentSession[], action: "pause" | "resume" | "stop") {
    if (!onAgentControl || groupControlBusy[groupId] || sessions.length === 0) return;
    setGroupControlBusy((previous) => ({ ...previous, [groupId]: true }));
    setGroupControlError((previous) => ({ ...previous, [groupId]: "" }));
    try {
      const outcomes = await Promise.allSettled(
        sessions.map((session) => onAgentControl(session, action))
      );
      const failed = outcomes.filter((outcome) => outcome.status === "rejected").length;
      if (failed > 0) {
        setGroupControlError((previous) => ({
          ...previous,
          [groupId]: `${failed}개의 에이전트를 ${BULK_FAILURE_VERB[action]}하지 못했어요.`,
        }));
      } else {
        onSessionActionComplete?.();
      }
    } finally {
      setGroupControlBusy((previous) => ({ ...previous, [groupId]: false }));
    }
  }

  function openMemberDetails(entry: MemberEntry, anchor: DOMRect) {
    if (entry.agent || entry.agentSession || externalAgentProfile(entry)) setDetailEntryId(entry.id);
    else setProfileCard({ entryId: entry.id, anchor });
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      {!hideSearch && (
      <div className="dc-member-search shrink-0">
        <label className="dc-search dc-search--compact dc-member-search-box">
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
            {disconnected ? "연결되면 멤버가 보여요." : loading ? "멤버를 불러오는 중이에요." : `${roomName}에는 아직 멤버가 없어요.`}
          </p>
        )}
        {memberSections.map((section) => section.groups.length > 0 && (
          <Fragment key={section.label}>
            <h3 className="dc-member-section-head">{section.label} — {section.count}</h3>
            {section.groups.map((group) => {
          const collapseId = `owner:${group.id}`;
          const collapsed = Boolean(collapsedGroups[collapseId]);
          const ToggleIcon = collapsed ? ChevronRight : ChevronDown;
          const pausable = onAgentControl
            ? group.agents.filter((entry) => entry.agentSession && agentSessionCanPause(entry.agentSession))
            : [];
          const resumable = onAgentControl
            ? group.agents.filter((entry) => entry.agentSession && agentSessionCanResume(entry.agentSession))
            : [];
          const stoppable = onAgentControl
            ? group.agents.filter((entry) => entry.agentSession && agentSessionCanBulkStop(entry.agentSession))
            : [];
          return (
            <section key={group.id} className="dc-person-member-group">
              {group.person ? (
                <MemberRow
                  entry={group.person}
                  onOpenDetails={openMemberDetails}
                  onContextMenu={handleMemberContextMenu}
                  canManageParticipant={roleMenuAvailable || Boolean(onParticipantRemove || (canModerate && onParticipantMute))}
                  canEditRoles={canEditRoles}
                />
              ) : (
                <p className="dc-person-owner-label preserve-words">{group.label}</p>
              )}
              {group.agents.length > 0 && (
                <div className="dc-owner-agent-group" data-collapsed={collapsed}>
                  <div className="dc-owner-agent-head">
                    <button type="button" className="dc-owner-agent-toggle" aria-expanded={!collapsed}
                      aria-label={`에이전트 ${group.agents.length}명 ${collapsed ? "펼치기" : "접기"}`}
                      onClick={() => toggleGroup(collapseId)}>
                      <ToggleIcon size={12} aria-hidden />
                      <span>에이전트 {group.agents.length}명</span>
                    </button>
                    {resumable.length > 0 && (
                      <button type="button" className="dc-owner-agent-bulk-action"
                        aria-label="전체 재개" title="전체 재개"
                        disabled={groupControlBusy[group.id]}
                        onClick={() => void runGroupControl(group.id, resumable.map((entry) => entry.agentSession!), "resume")}>
                        <Play size={13} aria-hidden />
                      </button>
                    )}
                    {pausable.length > 0 && (
                      <button type="button" className="dc-owner-agent-bulk-action"
                        aria-label="전체 일시정지" title="전체 일시정지"
                        disabled={groupControlBusy[group.id]}
                        onClick={() => void runGroupControl(group.id, pausable.map((entry) => entry.agentSession!), "pause")}>
                        <Pause size={13} aria-hidden />
                      </button>
                    )}
                    {stoppable.length > 0 && (
                      <button type="button" className="dc-owner-agent-bulk-action"
                        aria-label="전체 중지" title="전체 중지"
                        disabled={groupControlBusy[group.id]}
                        onClick={() => void runGroupControl(group.id, stoppable.map((entry) => entry.agentSession!), "stop")}>
                        <Square size={13} aria-hidden />
                      </button>
                    )}
                  </div>
                  {groupControlError[group.id] && (
                    <p className="dc-room-play-error preserve-words" role="alert">{groupControlError[group.id]}</p>
                  )}
                  {!collapsed && (
                    <div className="dc-owner-agent-list-wrap">
                      <span className="dc-owner-agent-spine" aria-hidden="true" />
                      <div className="dc-owner-agent-list">
                        {group.agents.map((entry) => (
                          <div key={entry.id} className="dc-owner-agent-item">
                            <MemberRow
                              entry={entry}
                              onOpenDetails={openMemberDetails}
                              onContextMenu={handleMemberContextMenu}
                              canManageParticipant={roleMenuAvailable || Boolean(onParticipantRemove || (canModerate && onParticipantMute))}
                              canEditRoles={canEditRoles}
                            />
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}
            </section>
          );
        })}
          </Fragment>
        ))}
        {contextBadges.length > 0 && (
          <details className="dc-member-context mt-3 px-2" aria-label="참가자 맥락 요약">
            <summary className="cursor-pointer list-none text-[11px] font-bold text-text-muted hover:text-text-secondary">
              고급 연결 요약
            </summary>
            <ProviderTruthChips badges={contextBadges} compact />
          </details>
        )}
      </div>
      {profileCardEntry && profileCard && (
        <MemberProfileCard entry={profileCardEntry} anchor={profileCard.anchor} isHost={canEditRoles}
          onClose={closeProfileCard} />
      )}
      {detailEntry && (
        <MemberDetailModal
          entry={detailEntry}
          onClose={() => setDetailEntryId("")}
          onAgentControl={onAgentControl}
          availableProviders={availableProviders}
          localProviderActions={localProviderActions}
          onAgentConfigure={onAgentConfigure}
          onAgentProfileUpdate={onAgentProfileUpdate}
          onAgentAvatarUpdate={onAgentAvatarUpdate}
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
            {roleMenuAvailable && (
              <div role="group" aria-label={`${memberMenu.entry.displayName} 역할`}>
                <p className="dc-member-context-menu-label">역할</p>
                {ROLE_OPTIONS.map((option) => (
                  <button
                    key={option.id}
                    type="button"
                    role="menuitemradio"
                    aria-checked={memberMenu.entry.role === option.id}
                    className="dc-member-context-menu-item"
                    onClick={() => {
                      const entry = memberMenu.entry;
                      setMemberMenu(null);
                      if (entry.role !== option.id) void handleRoleChange(entry.id, option.id);
                    }}
                  >
                    <option.icon size={14} />
                    <span className="flex-1">{option.label}</span>
                    {memberMenu.entry.role === option.id && <Check size={14} aria-hidden />}
                  </button>
                ))}
              </div>
            )}
            {roleMenuAvailable && canModerateEntry(memberMenu.entry) && <span className="dc-context-separator" aria-hidden />}
            {canModerateEntry(memberMenu.entry) && canModerate && onParticipantMute && (
              <button
                type="button"
                role="menuitem"
                className="dc-member-context-menu-item"
                disabled={muteBusy}
                onClick={() => void handleToggleMute(memberMenu.entry)}
              >
                {memberMenu.entry.muted ? <Volume2 size={14} /> : <VolumeX size={14} />}
                {memberMenu.entry.muted ? "음소거 해제" : "음소거"}
              </button>
            )}
            {canModerateEntry(memberMenu.entry) && onParticipantRemove && <ParticipantRemovalControls
              key={memberMenu.entry.id} participantId={memberMenu.entry.id} displayName={memberMenu.entry.displayName}
              person={memberMenu.entry.member?.participant_type === "human"}
              onRemove={async (id, action) => { await onParticipantRemove(id, action); setMemberMenu(null); }}
            />}
          </div>
        </>
      )}
    </div>
  );
}
