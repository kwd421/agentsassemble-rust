import { useState, type ReactNode } from "react";

import { getOrCreateBrowserCredential } from "../../lib/deviceIdentity";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { guestRecoveryRequestFromUrl } from "../../lib/guestRecovery";
import {
  joinInviteTokenFromUrl,
  loadRoomGuestSession,
  operatorPairingTokenFromUrl,
  roomGuestSessionExpired,
} from "../../lib/roomGuestSession";
import StartupIdentityGate from "./StartupIdentityGate";

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
  children: ReactNode;
}) {
  const desktop = isDesktopWebview();
  const [ready, setReady] = useState(
    () => !desktop && browserEntranceHasAuthority()
  );
  const [browserCredential] = useState(() => {
    if (!desktop) return { deviceToken: "", error: "" };
    try {
      return { deviceToken: getOrCreateBrowserCredential(), error: "" };
    } catch (error) {
      return {
        deviceToken: "",
        error:
          error instanceof Error
            ? error.message
            : "이 브라우저에서는 안전한 입장 자격 증명을 사용할 수 없습니다.",
      };
    }
  });

  if (!desktop && !ready) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
        <main
          className="grid w-full max-w-[520px] gap-3 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
          aria-label="브라우저 직접 시작 사용 불가"
        >
          <h1 className="text-2xl font-black text-text-primary">
            데스크톱 앱에서 시작해 주세요
          </h1>
          <p
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            이 브라우저에는 서버가 소유하는 시작 권위가 없습니다. 방 초대·운영자 연결·복구
            링크로 들어오거나 데스크톱 앱을 사용해 주세요.
          </p>
        </main>
      </div>
    );
  }

  if (browserCredential.error) {
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
            {browserCredential.error}
          </p>
        </main>
      </div>
    );
  }

  if (ready) return <>{children}</>;
  return (
    <StartupIdentityGate
      deviceToken={browserCredential.deviceToken}
      onComplete={() => setReady(true)}
    />
  );
}
