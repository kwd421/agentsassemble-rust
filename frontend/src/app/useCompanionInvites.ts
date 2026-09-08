import { useEffect, useRef, useState } from "react";
import { attendeePacketText, createCompanionAttendeeInvite, type AttendeePacketCustody } from "../api/attendeeInvite";
import { copyText } from "../lib/copyInviteText";
import { roomGuestSessionExpired, type RoomGuestSession } from "../lib/roomGuestSession";

export function useCompanionInvites(session: RoomGuestSession | null) {
  const current = useRef(session); current.current = session;
  const active = useRef(true);
  const busy = useRef(false);
  const pending = useRef(new Map<string, string>());
  const [provider, setProvider] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [creating, setCreating] = useState(false);
  const [notice, setNotice] = useState({ token: "", text: "" });
  function setStatus(text: string) { setNotice({ token: current.current?.sessionToken ?? "", text }); }
  const [records, setRecords] = useState<Array<AttendeePacketCustody & { sessionToken: string }>>([]);
  const [now, setNow] = useState(Date.now);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => {
    const deadlines = records.map((record) => record.expiresAtMs).filter((expiry) => expiry > now);
    if (!deadlines.length) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.max(0, Math.min(...deadlines) - Date.now()));
    return () => window.clearTimeout(timer);
  }, [records, now]);
  function isCurrent(owner: RoomGuestSession) {
    return active.current && current.current?.sessionToken === owner.sessionToken &&
      current.current?.roomUid === owner.roomUid && current.current?.meetingId === owner.meetingId && !roomGuestSessionExpired(owner);
  }
  async function create() {
    const owner = current.current;
    if (!owner || !isCurrent(owner) || busy.current || !provider.trim() || !displayName.trim()) return;
    busy.current = true; setCreating(true); setStatus("");
    const request = { provider: provider.trim(), display_name: displayName.trim() };
    const key = JSON.stringify([owner.sessionToken, owner.roomUid, owner.meetingId, request]);
    const requestId = pending.current.get(key) ?? crypto.randomUUID();
    pending.current.set(key, requestId);
    try {
      const packet = await createCompanionAttendeeInvite(owner, { ...request, request_id: requestId });
      pending.current.delete(key);
      if (!isCurrent(owner)) return;
      setRecords((prior) => [...prior, { ...packet, sessionToken: owner.sessionToken }]);
      setStatus("동반 AI 초대를 만들었어요. 참가 안내를 복사해 AI를 실행할 컴퓨터에 전달해 주세요.");
    } catch (error) {
      if (isCurrent(owner)) setStatus(error instanceof Error ? error.message : "초대 결과를 확인하지 못했어요. 다시 시도해 주세요.");
    } finally {
      busy.current = false;
      if (active.current) setCreating(false);
    }
  }
  async function copy(inviteId: string) {
    const owner = current.current;
    const record = records.find((item) => item.result.invite_id === inviteId && item.sessionToken === owner?.sessionToken);
    if (!owner || !record) return;
    const assertCurrent = () => {
      if (!isCurrent(owner) || record.origin !== window.location.origin || record.expiresAtMs <= Date.now()) {
        throw new Error("현재 사용할 수 없는 초대예요. 방에 다시 연결한 뒤 확인해 주세요.");
      }
    };
    try {
      const copied = await copyText(attendeePacketText(record.result), async () => { assertCurrent(); return assertCurrent; });
      if (isCurrent(owner)) setStatus(copied ? "참가 안내를 복사했어요." : "복사하지 못했어요. 브라우저의 클립보드 권한을 확인해 주세요.");
    } catch (error) {
      if (isCurrent(owner)) setStatus(error instanceof Error ? error.message : "참가 안내를 복사하지 못했어요.");
    }
  }
  return {
    available: session !== null, provider, setProvider, displayName, setDisplayName, creating, status: notice.token === session?.sessionToken ? notice.text : "", create, copy,
    invites: records.filter((record) => record.sessionToken === session?.sessionToken && record.result.room_uid === session?.roomUid)
      .map((record) => ({ key: record.result.invite_id, displayName: record.result.display_name, provider: record.result.provider,
        expiresAt: record.result.expires_at, copyable: record.expiresAtMs > now && record.origin === window.location.origin })).reverse(),
  };
}

export type CompanionInviteControls = ReturnType<typeof useCompanionInvites>;
