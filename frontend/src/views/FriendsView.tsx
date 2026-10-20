import { MoreVertical, UserRound, Bot, Cloud, Cpu, Wifi, Users, Search, RefreshCw, X } from "../ui/icons";
import ParticipantAvatarContent from "./components/ParticipantAvatarContent";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useFriendsDirectory } from "../app/useFriendsDirectory";
import type { FriendParticipantType } from "../types/generated/FriendParticipantType";
import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";
import { isActivePresence, presenceStatusLabel } from "../lib/presenceStatus";
import { PROVIDER_BRANDS, providerBrandKey } from "./components/providerBranding";

import type { SavedFriendsAuthority } from "../api/friends";

const types: Record<FriendParticipantType, string> = {
  subscription_ai: "구독 에이전트", api: "API 키", local: "내 컴퓨터", remote: "외부 AI", human: "사람", unknown: "기타",
};
const icons = { human: UserRound, subscription_ai: Bot, api: Cloud, local: Cpu, remote: Wifi, unknown: Users };

function newFriend(): SaveFriend {
  return { friend_id: crypto.randomUUID(), expected_revision: 0, details: {
    display_name: "", handle: "", participant_type: "human", provider_kind: "", connection_kind: "",
    agent_id: "", source_agent_id: "", last_meeting_id: "", status: "offline", source: "manual", last_seen_at: null,
  } };
}

export default function FriendsView({ onClose, authority, userArea }: { onClose: () => void; authority?: SavedFriendsAuthority; userArea?: ReactNode }) {
  const directory = useFriendsDirectory(authority);
  const [query, setQuery] = useState("");
  const [type, setType] = useState<FriendParticipantType | "all">("all");
  const [onlineOnly, setOnlineOnly] = useState(false);
  const [editing, setEditing] = useState<SaveFriend | null>(null);
  const [deleting, setDeleting] = useState<SavedFriend | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const searchRef = useRef<HTMLInputElement>(null);
  const needle = query.trim().toLocaleLowerCase();
  const visible = directory.friends.filter((friend) => (type === "all" || friend.details.participant_type === type)
    && (!onlineOnly || isActivePresence(friend.details.status))
    && [friend.details.display_name, friend.details.handle, friend.details.provider_kind].some((text) => text.toLocaleLowerCase().includes(needle)));
  const selected = visible.find((friend) => friend.friend_id === selectedId) ?? visible[0] ?? null;
  const add = () => setEditing(newFriend());
  const edit = (friend: SavedFriend) => setEditing({ friend_id: friend.friend_id, expected_revision: friend.revision, details: { ...friend.details } });
  return <section className="dc-friends-workspace" aria-label="친구" data-testid="friends-workspace">
    <aside className="dc-home-sidebar" aria-label="친구 탐색">
      <header className="dc-home-search">
        <button type="button" className="dc-home-search-button" onClick={() => searchRef.current?.focus()}>친구 찾기</button>
      </header>
      <nav className="dc-home-directory" aria-label="친구 분류">
        <button className="dc-home-nav-item" type="button" aria-pressed={type === "all"} onClick={() => setType("all")}><Users size={20} />친구</button>
        {Object.entries(types).map(([value, label]) => {
          const Icon = icons[value as FriendParticipantType];
          return <button key={value} type="button" className="dc-home-nav-item" aria-pressed={type === value} onClick={() => setType(value as FriendParticipantType)}><Icon size={20} />{label}</button>;
        })}
      </nav>
      {userArea}
    </aside>
    <section className="dc-friends-page" aria-label="친구 목록">
      <header className="dc-friends-head">
        <h1 className="dc-friends-title"><Users size={20} />친구</h1>
        <nav className="dc-friends-tabs" aria-label="친구 필터">
          <button type="button" aria-pressed={onlineOnly} onClick={() => setOnlineOnly(true)}>온라인</button>
          <button type="button" aria-pressed={!onlineOnly} onClick={() => setOnlineOnly(false)}>모두</button>
          <button type="button" className="add-tab" disabled={directory.busy || !directory.loaded} onClick={add}>친구 추가하기</button>
        </nav>
        <button type="button" className="dc-friends-icon-button dc-friends-close" aria-label="대화로 돌아가기" title="대화로 돌아가기" onClick={onClose}><X size={20} /></button>
      </header>
      <div className="dc-friends-body">
        <div className="dc-friends-main">
          <label className="dc-friends-search"><input ref={searchRef} aria-label="친구 검색" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="검색하기" /><Search size={18} aria-hidden /></label>
          <div className="dc-friend-section-head"><h2>{onlineOnly ? "온라인" : "모든 친구"}{directory.loaded ? ` — ${visible.length}` : ""}</h2><button type="button" className="dc-friends-icon-button" aria-label="새로고침" title="새로고침" disabled={directory.busy} onClick={() => void directory.reload()}><RefreshCw size={16} /></button></div>
          {onlineOnly && <p className="dc-friend-empty">마지막으로 저장된 상태를 기준으로 표시해요.</p>}
          {!editing && !deleting && directory.error && <div role="alert" className="dc-friends-error">
            <span>{directory.error}</span>
            {!directory.loaded && <button type="button" className="ops-button dc-friends-small-button" disabled={directory.busy} onClick={() => void directory.reload()}>다시 시도</button>}
          </div>}
          {directory.busy && !directory.loaded && <p role="status" className="dc-friend-empty">친구 목록을 불러오는 중이에요…</p>}
          {directory.loaded && !directory.busy && !directory.error && visible.length === 0 && <p className="dc-friend-empty">{query || type !== "all" || onlineOnly ? "일치하는 친구가 없어요." : "친구를 추가해 보세요."}</p>}
          {visible.map((friend) => <FriendRow key={friend.friend_id} friend={friend} busy={directory.busy} selected={selected?.friend_id === friend.friend_id}
            onSelect={() => setSelectedId(friend.friend_id)} onEdit={() => edit(friend)} onDelete={() => setDeleting(friend)} />)}
        </div>
        <aside className="dc-friends-activity" aria-label="친구 프로필">
          <h2>프로필</h2>
          {selected ? <FriendProfile friend={selected} busy={directory.busy} onEdit={() => edit(selected)} /> : <div className="dc-activity-card"><p>선택한 친구가 없어요.</p><span>친구를 선택하면 저장된 프로필을 볼 수 있어요.</span></div>}
        </aside>
      </div>
    </section>
    {editing && <FriendEditor key={editing.friend_id} initial={editing} busy={directory.busy} error={directory.error} onCancel={() => setEditing(null)} onSave={async (draft) => { if (await directory.save(draft)) setEditing(null); }} />}
    {deleting && <FriendDialog title="친구를 삭제할까요?" busy={directory.busy} onCancel={() => setDeleting(null)}>
      <p style={{ margin: 0, overflowWrap: "anywhere" }}>{deleting.details.display_name}을 친구 목록에서 삭제해요. 방 참여와 대화 기록은 유지돼요.</p>
      {directory.error && <p role="alert">{directory.error}</p>}
      <div className="dc-create-channel-actions">
        <button type="button" className="dc-agent-create-secondary" style={{ minHeight: 40 }} disabled={directory.busy} autoFocus onClick={() => setDeleting(null)}>취소</button>
        <button type="button" className="dc-member-session-button" data-variant="danger" style={{ minHeight: 40, minWidth: 96 }} disabled={directory.busy} onClick={async () => { if (await directory.remove(deleting.friend_id)) setDeleting(null); }}>삭제</button>
      </div>
    </FriendDialog>}
  </section>;
}

// A provider shows by its product name (Codex, DeepSeek), never its raw kind id.
function providerName(kind: string): string {
  const key = providerBrandKey(undefined, kind);
  return key ? PROVIDER_BRANDS[key].productName : "";
}

// The same person looks the same here as in the room's member list.
function FriendAvatar({ friend, size = 32 }: { friend: SavedFriend; size?: number }) {
  const Icon = icons[friend.details.participant_type];
  return <span className="dc-friend-avatar" data-type={friend.details.participant_type} aria-hidden>
    <ParticipantAvatarContent participantType={friend.details.participant_type} displayName={friend.details.display_name}
      providerKind={friend.details.provider_kind} size={size} fallback={<Icon size={Math.round(size / 2)} />} />
  </span>;
}

function FriendProfile({ friend, busy, onEdit }: { friend: SavedFriend; busy: boolean; onEdit: () => void }) {
  const { details } = friend;
  // Only what a person can read: ids (agent, last room) never reach the screen,
  // and a provider is shown only for an AI that has one.
  const facts = [["종류", types[details.participant_type]], ["마지막 상태", presenceStatusLabel(details.status)],
    ...(details.participant_type !== "human" && providerName(details.provider_kind) ? [["제공자", providerName(details.provider_kind)]] : [])];
  return <article className="dc-friend-profile-card">
    <div className="dc-friend-profile-banner" aria-hidden />
    <div className="dc-friend-profile-body"><FriendAvatar friend={friend} size={64} /><h3>{details.display_name}</h3>
      {details.handle && <p className="dc-friend-profile-handle">{details.handle}</p>}
      <button type="button" className="ops-button dc-friends-small-button" disabled={busy} onClick={onEdit}>친구 편집</button>
      <dl className="dc-friend-profile-facts">{facts.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
    </div>
  </article>;
}

function FriendRow({ friend, busy, selected, onSelect, onEdit, onDelete }: { friend: SavedFriend; busy: boolean; selected: boolean; onSelect: () => void; onEdit: () => void; onDelete: () => void }) {
  const menu = useRef<HTMLDialogElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  function openMenu(x: number, y: number) {
    if (busy || !menu.current) return;
    trigger.current?.focus();
    menu.current.style.left = `${Math.max(16, Math.min(x, window.innerWidth - 224))}px`;
    menu.current.style.top = `${Math.max(16, Math.min(y, window.innerHeight - 128))}px`;
    menu.current.showModal();
  }
  function select(action: () => void) {
    menu.current?.close();
    trigger.current?.focus();
    action();
  }
  return <article className="dc-friend-row" data-selected={selected}
    onContextMenu={(event) => { event.preventDefault(); openMenu(event.clientX, event.clientY); }}>
    <button type="button" className="dc-friend-main-button" aria-label={`${friend.details.display_name} 프로필 보기`} aria-pressed={selected} onClick={onSelect}><FriendAvatar friend={friend} /><span><strong className="dc-friend-name">{friend.details.display_name}</strong>
      <span className="dc-friend-detail">{[types[friend.details.participant_type], providerName(friend.details.provider_kind), friend.details.handle].filter(Boolean).join(" · ")}</span>
    </span></button>
    <button ref={trigger} type="button" className="dc-friends-more" disabled={busy}
      aria-label={`${friend.details.display_name} 더보기`} aria-haspopup="dialog"
      onClick={(event) => { const rect = event.currentTarget.getBoundingClientRect(); openMenu(rect.right - 208, rect.bottom + 8); }}><MoreVertical size={20} /></button>
    <dialog ref={menu} className="dc-member-context-menu" aria-label="친구 동작"
      style={{ position: "fixed", margin: 0, width: 208, maxWidth: "calc(100vw - 32px)", padding: 8, boxSizing: "border-box", color: "var(--color-text-primary)", border: "1px solid var(--color-panel-border)" }}
      onClick={(event) => { if (event.target === event.currentTarget) menu.current?.close(); }}>
      <div style={{ display: "grid", gap: 4 }}>
        <button type="button" className="dc-member-context-menu-item" disabled={busy} onClick={() => select(onEdit)}>편집</button>
        <button type="button" className="dc-member-context-menu-item" data-variant="danger" disabled={busy} onClick={() => select(onDelete)}>삭제</button>
      </div>
    </dialog>
  </article>;
}

function FriendEditor({ initial, busy, error, onCancel, onSave }: {
  initial: SaveFriend; busy: boolean; error: string; onCancel: () => void; onSave: (draft: SaveFriend) => Promise<void>;
}) {
  const [draft, setDraft] = useState(initial);
  const changed = initial.expected_revision === 0 || JSON.stringify(draft.details) !== JSON.stringify(initial.details);
  const update = (field: "display_name" | "handle" | "provider_kind", value: string) => setDraft((current) => ({ ...current, details: { ...current.details, [field]: value } }));
  // A provider only means something for an AI friend, and is picked from the
  // known products instead of typed as a raw kind id.
  const isHuman = draft.details.participant_type === "human";
  const providerOptions = Object.entries(PROVIDER_BRANDS).map(([key, brand]) => ({ value: key, label: brand.productName }));
  const currentProvider = draft.details.provider_kind;
  return <FriendDialog title={initial.expected_revision === 0 ? "친구 추가" : "친구 편집"} busy={busy} onCancel={onCancel}>
    <form style={{ display: "grid", gap: 16 }} onSubmit={(event) => { event.preventDefault(); if (!busy && changed && draft.details.display_name.trim()) void onSave(draft); }}>
      <label className="dc-create-channel-name">이름<input className="ops-input" autoFocus required maxLength={120} value={draft.details.display_name} disabled={busy} onChange={(event) => update("display_name", event.target.value)} /></label>
      <label className="dc-create-channel-name">종류<select className="ops-input dc-select" style={{ appearance: "none", cursor: "pointer" }} value={draft.details.participant_type} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, details: { ...current.details, participant_type: event.target.value as FriendParticipantType } }))}>
        {Object.entries(types).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
      </select></label>
      <label className="dc-create-channel-name">사용자명 (선택)<input className="ops-input" maxLength={120} value={draft.details.handle} disabled={busy} onChange={(event) => update("handle", event.target.value)} /></label>
      {!isHuman && <label className="dc-create-channel-name">제공자 (선택)<select className="ops-input dc-select" style={{ appearance: "none", cursor: "pointer" }} value={currentProvider} disabled={busy} onChange={(event) => update("provider_kind", event.target.value)}>
        <option value="">선택 안 함</option>
        {providerOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
        {currentProvider && !providerOptions.some((option) => option.value === currentProvider) && <option value={currentProvider}>{providerName(currentProvider) || "기타"}</option>}
      </select></label>}
      {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
      <div className="dc-create-channel-actions">
        <button type="button" className="dc-agent-create-secondary" style={{ minHeight: 40 }} disabled={busy} onClick={onCancel}>취소</button>
        <button type="submit" className="ops-cta" style={{ padding: "0 16px" }} disabled={busy || !changed || !draft.details.display_name.trim()}>{busy ? "저장 중…" : "저장"}</button>
      </div>
    </form>
  </FriendDialog>;
}

function FriendDialog({ title, busy, onCancel, children }: { title: string; busy: boolean; onCancel: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  return <div className="dc-modal-backdrop" role="presentation"><dialog ref={ref} className="dc-create-channel-modal" aria-label={title}
    style={{ maxHeight: "calc(100dvh - 32px)", margin: "auto", overflowY: "auto", color: "var(--color-text-primary)", border: "1px solid var(--color-panel-border)" }}
    onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }} onClick={(event) => { if (event.target === event.currentTarget && !busy) onCancel(); }}>
    <section style={{ display: "grid", gap: 16 }}>
      <header className="dc-create-channel-head"><h2>{title}</h2>
        <button type="button" className="dc-dialog-close" aria-label="닫기" disabled={busy} onClick={onCancel}><X size={24} /></button>
      </header>
      {children}
    </section>
  </dialog></div>;
}
