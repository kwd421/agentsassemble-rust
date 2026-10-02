import { useState, type ReactNode } from "react";

import {
  getOrCreateBrowserCredential,
  getOrCreateClientId,
} from "../../lib/deviceIdentity";
import { isBundledDesktopWebview } from "../../lib/desktopBridge";
import { guestRecoveryRequestFromUrl } from "../../lib/guestRecovery";
import { consumeCentralOwnerConnectFromUrl } from "../../lib/centralOwnerConnect";
import {
  joinInviteTokenFromUrl,
  loadRoomGuestSession,
  operatorPairingTokenFromUrl,
  roomGuestSessionExpired,
} from "../../lib/roomGuestSession";
import { centralAccountEntryUrl, isCentralWebEntry } from "../../lib/centralIdentity";
import StartupIdentityGate from "./StartupIdentityGate";
import CentralOwnerConnectGate from "./CentralOwnerConnectGate";

function browserEntranceHasAuthority(): boolean {
  const url = window.location.href;
  const guestSession = loadRoomGuestSession();
  return Boolean(
    joinInviteTokenFromUrl(url) ||
      operatorPairingTokenFromUrl(url) ||
      guestRecoveryRequestFromUrl(url) ||
      (guestSession && !roomGuestSessionExpired(guestSession))
  );
}

export default function StartupIdentityBoundary({
  children,
}: {
  children: (identity: { deviceToken: string; clientId: string }) => ReactNode;
}) {
  const [centralOwnerConnect] = useState(consumeCentralOwnerConnectFromUrl);
  const [desktop] = useState(
    () => isBundledDesktopWebview() && !centralOwnerConnect
  );
  const [browserEntrance] = useState(
    () => !desktop && (Boolean(centralOwnerConnect) || browserEntranceHasAuthority())
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

  if (isCentralWebEntry()) return <StartupIdentityGate deviceToken="" onComplete={finishCentralEntry} />;

  if (!desktop && !ready && !centralOwnerConnect) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
        <main
          className="grid w-full max-w-[520px] gap-3 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
          aria-label="브라우저 직접 시작 사용 불가"
        >
          <h1 className="text-2xl font-black text-text-primary">
            AgentsAssemble에 로그인
          </h1>
          {centralAccountEntryUrl() && <a href={centralAccountEntryUrl()} className="ops-button">Google 로그인 · 내 서버 열기</a>}
          <p
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            내 서버는 중앙 계정으로 로그인해 열 수 있어요. 초대받은 방은 호스트에게 받은 초대·기기 연결·복구 링크로 들어가세요.
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
        onComplete={() => setReady(true)}
      />
    );
  }

  if (ready) {
    return children({
      deviceToken: browserIdentity.deviceToken,
      clientId: browserIdentity.clientId,
    });
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
