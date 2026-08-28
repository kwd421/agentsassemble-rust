import { useEffect, useRef, useState } from "react";

import {
  HumanInviteDispatchError,
  revokeManagedHumanInvite,
  type ManagedHumanInviteCustody,
} from "../api";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";

export type HumanInviteRevocationState =
  | "idle"
  | "in_flight"
  | "unknown"
  | "dead";

type ManagedHumanInviteRecord = {
  key: string;
  roomDockId: string;
  displayName: string;
  maxUses: number;
  ttlSeconds: number;
  operationGeneration: number;
  retired: boolean;
  revocation: HumanInviteRevocationState;
  revokeGeneration: number;
  custody: ManagedHumanInviteCustody;
};

export type HumanInvitePresentation = Readonly<{
  key: string;
  displayName: string;
  maxUses: number;
  ttlSeconds: number;
  expiresAt: string;
  expired: boolean;
  retired: boolean;
  originCurrent: boolean;
  authorityCurrent: boolean;
  revocation: HumanInviteRevocationState;
  copyUrl: string;
}>;

export function sameManagerAuthority(
  left: DesktopManagerRoomAuthority | null,
  right: DesktopManagerRoomAuthority
) {
  return Boolean(
    left &&
      left.server_id === right.server_id &&
      left.authority_lineage_id === right.authority_lineage_id &&
      left.room_id === right.room_id &&
      left.room_uid === right.room_uid
  );
}

function humanInviteKey(custody: ManagedHumanInviteCustody) {
  return `${custody.authority.server_id}\0${custody.authority.authority_lineage_id}\0${custody.authority.room_uid}\0${custody.inviteId}`;
}

function presentHumanInvite(
  record: ManagedHumanInviteRecord,
  currentAuthority: DesktopManagerRoomAuthority | null,
  publicOrigin: string,
  now: number
): HumanInvitePresentation {
  const expired = record.custody.expiresAt.epochMilliseconds <= now;
  const originCurrent = Boolean(
    publicOrigin && publicOrigin === record.custody.responseOrigin
  );
  const authorityCurrent = sameManagerAuthority(
    currentAuthority,
    record.custody.authority
  );
  const copyable =
    !record.retired &&
    record.revocation === "idle" &&
    !expired &&
    originCurrent &&
    authorityCurrent;
  return Object.freeze({
    key: record.key,
    displayName: record.displayName,
    maxUses: record.maxUses,
    ttlSeconds: record.ttlSeconds,
    expiresAt: record.custody.expiresAt.exact,
    expired,
    retired: record.retired,
    originCurrent,
    authorityCurrent,
    revocation: record.revocation,
    copyUrl: copyable ? record.custody.joinUrl : "",
  });
}

type UseManagedHumanInvitesOptions = {
  modalRoomDockId: string;
  currentPublicOrigin: string;
  resolveManagerRoomAuthority: (roomDockId: string) => DesktopManagerRoomAuthority;
  copyText: (
    value: string,
    prepareDispatch?: () => Promise<() => void>
  ) => Promise<boolean>;
  refreshCurrentPublicOrigin: () => Promise<HumanInviteOriginProof | null>;
  publishStatus: (status: string) => void;
};

type HumanInviteOriginProof = Readonly<{
  publicOrigin: string;
  isCurrent: () => boolean;
}>;

const COPY_NO_LONGER_ELIGIBLE = Symbol("managed human invite copy is no longer eligible");

export function useManagedHumanInvites({
  modalRoomDockId,
  currentPublicOrigin,
  resolveManagerRoomAuthority,
  copyText,
  refreshCurrentPublicOrigin,
  publishStatus,
}: UseManagedHumanInvitesOptions) {
  const [records, setRecords] = useState<ManagedHumanInviteRecord[]>([]);
  const [clockNow, setClockNow] = useState(() => Date.now());
  const recordsRef = useRef<ManagedHumanInviteRecord[]>([]);
  const activeRef = useRef(true);

  function commit(
    update: (current: ManagedHumanInviteRecord[]) => ManagedHumanInviteRecord[]
  ) {
    const next = update(recordsRef.current);
    recordsRef.current = next;
    setRecords(next);
    return next;
  }

  function resolveExactAuthority(roomDockId: string) {
    try {
      return resolveManagerRoomAuthority(roomDockId);
    } catch {
      return null;
    }
  }

  useEffect(() => {
    activeRef.current = true;
    return () => {
      activeRef.current = false;
    };
  }, []);

  useEffect(() => {
    const nearestExpiry = records.reduce<number | null>((nearest, record) => {
      if (
        record.revocation === "dead" ||
        record.custody.expiresAt.epochMilliseconds <= clockNow
      ) {
        return nearest;
      }
      return nearest === null
        ? record.custody.expiresAt.epochMilliseconds
        : Math.min(nearest, record.custody.expiresAt.epochMilliseconds);
    }, null);
    if (nearestExpiry === null) return;
    const delay = Math.max(0, nearestExpiry - Date.now());
    const timer = window.setTimeout(
      () => setClockNow(Date.now()),
      Math.min(delay, 2_147_483_647)
    );
    return () => window.clearTimeout(timer);
  }, [clockNow, records]);

  function retainAccepted({
    roomDockId,
    displayName,
    maxUses,
    ttlSeconds,
    operationGeneration,
    current,
    custody,
  }: {
    roomDockId: string;
    displayName: string;
    maxUses: number;
    ttlSeconds: number;
    operationGeneration: number;
    current: boolean;
    custody: ManagedHumanInviteCustody;
  }) {
    const record: ManagedHumanInviteRecord = {
      key: humanInviteKey(custody),
      roomDockId,
      displayName,
      maxUses,
      ttlSeconds,
      operationGeneration,
      retired: !current,
      revocation: "idle",
      revokeGeneration: 0,
      custody,
    };
    commit((existingRecords) => [
      ...existingRecords.map((existing) =>
        current &&
        existing.roomDockId === roomDockId &&
        existing.operationGeneration < operationGeneration
          ? { ...existing, retired: true }
          : existing
      ),
      record,
    ]);
    setClockNow(Date.now());
    return record;
  }

  const currentAuthority = modalRoomDockId
    ? resolveExactAuthority(modalRoomDockId)
    : null;
  const humanInvites = records
    .filter((record) => record.roomDockId === modalRoomDockId)
    .map((record) =>
      presentHumanInvite(record, currentAuthority, currentPublicOrigin, clockNow)
    )
    .reverse();
  const currentInvite = humanInvites.find((invite) => !invite.retired);

  function assertCopyEligible(
    expected: ManagedHumanInviteRecord,
    originProof: HumanInviteOriginProof
  ) {
    if (!originProof.isCurrent()) throw COPY_NO_LONGER_ELIGIBLE;
    const latest = recordsRef.current.find(
      (candidate) =>
        candidate.key === expected.key && candidate.custody === expected.custody
    );
    if (
      !latest ||
      presentHumanInvite(
        latest,
        resolveExactAuthority(latest.roomDockId),
        originProof.publicOrigin,
        Date.now()
      ).copyUrl !== expected.custody.joinUrl
    ) {
      throw COPY_NO_LONGER_ELIGIBLE;
    }
  }

  async function copy(key: string) {
    const record = recordsRef.current.find((candidate) => candidate.key === key);
    if (!record) return;
    const presentation = presentHumanInvite(
      record,
      resolveExactAuthority(record.roomDockId),
      currentPublicOrigin,
      Date.now()
    );
    if (!presentation.copyUrl) {
      publishStatus("현재 확인된 활성 사람 초대만 복사할 수 있습니다.");
      return;
    }
    let latestProof: HumanInviteOriginProof | null = null;
    try {
      const copied = await copyText(presentation.copyUrl, async () => {
        const proof = await refreshCurrentPublicOrigin();
        if (!proof) throw COPY_NO_LONGER_ELIGIBLE;
        assertCopyEligible(record, proof);
        latestProof = proof;
        return () => assertCopyEligible(record, proof);
      });
      if (!latestProof) return;
      assertCopyEligible(record, latestProof);
      publishStatus(copied ? "보안 초대 링크 복사됨" : "보안 초대 링크 복사 실패");
    } catch (error) {
      if (error === COPY_NO_LONGER_ELIGIBLE) return;
      publishStatus(
        error instanceof Error
          ? error.message
          : "현재 공개 초대 상태를 확인하지 못했습니다."
      );
    }
  }

  function revokeAttemptIsCurrent(key: string, generation: number) {
    const record = recordsRef.current.find((candidate) => candidate.key === key);
    return Boolean(
      activeRef.current &&
        record?.revocation === "in_flight" &&
        record.revokeGeneration === generation
    );
  }

  async function revoke(key: string) {
    const record = recordsRef.current.find((candidate) => candidate.key === key);
    if (
      !record ||
      record.revocation === "in_flight" ||
      record.revocation === "dead"
    ) {
      return;
    }
    const source = record.revocation;
    const generation = record.revokeGeneration + 1;
    commit((existingRecords) =>
      existingRecords.map((candidate) =>
        candidate.key === key
          ? { ...candidate, revocation: "in_flight", revokeGeneration: generation }
          : candidate
      )
    );
    publishStatus("사람 초대 폐기 중...");
    try {
      const result = await revokeManagedHumanInvite(record.custody, () => {
        if (!revokeAttemptIsCurrent(key, generation)) {
          throw new Error("사람 초대 폐기 작업이 대체되었습니다.");
        }
      });
      if (!revokeAttemptIsCurrent(key, generation)) return;
      commit((existingRecords) =>
        existingRecords.map((candidate) =>
          candidate.key === key ? { ...candidate, revocation: "dead" } : candidate
        )
      );
      publishStatus(
        result === "invite_not_found"
          ? "사람 초대가 이미 폐기되었습니다."
          : "사람 초대를 폐기했습니다."
      );
    } catch (error) {
      if (!revokeAttemptIsCurrent(key, generation)) return;
      const nextState: HumanInviteRevocationState =
        error instanceof HumanInviteDispatchError &&
        error.outcome === "proven_not_dispatched"
          ? source
          : "unknown";
      commit((existingRecords) =>
        existingRecords.map((candidate) =>
          candidate.key === key
            ? { ...candidate, revocation: nextState }
            : candidate
        )
      );
      publishStatus(
        nextState === "unknown"
          ? "사람 초대 폐기 결과를 확인할 수 없습니다. 명시적으로 다시 시도할 수 있습니다."
          : "사람 초대 폐기 요청이 전송되지 않았습니다."
      );
    }
  }

  return {
    humanInvites,
    secureInviteUrl: currentInvite?.copyUrl || "",
    retainAccepted,
    copy,
    copyCurrent: () => (currentInvite ? copy(currentInvite.key) : Promise.resolve()),
    revoke,
  };
}
