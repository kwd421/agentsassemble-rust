import MemberJoinPanel from "./MemberJoinPanel";
import GoogleGLogo from "./GoogleGLogo";
import RemoteMemberWorkspaceBoundary from "./RemoteMemberWorkspaceBoundary";
import { remoteWorkspaceVersion, remoteWorkspacesSnapshot, subscribeRemoteWorkspace, pendingMemberSnapshot, selectRemoteMember } from "../../lib/remote/remoteWorkspace";
import { useCallback, useState, useSyncExternalStore, type ReactNode } from "react";

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
import { retainedNativeMemberCredential, type MemberReturn } from "../../lib/central/memberConnect";
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
  // The version counter is the subscription snapshot (stable); the list is
  // read during render so no new array identity triggers a resubscribe loop.
  useSyncExternalStore(subscribeRemoteWorkspace, remoteWorkspaceVersion);
  const remotes = remoteWorkspacesSnapshot();
  // App-shell identity is explicit, never "most recent": the account's own
  // server owns the shell; otherwise the single live host does. Every live
  // host still wraps the content so its own end-state shows per host.
  const remote = remotes.find((entry) => entry.owner) ?? remotes[0] ?? null;
  const pendingMember = useSyncExternalStore(subscribeRemoteWorkspace, pendingMemberSnapshot);
  const [centralOwnerConnect] = useState(() => { clearStoredCentralOwnerWorkspace(); return consumeCentralOwnerConnectFromUrl(); });
  const [ownerWorkspace, setOwnerWorkspace] = useState<CentralOwnerWorkspace | null>(null);
  const [desktop] = useState(
    () => isBundledDesktopWebview() && !centralOwnerConnect
  );
  const [browserEntrance] = useState(
    () => !desktop && (Boolean(centralOwnerConnect || (memberReturn?.record || memberReturn?.connect)) || browserEntranceHasAuthority())
  );
  const [ready, setReady] = useState(browserEntrance && !centralOwnerConnect);
  const [browserIdentity] = useState(() => {
    if (!desktop && !browserEntrance) {
      return { deviceToken: "", clientId: "", error: "" };
    }
    try {
      return {
        deviceToken: memberReturn?.browserCredential || (loadRoomGuestSession()?.nativeMember ? retainedNativeMemberCredential() : getOrCreateBrowserCredential()),
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
            : "이 브라우저에서는 안전한 입장 자격 증명을 사용할 수 없어요.",
      };
    }
  });

  const finishOwnerEntry = useCallback((session: CentralOwnerWorkspace) => {
    setOwnerWorkspace(session);
    setReady(true);
  }, []);

  if (pendingMember) return <MemberJoinPanel secureEntry={pendingMember} onCancel={() => selectRemoteMember(null)} />;
  if (remote) {
    const content = children({ deviceToken: remote.deviceToken, clientId: remote.clientId });
    return remotes.reduceRight<ReactNode>((wrapped, entry) => entry.owner
      ? <CentralOwnerWorkspaceBoundary key={entry.owner.sessionId} session={entry.owner}>{wrapped}</CentralOwnerWorkspaceBoundary>
      : <RemoteMemberWorkspaceBoundary key={entry.transport.hello.channel_id} transport={entry.transport} serverId={entry.serverId}>{wrapped}</RemoteMemberWorkspaceBoundary>, content);
  }

  if (memberReturn && !memberReturn.record && !memberReturn.connect) return <GuestJoinProfilePanel displayName=""
    retryMode="join" status="참가를 준비하지 못했어요. 원래 초대 링크를 다시 열어 주세요."
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
          {centralAccountEntryUrl() && <a href={centralAccountEntryUrl()} className="dc-google-signin mt-2 min-h-11 text-[15px]"><GoogleGLogo />Google로 로그인</a>}
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
          aria-label="앱 시작 실패"
        >
          <h1 className="text-2xl font-black text-text-primary">
            앱을 시작하지 못했어요.
          </h1>
          <p
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            브라우저의 저장소 접근을 허용한 뒤 다시 열어 주세요.
          </p>
          <details><summary>자세히</summary><p className="break-all whitespace-pre-wrap">{browserIdentity.error}</p></details>
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
