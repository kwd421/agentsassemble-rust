import { useEffect, useRef, useState } from "react";
import { createConnectorInvite, type ConnectorInviteCustody } from "../api/connectorInvite";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { sameManagerAuthority } from "./useManagedHumanInvites";

type OriginProof = { publicOrigin: string; isCurrent: () => boolean };
type Pending = { authority: DesktopManagerRoomAuthority; requestId: string };
export type ConnectorInvitePresentation = { key: string; expiresAt: string; copyable: boolean };

export function useConnectorInvites({ roomDockId, publicOrigin, resolveManager, captureOriginRefresh, copyText, publishStatus }: {
  roomDockId: string;
  publicOrigin: string;
  resolveManager: (roomDockId: string) => DesktopManagerRoomAuthority;
  captureOriginRefresh: () => () => Promise<OriginProof | null>;
  copyText: (text: string, prepare: () => Promise<() => void>) => Promise<boolean>;
  publishStatus: (message: string) => void;
}) {
  const [records, setRecords] = useState<ConnectorInviteCustody[]>([]);
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
  function copyable(record: ConnectorInviteCustody, origin: string, time: number) {
    return record.origin === origin && record.expiresAtMs > time && authorityCurrent(record.authority);
  }
  async function create() {
    if (busy.current || !roomDockId) return;
    busy.current = true; setCreating(true);
    const refresh = captureOriginRefresh();
    let proof: OriginProof | null = null;
    try {
      const authority = resolveManager(roomDockId);
      let receipt = pending.current.get(roomDockId);
      if (!receipt || !sameManagerAuthority(receipt.authority, authority)) {
        receipt = { authority, requestId: crypto.randomUUID() };
        pending.current.set(roomDockId, receipt);
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
      const record = await createConnectorInvite(authority, { request_id: receipt.requestId, scope: "read_write" }, assertCurrent);
      // Keep the receipt after any uncertain response. Only a confirmed result releases it.
      pending.current.delete(roomDockId);
      if (!active.current) return;
      setRecords((prior) => [...prior, record]);
      assertCurrent();
      if (record.origin !== currentProof.publicOrigin) throw new Error("초대 주소가 변경됐어요. 현재 주소를 확인한 뒤 복사해 주세요.");
      publishStatus("외부 AI 초대를 만들었어요. 현재 AI 대화에 링크를 전달해 주세요.");
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
      const copied = await copyText(record.result.join_url, async () => {
        proof = await refresh();
        const currentProof = proof;
        function assertCurrent() {
          if (!active.current || !currentProof?.isCurrent() || !copyable(record!, currentProof.publicOrigin, Date.now())) {
            throw new Error("현재 초대 주소와 권한을 확인할 수 없어요. 다시 시도해 주세요.");
          }
        }
        assertCurrent(); return assertCurrent;
      });
      if (active.current && proof !== null && (proof as OriginProof).isCurrent()) publishStatus(copied ? "외부 AI 초대 링크를 복사했어요." : "링크를 복사하지 못했어요.");
    } catch (error) {
      if (active.current) publishStatus(error instanceof Error ? error.message : "링크를 복사하지 못했어요.");
    }
  }
  const invites: ConnectorInvitePresentation[] = records.filter((record) => authorityCurrent(record.authority)).map((record) => ({
    key: record.result.invite_id, expiresAt: record.result.expires_at, copyable: copyable(record, publicOrigin, now),
  })).reverse();
  return { invites, creating, create, copy };
}
