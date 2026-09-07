import { useEffect, useRef, useState } from "react";
import { revokeOperatorPairing, type OperatorPairingCustody } from "../api/operatorPairing";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { sameManagerAuthority } from "./useManagedHumanInvites";

type PairingRecord = {
  key: string;
  roomDockId: string;
  custody: OperatorPairingCustody;
  retired: boolean;
  state: "ready" | "revoking" | "unknown" | "revoked";
};

export type OperatorPairingPresentation = Readonly<{
  key: string;
  expiresAt: string;
  copyable: boolean;
  state: PairingRecord["state"];
  expired: boolean;
}>;

type OriginProof = { publicOrigin: string; isCurrent: () => boolean };
const RETIRED_COPY = Symbol("retired pairing copy");

export function useManagedOperatorPairings({
  roomDockId, publicOrigin, resolveManager, copyText, captureOriginRefresh, publishStatus,
}: {
  roomDockId: string;
  publicOrigin: string;
  resolveManager: (roomDockId: string) => DesktopManagerRoomAuthority;
  copyText: (text: string, prepare: () => Promise<() => void>) => Promise<boolean>;
  captureOriginRefresh: () => () => Promise<OriginProof | null>;
  publishStatus: (message: string) => void;
}) {
  const [records, setRecords] = useState<PairingRecord[]>([]);
  const recordsRef = useRef(records);
  const activeRef = useRef(true);
  const [now, setNow] = useState(Date.now);

  useEffect(() => {
    activeRef.current = true;
    return () => { activeRef.current = false; };
  }, []);

  // One deadline owns link-expiry presentation; there is no periodic refresh.
  useEffect(() => {
    const expiries = records.filter((record) => record.state === "ready")
      .map((record) => record.custody.expiresAtMs).filter((expiry) => expiry > now);
    if (!expiries.length) return;
    const timer = window.setTimeout(() => setNow(Date.now()),
      Math.min(Math.max(0, Math.min(...expiries) - Date.now()), 2_147_483_647));
    return () => window.clearTimeout(timer);
  }, [records, now]);

  function commit(update: (records: PairingRecord[]) => PairingRecord[]) {
    const next = update(recordsRef.current);
    recordsRef.current = next;
    setRecords(next);
  }

  function managerCurrent(record: PairingRecord) {
    try {
      return sameManagerAuthority(resolveManager(record.roomDockId), record.custody.authority);
    } catch {
      return false;
    }
  }

  function copyable(record: PairingRecord, origin: string, clock: number) {
    return !record.retired && record.state === "ready" &&
      record.custody.expiresAtMs > clock && record.custody.origin === origin &&
      managerCurrent(record);
  }

  function retain(custody: OperatorPairingCustody, sourceRoomDockId: string, current: boolean) {
    const record: PairingRecord = {
      key: `${custody.authority.server_id}:${custody.authority.room_uid}:${custody.pairingId}`,
      roomDockId: sourceRoomDockId, custody, retired: !current, state: "ready",
    };
    commit((prior) => [...prior, record]);
    setNow(Date.now());
  }

  async function copy(key: string) {
    const record = recordsRef.current.find((entry) => entry.key === key);
    if (!record || !copyable(record, publicOrigin, Date.now())) return;
    const refresh = captureOriginRefresh();
    function assertCurrent(proof: OriginProof) {
      if (!activeRef.current || !proof.isCurrent()) throw RETIRED_COPY;
      const latest = recordsRef.current.find((entry) => entry.key === key);
      if (!latest || !copyable(latest, proof.publicOrigin, Date.now())) {
        throw new Error("연결 링크가 변경되었거나 만료됐어요. 새 링크를 만들어 주세요.");
      }
    }
    try {
      let lastProof: OriginProof | null = null;
      const copied = await copyText(record.custody.pairingUrl, async () => {
        const proof = await refresh();
        if (!proof) throw RETIRED_COPY;
        assertCurrent(proof);
        lastProof = proof;
        return () => assertCurrent(proof);
      });
      if (!lastProof) return;
      assertCurrent(lastProof);
      publishStatus(copied ? "기기 연결 링크를 복사했어요." : "링크를 복사하지 못했어요.");
    } catch (error) {
      if (error === RETIRED_COPY) return;
      if (activeRef.current) publishStatus(error instanceof Error ? error.message : "링크 복사에 실패했어요.");
    }
  }

  async function revoke(key: string) {
    const record = recordsRef.current.find((entry) => entry.key === key);
    if (!record || record.state === "revoking" || record.state === "revoked") return;
    commit((prior) => prior.map((entry) => entry.key === key ? { ...entry, state: "revoking" } : entry));
    try {
      await revokeOperatorPairing(record.custody, () => {
        if (!activeRef.current || !managerCurrent(record)) {
          throw new Error("현재 방의 연결 해제 권위를 확인할 수 없어요.");
        }
      });
      if (!activeRef.current) return;
      commit((prior) => prior.map((entry) => entry.key === key ? { ...entry, state: "revoked" } : entry));
      publishStatus("이 링크와 연결된 기기의 방 접속을 해제했어요.");
    } catch (error) {
      if (!activeRef.current) return;
      // An unconfirmed revocation never returns a link to copyable state.
      commit((prior) => prior.map((entry) => entry.key === key ? { ...entry, state: "unknown" } : entry));
      publishStatus(error instanceof Error ? error.message : "연결 해제 결과를 확인할 수 없어요. 다시 시도해 주세요.");
    }
  }

  const pairings: OperatorPairingPresentation[] = records
    .filter((record) => record.roomDockId === roomDockId).map((record) => ({
      key: record.key, expiresAt: record.custody.expiresAt,
      copyable: copyable(record, publicOrigin, now), state: record.state,
      expired: record.custody.expiresAtMs <= now,
    })).reverse();
  return { pairings, retain, copy, revoke };
}
