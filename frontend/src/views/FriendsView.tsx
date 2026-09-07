import { MoreHorizontal, UserRound, Bot } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { useFriendsDirectory } from "../app/useFriendsDirectory";
import type { FriendParticipantType } from "../types/generated/FriendParticipantType";
import type { SaveFriend } from "../types/generated/SaveFriend";
import type { SavedFriend } from "../types/generated/SavedFriend";
import { isActivePresence } from "../lib/presenceStatus";

const types: Record<FriendParticipantType, string> = {
  human: "사람", subscription_ai: "구독형 AI", api: "API", local: "Local", remote: "외부 AI", unknown: "기타",
};
const inputStyle = { width: "100%", minHeight: 44, padding: "8px 12px", boxSizing: "border-box" as const };
const selectStyle = { ...inputStyle, appearance: "none" as const, cursor: "pointer" };
const buttonStyle = { minHeight: 44, minWidth: 44, padding: "0 12px" };

function newFriend(): SaveFriend {
  return { friend_id: crypto.randomUUID(), expected_revision: 0, details: {
    display_name: "", handle: "", participant_type: "human", provider_kind: "", connection_kind: "",
    agent_id: "", source_agent_id: "", last_meeting_id: "", status: "offline", source: "manual", last_seen_at: null,
  } };
}

export default function FriendsView({ onClose }: { onClose: () => void }) {
  const directory = useFriendsDirectory();
  const [query, setQuery] = useState("");
  const [type, setType] = useState<FriendParticipantType | "all">("all");
  const [onlineOnly, setOnlineOnly] = useState(false);
  const [editing, setEditing] = useState<SaveFriend | null>(null);
  const [deleting, setDeleting] = useState<SavedFriend | null>(null);
  const needle = query.trim().toLocaleLowerCase();
  const visible = directory.friends.filter((friend) => (type === "all" || friend.details.participant_type === type)
    && (!onlineOnly || isActivePresence(friend.details.status))
    && [friend.details.display_name, friend.details.handle, friend.details.provider_kind].some((text) => text.toLocaleLowerCase().includes(needle)));
  return <section style={{ padding: 24, overflowY: "auto", minHeight: 0, display: "grid", gap: 24, alignContent: "start" }} aria-label="친구 목록">
    <header style={{ display: "flex", flexWrap: "wrap", alignItems: "center", gap: 12 }}>
      <h1 style={{ margin: 0, marginRight: "auto" }}>친구</h1>
      <button type="button" className="dc-agent-create-primary" style={buttonStyle} disabled={directory.busy || !directory.loaded} onClick={() => setEditing(newFriend())}>친구 추가</button>
      <button type="button" className="ops-button" style={buttonStyle} onClick={onClose}>대화로 돌아가기</button>
    </header>
    <div style={{ display: "grid", gap: 12 }}>
      <label>친구 검색<input className="ops-input" style={inputStyle} type="search" value={query} onChange={(event) => setQuery(event.target.value)} placeholder="이름, 핸들 또는 제공자" /></label>
      <div style={{ display: "flex", gap: 12, flexWrap: "wrap", alignItems: "end" }}>
        <button type="button" className="ops-button" style={buttonStyle} aria-pressed={!onlineOnly} onClick={() => setOnlineOnly(false)}>모두</button>
        <button type="button" className="ops-button" style={buttonStyle} aria-pressed={onlineOnly} onClick={() => setOnlineOnly(true)}>최근 온라인</button>
        <label style={{ width: 160, maxWidth: "100%" }}>종류<select className="ops-input" style={selectStyle} value={type} onChange={(event) => setType(event.target.value as typeof type)}>
          <option value="all">모두</option>{Object.entries(types).map(([value, label]) => <option key={value} value={value}>{label}</option>)}
        </select></label>
        <button type="button" className="ops-button" style={{ ...buttonStyle, alignSelf: "end" }} disabled={directory.busy} onClick={() => void directory.reload()}>새로고침</button>
      </div>
    </div>
    {onlineOnly && <p style={{ margin: 0 }}>마지막으로 저장된 상태를 기준으로 표시해요.</p>}
    {!editing && !deleting && directory.error && <p role="alert">{directory.error}</p>}
    {directory.busy && <p role="status">친구 목록을 처리하고 있어요…</p>}
    {directory.loaded && !directory.busy && !directory.error && visible.length === 0 && <p>{query || type !== "all" || onlineOnly ? "일치하는 친구가 없어요." : "친구를 추가해 보세요."}</p>}
    <div style={{ display: "grid", gap: 12 }}>
      {visible.map((friend) => <FriendRow key={friend.friend_id} friend={friend} busy={directory.busy}
        onEdit={() => setEditing({ friend_id: friend.friend_id, expected_revision: friend.revision, details: { ...friend.details } })}
        onDelete={() => setDeleting(friend)} />)}
    </div>
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

function FriendRow({ friend, busy, onEdit, onDelete }: { friend: SavedFriend; busy: boolean; onEdit: () => void; onDelete: () => void }) {
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
  return <article style={{ padding: "16px 0", borderBottom: "1px solid var(--color-panel-border)", display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}
    onContextMenu={(event) => { event.preventDefault(); openMenu(event.clientX, event.clientY); }}>
    <span className="dc-invite-friend-avatar" style={{ width: 44, height: 44, flexShrink: 0 }} aria-hidden>
      {friend.details.participant_type === "human" ? <UserRound size={22} /> : <Bot size={22} />}
    </span>
    <div style={{ flex: 1, minWidth: 0, overflowWrap: "anywhere" }}><strong>{friend.details.display_name}</strong>
      <p style={{ margin: "4px 0 0", fontSize: 13, color: "var(--color-text-muted)" }}>{[types[friend.details.participant_type], friend.details.provider_kind, friend.details.handle].filter(Boolean).join(" · ")}</p>
    </div>
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
      <label>제공자 (선택)<input className="ops-input" style={inputStyle} maxLength={256} value={draft.details.provider_kind} disabled={busy} onChange={(event) => update("provider_kind", event.target.value)} /></label>
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
