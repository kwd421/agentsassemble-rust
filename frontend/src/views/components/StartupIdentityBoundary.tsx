import { useCallback, useState, type ReactNode } from "react";

import {
  getOrCreateBrowserCredential,
  getOrCreateClientId,
} from "../../lib/deviceIdentity";
import { isBundledDesktopWebview } from "../../lib/desktopBridge";
import { guestRecoveryRequestFromUrl } from "../../lib/guestRecovery";
import { consumeCentralOwnerConnectFromUrl } from "../../lib/central/ownerConnect";
import { clearStoredCentralOwnerWorkspace, type CentralOwnerWorkspace } from "../../lib/central/ownerWorkspace";
import CentralOwnerWorkspaceBoundary from "./CentralOwnerWorkspaceBoundary";
import {
  joinInviteTokenFromUrl,
  loadRoomGuestSession,
  operatorPairingTokenFromUrl,
  roomGuestSessionExpired,
} from "../../lib/roomGuestSession";
import { centralAccountEntryUrl, isCentralWebEntry } from "../../lib/central/identity";
import StartupIdentityGate from "./StartupIdentityGate";
import CentralOwnerConnectGate from "./CentralOwnerConnectGate";
import type { MemberReturn } from "../../lib/central/memberConnect";
import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

function browserEntranceHasAuthority(): boolean {
  const url = window.location.href;
  const guestSession = loadRoomGuestSession();
  return Boolean(
    joinInviteTokenFromUrl(url) ||
      operatorPairingTokenFromUrl(url) ||
      guestRecoveryRequestFromUrl(url) ||
      (guestSession && !guestSession.centralOwner && !roomGuestSessionExpired(guestSession))
  );
}

export default function StartupIdentityBoundary({
  children,
  memberReturn,
}: {
  children: (identity: { deviceToken: string; clientId: string }) => ReactNode;
  memberReturn?: MemberReturn;
}) {
  const [centralOwnerConnect] = useState(() => { clearStoredCentralOwnerWorkspace(); return consumeCentralOwnerConnectFromUrl(); });
  const [ownerWorkspace, setOwnerWorkspace] = useState<CentralOwnerWorkspace | null>(null);
  const [desktop] = useState(
    () => isBundledDesktopWebview() && !centralOwnerConnect
  );
  const [browserEntrance] = useState(
    () => !desktop && (Boolean(centralOwnerConnect || memberReturn?.record) || browserEntranceHasAuthority())
  );
  const [ready, setReady] = useState(browserEntrance && !centralOwnerConnect);
  const [browserIdentity] = useState(() => {
    if (!desktop && !browserEntrance) {
      return { deviceToken: "", clientId: "", error: "" };
    }
    try {
      return {
        deviceToken: getOrCreateBrowserCredential(),
        clientId: getOrCreateClientId(),
        error: "",
      };
    } catch (error) {
      return {
        deviceToken: "",
        clientId: "",
        error:
          error instanceof Error
            ? error.message
            : "이 브라우저에서는 안전한 입장 자격 증명을 사용할 수 없습니다.",
      };
    }
  });

  const finishOwnerEntry = useCallback((session: CentralOwnerWorkspace) => {
    setOwnerWorkspace(session);
    setReady(true);
  }, []);

  if (memberReturn && !memberReturn.record) return <GuestJoinProfilePanel displayName=""
    retryMode="join" status={memberReturn.error || "입장 기록이 없어요. 원래 초대 링크를 다시 열어 주세요."}
    onDisplayNameChange={() => {}} onAvatarImageChange={() => {}} onJoin={() => window.history.back()} />;

  if (isCentralWebEntry()) return <StartupIdentityGate deviceToken="" onComplete={finishCentralEntry} />;

  if (!desktop && !ready && !centralOwnerConnect) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
        <main
          className="grid w-full max-w-[520px] gap-3 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
          aria-label="브라우저 직접 시작 사용 불가"
        >
          <h1 className="text-2xl font-bold text-text-primary">
            AgentsAssemble에 로그인
          </h1>
          <p className="text-[14px] leading-6 text-text-muted">
            내 서버는 Google 계정으로 로그인해서 열어요.
          </p>
          {centralAccountEntryUrl() && <a href={centralAccountEntryUrl()} className="ops-cta mt-2 inline-flex min-h-11 items-center justify-center px-4 text-[15px]">Google로 로그인</a>}
          <p className="mt-1 text-[13px] leading-5 text-text-muted">
            초대받은 방은 호스트가 보낸 초대·기기 연결·복구 링크로 바로 들어갈 수 있어요.
          </p>
        </main>
      </div>
    );
  }

  if (browserIdentity.error) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
        <main
          className="grid w-full max-w-[520px] gap-3 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
          aria-label="브라우저 신원 사용 불가"
        >
          <h1 className="text-2xl font-black text-text-primary">
            안전한 브라우저 신원을 사용할 수 없습니다
          </h1>
          <p
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            {browserIdentity.error}
          </p>
        </main>
      </div>
    );
  }

  if (centralOwnerConnect && !ready) {
    return (
      <CentralOwnerConnectGate
        connect={centralOwnerConnect}
        deviceToken={browserIdentity.deviceToken}
        onComplete={finishOwnerEntry}
      />
    );
  }

  if (ready) {
    const content = children({ deviceToken: browserIdentity.deviceToken, clientId: browserIdentity.clientId });
    return ownerWorkspace ? <CentralOwnerWorkspaceBoundary session={ownerWorkspace}>{content}</CentralOwnerWorkspaceBoundary> : content;
  }
  return (
    <StartupIdentityGate
      deviceToken={browserIdentity.deviceToken}
      onComplete={() => setReady(true)}
    />
  );
}

// Central entry navigates through a bound server grant, never local bootstrap.
function finishCentralEntry() {}
