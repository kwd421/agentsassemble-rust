import { useEffect, useRef, useState } from "react";
import { createConnectorInvite, type ConnectorInviteCustody } from "../api/connectorInvite";
import { createFriendAttendeeInvite, attendeePacketText, type AttendeePacketCustody } from "../api/attendeeInvite";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { sameManagerAuthority } from "./useManagedHumanInvites";

type OriginProof = { publicOrigin: string; isCurrent: () => boolean };
type Pending = { authority: DesktopManagerRoomAuthority; requestId: string };
export type ConnectorInvitePresentation = { key: string; expiresAt: string; copyable: boolean };
export type AttendeeInvitePresentation = ConnectorInvitePresentation & { displayName: string; provider: string };
type ManagedAiInvite = (ConnectorInviteCustody & { kind: "connector" }) | (AttendeePacketCustody & { kind: "attendee"; authority: DesktopManagerRoomAuthority });

export function useManagedAiInvites({ roomDockId, publicOrigin, resolveManager, captureOriginRefresh, copyText, publishStatus }: {
  roomDockId: string;
  publicOrigin: string;
  resolveManager: (roomDockId: string) => DesktopManagerRoomAuthority;
  captureOriginRefresh: () => () => Promise<OriginProof | null>;
  copyText: (text: string, prepare: () => Promise<() => void>) => Promise<boolean>;
  publishStatus: (message: string) => void;
}) {
  const [records, setRecords] = useState<ManagedAiInvite[]>([]);
  const pending = useRef(new Map<string, Pending>());
  const busy = useRef(false);
  const active = useRef(true);
  const [creating, setCreating] = useState(false);
  const [now, setNow] = useState(Date.now);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  useEffect(() => {
    const future = records.map((record) => record.expiresAtMs).filter((expiry) => expiry > now);
    if (!future.length) return;
    const timer = window.setTimeout(() => setNow(Date.now()), Math.max(0, Math.min(...future) - Date.now()));
    return () => window.clearTimeout(timer);
  }, [records, now]);

  function authorityCurrent(authority: DesktopManagerRoomAuthority) {
    try { return sameManagerAuthority(resolveManager(roomDockId), authority); }
    catch { return false; }
  }
  function copyable(record: ManagedAiInvite, origin: string, time: number) {
    return record.origin === origin && record.expiresAtMs > time && authorityCurrent(record.authority);
  }
  async function create(friendId?: string) {
    if (busy.current || !roomDockId) return;
    busy.current = true; setCreating(true);
    const refresh = captureOriginRefresh();
    let proof: OriginProof | null = null;
    try {
      const authority = resolveManager(roomDockId);
      const pendingKey = JSON.stringify([roomDockId, friendId ?? null]);
      let receipt = pending.current.get(pendingKey);
      if (!receipt || !sameManagerAuthority(receipt.authority, authority)) {
        receipt = { authority, requestId: crypto.randomUUID() };
        pending.current.set(pendingKey, receipt);
      }
      proof = await refresh();
      if (!proof || !proof.publicOrigin) return;
      const currentProof = proof;
      function assertCurrent() {
        if (!active.current || !currentProof.isCurrent() || !authorityCurrent(authority)) {
          throw new Error("방이나 초대 주소가 변경됐어요. 현재 방에서 다시 시도해 주세요.");
        }
      }
      assertCurrent();
      const record: ManagedAiInvite = friendId
        ? { ...await createFriendAttendeeInvite(authority, { request_id: receipt.requestId, friend_id: friendId }, assertCurrent), kind: "attendee", authority }
        : { ...await createConnectorInvite(authority, { request_id: receipt.requestId, scope: "read_write" }, assertCurrent), kind: "connector" };
      // Keep the receipt after any uncertain response. Only a confirmed result releases it.
      pending.current.delete(pendingKey);
      if (!active.current) return;
      setRecords((prior) => [...prior, record]);
      assertCurrent();
      if (record.origin !== currentProof.publicOrigin) throw new Error("초대 주소가 변경됐어요. 현재 주소를 확인한 뒤 복사해 주세요.");
      publishStatus(friendId ? "AI 친구 초대를 만들었어요. 참가 안내를 복사해 전달해 주세요." : "외부 AI 초대를 만들었어요. 현재 AI 대화에 링크를 전달해 주세요.");
    } catch (error) {
      if (active.current && (!proof || proof.isCurrent())) publishStatus(error instanceof Error ? error.message : "외부 AI 초대를 만들지 못했어요. 다시 시도해 주세요.");
    } finally {
      busy.current = false;
      if (active.current) setCreating(false);
    }
  }
  async function copy(key: string) {
    const record = records.find((record) => record.result.invite_id === key);
    if (!record || !copyable(record, publicOrigin, Date.now())) return;
    const refresh = captureOriginRefresh();
    let proof: OriginProof | null = null;
    try {
      const copied = await copyText(record.kind === "attendee" ? attendeePacketText(record.result) : record.result.join_url, async () => {
        proof = await refresh();
        const currentProof = proof;
        function assertCurrent() {
          if (!active.current || !currentProof?.isCurrent() || !copyable(record!, currentProof.publicOrigin, Date.now())) {
            throw new Error("현재 초대 주소와 권한을 확인할 수 없어요. 다시 시도해 주세요.");
          }
        }
        assertCurrent(); return assertCurrent;
      });
      if (active.current && proof !== null && (proof as OriginProof).isCurrent()) publishStatus(copied ? record.kind === "attendee" ? "참가 안내를 복사했어요." : "외부 AI 초대 링크를 복사했어요." : "초대 내용을 복사하지 못했어요.");
    } catch (error) {
      if (active.current) publishStatus(error instanceof Error ? error.message : "링크를 복사하지 못했어요.");
    }
  }
  const currentRecords = records.filter((record) => authorityCurrent(record.authority));
  const invites: ConnectorInvitePresentation[] = currentRecords.filter((record) => record.kind === "connector").map((record) => ({
    key: record.result.invite_id, expiresAt: record.result.expires_at, copyable: copyable(record, publicOrigin, now),
  })).reverse();
  const attendeeInvites: AttendeeInvitePresentation[] = currentRecords.filter((record) => record.kind === "attendee").map((record) => ({
    key: record.result.invite_id, expiresAt: record.result.expires_at, copyable: copyable(record, publicOrigin, now), displayName: record.result.display_name, provider: record.result.provider,
  })).reverse();
  return { invites, attendeeInvites, creating, create, copy };
}
