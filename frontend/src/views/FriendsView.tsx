import { MoreHorizontal, UserRound, Bot, Cloud, Cpu, Wifi, Users, Search, Plus } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useFriendsDirectory } from "../app/useFriendsDirectory";
import type { FriendParticipantType } from "../types/generated/FriendParticipantType";
import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";
import { isActivePresence, presenceStatusLabel } from "../lib/presenceStatus";

import type { RoomHttpAuthority } from "../api/roomHttpAuthority";

const types: Record<FriendParticipantType, string> = {
  subscription_ai: "구독형 AI", api: "API", local: "Local", remote: "외부 AI", human: "사람", unknown: "기타",
};
const icons = { human: UserRound, subscription_ai: Bot, api: Cloud, local: Cpu, remote: Wifi, unknown: Users };
const inputStyle = { width: "100%", minHeight: 44, padding: "8px 12px", boxSizing: "border-box" as const };
const selectStyle = { ...inputStyle, appearance: "none" as const, cursor: "pointer" };
const buttonStyle = { minHeight: 44, minWidth: 44, padding: "0 12px" };

function newFriend(): SaveFriend {
  return { friend_id: crypto.randomUUID(), expected_revision: 0, details: {
    display_name: "", handle: "", participant_type: "human", provider_kind: "", connection_kind: "",
    agent_id: "", source_agent_id: "", last_meeting_id: "", status: "offline", source: "manual", last_seen_at: null,
  } };
}

export default function FriendsView({ onClose, authority, userArea }: { onClose: () => void; authority?: RoomHttpAuthority; userArea?: ReactNode }) {
  const directory = useFriendsDirectory(authority);
  const [query, setQuery] = useState("");
  const [type, setType] = useState<FriendParticipantType | "all">("all");
  const [onlineOnly, setOnlineOnly] = useState(false);
  const [editing, setEditing] = useState<SaveFriend | null>(null);
  const [deleting, setDeleting] = useState<SavedFriend | null>(null);
  const [selectedId, setSelectedId] = useState("");
  const needle = query.trim().toLocaleLowerCase();
  const visible = directory.friends.filter((friend) => (type === "all" || friend.details.participant_type === type)
    && (!onlineOnly || isActivePresence(friend.details.status))
    && [friend.details.display_name, friend.details.handle, friend.details.provider_kind].some((text) => text.toLocaleLowerCase().includes(needle)));
  const selected = visible.find((friend) => friend.friend_id === selectedId) ?? visible[0] ?? null;
  const add = () => setEditing(newFriend());
  const edit = (friend: SavedFriend) => setEditing({ friend_id: friend.friend_id, expected_revision: friend.revision, details: { ...friend.details } });
  return <section className="dc-friends-workspace" aria-label="친구" data-testid="friends-workspace">
    <aside className="dc-home-sidebar" aria-label="친구 탐색">
      <header className="dc-home-search"><Users size={20} /><strong>친구</strong></header>
      <nav className="dc-home-directory" aria-label="친구 분류">
        <button className="dc-home-nav-item" type="button" aria-pressed={type === "all"} onClick={() => setType("all")}><Users size={20} />친구</button>
        {Object.entries(types).map(([value, label]) => {
          const Icon = icons[value as FriendParticipantType];
          return <button key={value} type="button" className="dc-home-nav-item" aria-pressed={type === value} onClick={() => setType(value as FriendParticipantType)}><Icon size={20} />{label}</button>;
        })}
        <div className="dc-dm-title"><span>저장된 친구</span><button type="button" aria-label="친구 추가하기" disabled={directory.busy || !directory.loaded} onClick={add}><Plus size={16} /></button></div>
        {visible.map((friend) => <button key={friend.friend_id} className="dc-dm-row" type="button" aria-pressed={selected?.friend_id === friend.friend_id} onClick={() => setSelectedId(friend.friend_id)}>
          <FriendAvatar friend={friend} /><span>{friend.details.display_name}</span>
        </button>)}
      </nav>
      {userArea}
    </aside>
    <section className="dc-friends-page" aria-label="친구 목록">
      <header className="dc-friends-head">
        <h1 className="dc-friends-title"><Users size={20} />친구</h1>
        <nav className="dc-friends-tabs" aria-label="친구 필터">
          <button type="button" aria-pressed={onlineOnly} onClick={() => setOnlineOnly(true)}>온라인</button>
          <button type="button" aria-pressed={!onlineOnly} onClick={() => setOnlineOnly(false)}>모두</button>
          <button type="button" className="add-tab" disabled={directory.busy || !directory.loaded} onClick={add}>친구 추가</button>
        </nav>
        <button type="button" className="ops-button dc-friends-close" style={buttonStyle} onClick={onClose}>대화로 돌아가기</button>
      </header>
      <div className="dc-friends-body">
        <div className="dc-friends-main">
          <label className="dc-friends-search"><Search size={16} /><input aria-label="친구 검색" type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="검색하기" /></label>
          <div className="dc-friend-section-head"><h2>{onlineOnly ? "온라인" : "모든 친구"} — {visible.length}</h2><button type="button" className="ops-button" style={buttonStyle} disabled={directory.busy} onClick={() => void directory.reload()}>새로고침</button></div>
          {onlineOnly && <p className="dc-friend-empty">마지막으로 저장된 상태를 기준으로 표시해요.</p>}
          {!editing && !deleting && directory.error && <p role="alert">{directory.error}</p>}
          {directory.busy && <p role="status">친구 목록을 처리하고 있어요…</p>}
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
      <div style={{ display: "flex", justifyContent: "end", gap: 12 }}>
        <button type="button" className="ops-button" style={buttonStyle} disabled={directory.busy} autoFocus onClick={() => setDeleting(null)}>취소</button>
        <button type="button" className="dc-member-session-button" data-variant="danger" style={buttonStyle} disabled={directory.busy} onClick={async () => { if (await directory.remove(deleting.friend_id)) setDeleting(null); }}>삭제</button>
      </div>
    </FriendDialog>}
  </section>;
}

function FriendAvatar({ friend }: { friend: SavedFriend }) {
  const Icon = icons[friend.details.participant_type];
  return <span className="dc-friend-avatar" data-type={friend.details.participant_type} aria-hidden><Icon size={22} /></span>;
}

function FriendProfile({ friend, busy, onEdit }: { friend: SavedFriend; busy: boolean; onEdit: () => void }) {
  const { details } = friend;
  const facts = [["종류", types[details.participant_type]], ["저장된 상태", presenceStatusLabel(details.status)],
    ["제공자", details.provider_kind || "미지정"], ["에이전트", details.source_agent_id || details.agent_id || "미지정"], ["최근 방", details.last_meeting_id || "기록 없음"]];
  return <article className="dc-friend-profile-card">
    <div className="dc-friend-profile-banner" aria-hidden />
    <div className="dc-friend-profile-body"><FriendAvatar friend={friend} /><h3>{details.display_name}</h3>
      {details.handle && <p className="dc-friend-profile-handle">{details.handle}</p>}
      <button type="button" className="ops-button" style={buttonStyle} disabled={busy} onClick={onEdit}>친구 편집</button>
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
      <span className="dc-friend-detail">{[types[friend.details.participant_type], friend.details.provider_kind, friend.details.handle].filter(Boolean).join(" · ")}</span>
    </span></button>
    <button ref={trigger} type="button" className="ops-button" style={{ ...buttonStyle, flexShrink: 0 }} disabled={busy}
      aria-label={`${friend.details.display_name} 더보기`} aria-haspopup="dialog"
      onClick={(event) => { const rect = event.currentTarget.getBoundingClientRect(); openMenu(rect.right - 208, rect.bottom + 8); }}><MoreHorizontal size={20} /></button>
    <dialog ref={menu} className="dc-member-context-menu" aria-label="친구 동작"
      style={{ position: "fixed", margin: 0, width: 208, maxWidth: "calc(100vw - 32px)", padding: 8, boxSizing: "border-box", color: "var(--color-text-primary)", border: "1px solid var(--color-panel-border)" }}
      onClick={(event) => { if (event.target === event.currentTarget) menu.current?.close(); }}>
      <div style={{ display: "grid", gap: 4 }}>
        <button type="button" className="dc-member-context-menu-item" style={buttonStyle} disabled={busy} onClick={() => select(onEdit)}>편집</button>
        <button type="button" className="dc-member-context-menu-item" data-variant="danger" style={buttonStyle} disabled={busy} onClick={() => select(onDelete)}>삭제</button>
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
  return <FriendDialog title={initial.expected_revision === 0 ? "친구 추가" : "친구 편집"} busy={busy} onCancel={onCancel}>
    <form style={{ display: "grid", gap: 20 }} onSubmit={(event) => { event.preventDefault(); if (!busy && changed && draft.details.display_name.trim()) void onSave(draft); }}>
      <label>이름<input className="ops-input" style={inputStyle} autoFocus required maxLength={120} value={draft.details.display_name} disabled={busy} onChange={(event) => update("display_name", event.target.value)} /></label>
      <label>종류<select className="ops-input" style={selectStyle} value={draft.details.participant_type} disabled={busy} onChange={(event) => setDraft((current) => ({ ...current, details: { ...current.details, participant_type: event.target.value as FriendParticipantType } }))}>
        {Object.entries(types).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
      </select></label>
      <label>핸들 (선택)<input className="ops-input" style={inputStyle} maxLength={120} value={draft.details.handle} disabled={busy} onChange={(event) => update("handle", event.target.value)} /></label>
      <label>제공자 (선택)<input autoCorrect="off" autoCapitalize="none" spellCheck={false} className="ops-input" style={inputStyle} maxLength={256} value={draft.details.provider_kind} disabled={busy} onChange={(event) => update("provider_kind", event.target.value)} /></label>
      {error && <p role="alert">{error}</p>}
      <div style={{ display: "flex", justifyContent: "end", gap: 12 }}>
        <button type="button" className="ops-button" style={buttonStyle} disabled={busy} onClick={onCancel}>취소</button>
        <button type="submit" className="dc-agent-create-primary" style={buttonStyle} disabled={busy || !changed || !draft.details.display_name.trim()}>{busy ? "저장 중…" : "저장"}</button>
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
    style={{ width: "min(440px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", margin: "auto", padding: 24, overflowY: "auto", color: "var(--color-text-primary)", border: "1px solid var(--color-panel-border)" }}
    onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }} onClick={(event) => { if (event.target === event.currentTarget && !busy) onCancel(); }}>
    <section style={{ display: "grid", gap: 20 }}><h2 style={{ margin: 0 }}>{title}</h2>{children}</section>
  </dialog></div>;
}
