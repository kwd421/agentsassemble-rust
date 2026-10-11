import AppUpdateIndicator from "./AppUpdateIndicator";
import CentralGuestEntry, { type CentralGuestScreen } from "./CentralGuestEntry";
import ServerRestoreCard from "./ServerRestoreCard";
import LocalAccountDataChoice from "./LocalAccountDataChoice";
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import GoogleRegistrationChoice from "./GoogleRegistrationChoice";
import { useCentralDirectory } from "../../app/useCentralDirectory";
import { isCentralTemporaryError } from "../../lib/central/connectionError";
import type { CentralServerDisplay } from "../../lib/central/directoryCache";
import ConnectionBanner from "./ConnectionBanner";
import type { HostDeviceInfo } from "../../types/generated/HostDeviceInfo";
import CentralServerList from "./CentralServerList";
import FreshHostRegistrationButton from "./FreshHostRegistrationButton";
import CentralAccountSettings from "./CentralAccountSettings";
import { startCentralWebGoogle, completeCentralWebGoogleReturn } from "../../lib/central/webGoogle";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  LoaderCircle,
  LogIn,
  RefreshCw,
} from "lucide-react";

import {
  centralIdentityConfigured,
  isCentralWebEntry,
  centralSessionLoggedOut,
  isCentralAuthenticationError,
  loadCentralSession,
  loadPendingCentralRecoveryCode,
  loginCentralGoogle,
  logoutCentral,
  openCentralOwnedServer, openCentralMemberServer,
  registerLocalServer, retryPendingLocalDemotion,
} from "../../lib/central/identity";
import {
  fetchDesktopOperatorRuntime,
  requestDesktopHostProductSurface,
  requestDesktopHostDeviceInfo,
  requestDesktopBootstrapStatus,
  type DesktopBootstrapGrant,
} from "../../lib/desktopBridge";
import { createSecureRequestId } from "../../lib/secureRequestId";
import {
  hydratePersistedRoom,
  mergeServerRoomsIntoDock,
  persistableRoom,
} from "../../lib/roomDockModel";
import {
  loadRoomDockItems,
  persistRoomDockItems,
} from "../../lib/roomDockPersistence";
import {
  bindRoomDirectoryAuthority,
  parseStrictRoomDirectory,
} from "../../lib/roomDirectoryContract";

import { saveLocalProfile } from "../../lib/localProfile";

type Screen = CentralGuestScreen | "servers";

// Keep native strings and browser errors available without exposing them by default.
function failureMessage(reason: unknown, message: string): ReactNode {
  const detail = reason instanceof Error ? reason.message : typeof reason === "string" ? reason : "";
  return <>
    <p>{message}</p>
    <p>잠시 후 다시 시도해 주세요.</p>
    {detail && <details><summary>자세히</summary><p className="break-all whitespace-pre-wrap">{detail}</p></details>}
  </>;
}

export default function StartupIdentityGate({
  deviceToken,
  onComplete,
}: {
  deviceToken: string;
  onComplete: () => void;
}) {
  const centralEnabled = centralIdentityConfigured();
  const webEntry = isCentralWebEntry();
  const [googleRegistration, setGoogleRegistration] = useState<GoogleRegistrationRequired | null>(null);
  const [screen, setScreen] = useState<Screen>("choice");
  const [accountSettingsOpen, setAccountSettingsOpen] = useState(() => webEntry && new URLSearchParams(window.location.search).get("account") === "settings");
  const [displayName, setDisplayName] = useState("");
  const [hostingState, setHostingState] = useState<"device" | "retired" | "account_deleted" | null>(null);
  const [hostingChecked, setHostingChecked] = useState(false);
  const [deviceAccount, setDeviceAccount] = useState<string | null>(null);
  const autoOpened = useRef(false);
  const [localHostError, setLocalHostError] = useState<ReactNode>("");
  const [localHost, setLocalHost] = useState<HostDeviceInfo | null>(null);
  const { directory, refresh: refreshCentral } = useCentralDirectory();
  const [connectingServerId, setConnectingServerId] = useState("");
  const centralUnavailable = directory?.status === "central-unconfirmed" || directory?.status === "error";
  const centralPerson = directory?.person;
  const accountDeviceOnly = Boolean(centralPerson && deviceAccount === centralPerson.person_id);
  const centralServers = directory?.servers || [];
  const ownerConflict = directory?.live?.owner_server_conflict;
  const ownedServers = centralServers.filter(server => server.relation === "owner");
  const ownedServer = ownedServers.length === 1 ? ownedServers[0] : undefined;
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("저장된 사용자 확인 중");
  const [error, setError] = useState<ReactNode>("");
  const googleAbortController = useRef<AbortController | null>(null);
  const bootstrapRequestId = useRef(createSecureRequestId());

  useEffect(
    () => () => {
      googleAbortController.current?.abort();
    },
    []
  );

  async function enterApplication(expectedDesktopAuthority?: DesktopBootstrapGrant) {
    setChecking(true);
    setStatus("로컬 엔진과 방 목록을 준비하는 중");
    const desktopAuthority =
      expectedDesktopAuthority || (await requestDesktopBootstrapStatus());
    if (desktopAuthority.phase !== "complete") {
      throw new Error("완료된 데스크톱 권위가 방 목록을 소유하지 않습니다.");
    }
    const response = await fetchDesktopOperatorRuntime("/api/rooms", {
      cache: "no-store",
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const payload = parseStrictRoomDirectory(await response.json());
    if (
      payload.server_id !== desktopAuthority.server_id ||
      payload.authority_lineage_id !== desktopAuthority.authority_lineage_id
    ) {
      throw new Error("방 목록 권위가 네이티브 bootstrap 계보와 일치하지 않습니다.");
    }
    await bindRoomDirectoryAuthority(
      payload,
      {
        revision: desktopAuthority.server_product_surface_revision,
        digest: desktopAuthority.server_product_surface_digest,
      }
    );
    const current = loadRoomDockItems().map(hydratePersistedRoom);
    const synchronized = mergeServerRoomsIntoDock(
      current,
      payload.rooms,
      window.location.origin,
      payload.server_id
    );
    persistRoomDockItems(synchronized.map(persistableRoom));
    onComplete();
  }

  const finishCentralStartup = useCallback(async () => {
    const result = await refreshCentral();
    if (!result.person) throw result.error || new Error("로그인이 필요해요. 다시 로그인해 주세요.");
    setScreen("servers"); setChecking(false);
  }, [refreshCentral]);

  useEffect(() => {
    if (screen !== "servers" || !directory) return;
    if (directory.status === "authentication-required") {
      setScreen("choice");
      setError("로그인이 만료됐어요. 다시 로그인해 주세요.");
    } else if (directory.status === "error") {
      setError(failureMessage(directory.error, "서버 목록을 확인하지 못했어요."));
    }
  }, [directory, screen]);

  async function selectCentralServer(server?: CentralServerDisplay, name?: string) {
    if (busy || !centralPerson) return;
    setConnectingServerId(server?.server_id || localHost?.server_id || "local");
    setBusy(true);
    setError("");
    try {
      if (server) {
        const current = await refreshCentral();
        const live = current.live?.servers.find(item => item.server_id === server.server_id);
        if (!live) throw new Error("서버 연결이 끊겼어요. 로그인 서버 연결을 다시 확인해 주세요.");
        if (live.relation === "member") await openCentralMemberServer(live);
        else await openCentralOwnedServer(live);
      }
      else {
        if (webEntry) throw new Error("서버를 실행하려면 이 기기의 앱을 열어 주세요.");
        // Only this host's local authority opens its local room directory.
        const current = await refreshCentral();
        if (!current.person) throw current.error || new Error("로그인이 필요해요. 다시 로그인해 주세요.");
        if (current.live?.owner_server_conflict) {
          autoOpened.current = false;
          setScreen("servers");
          return;
        }
        if (current.status === "central-unconfirmed") {
          // Cached identity is presentation only; never initialize a new operator from it.
          await enterApplication(await requestDesktopBootstrapStatus());
        } else {
          const authority = await saveLocalProfile(current.person.display_name, bootstrapRequestId.current, current.person);
          try { await registerLocalServer(deviceToken, name); }
          catch (reason) { if (!isCentralTemporaryError(reason)) throw reason; }
          await enterApplication(authority);
        }
      }
    } catch (reason) {
      setChecking(false);
      if (reason instanceof Error && "code" in reason && (reason.code === "server_exists" || reason.code === "server_retired" || reason.code === "registration_absent")) {
        if (reason.code === "server_exists") setDeviceAccount(centralPerson.person_id);
        const restriction = await refreshLocalHost();
        if (reason.code === "server_exists" || restriction) await refreshCentral();
        else setError(failureMessage(reason, "서버를 열지 못했어요."));
      } else setError(failureMessage(reason, "서버를 열지 못했어요."));
    } finally {
      setConnectingServerId(""); setBusy(false);
    }
  }

  useEffect(() => {
    if (screen !== "servers" || webEntry || !hostingChecked || hostingState || accountDeviceOnly || busy || autoOpened.current || !localHost?.server_id) return;
    if (ownedServer?.server_id !== localHost.server_id || !["connected", "central-unconfirmed"].includes(directory?.status || "")) return;
    autoOpened.current = true;
    void selectCentralServer();
  }, [screen, webEntry, hostingChecked, hostingState, accountDeviceOnly, busy, ownedServer?.server_id, localHost?.server_id, directory?.status]);

  async function refreshLocalHost() {
    if (webEntry) return;
    try {
      await retryPendingLocalDemotion(deviceToken);
      const device = await requestDesktopHostDeviceInfo();
      setLocalHost(device);
      const restriction = device.hosting_state;
      setHostingState(restriction); setHostingChecked(true);
      setLocalHostError("");
      return restriction;
    } catch (reason) {
      setLocalHost(null);
      setLocalHostError(failureMessage(reason, "이 기기의 서버 정보를 확인하지 못했어요."));
    }
  }

  async function refreshServers() {
    if (busy) return;
    setBusy(true); setError("");
    try { await refreshLocalHost(); autoOpened.current = false; await finishCentralStartup(); }
    catch (reason) { setError(failureMessage(reason, "서버 목록을 불러오지 못했어요.")); }
    finally { setBusy(false); }
  }

  async function logout() {
    if (busy) return;
    setBusy(true); setError("");
    try {
      await logoutCentral();
      setScreen("choice");
    } catch (reason) { setError(failureMessage(reason, "로그아웃하지 못했어요.")); }
    finally { setBusy(false); }
  }

  useEffect(() => {
    let active = true;
    async function initialize() {
      try {
        if (webEntry) await completeCentralWebGoogleReturn();
        else {
          await requestDesktopHostProductSurface();
          if (centralEnabled) {
            try {
              await retryPendingLocalDemotion(deviceToken);
              const device = await requestDesktopHostDeviceInfo();
              const restriction = device.hosting_state;
              if (active) { setLocalHost(device); setHostingState(restriction); setHostingChecked(true); setLocalHostError(""); }
            } catch (reason) {
              if (active) setLocalHostError(failureMessage(reason, "이 기기의 서버 정보를 확인하지 못했어요."));
            }
          }
        }
        if (centralEnabled && centralSessionLoggedOut()) {
          if (active) setChecking(false);
          return;
        }
        if (!centralEnabled) {
          const bootstrap = await requestDesktopBootstrapStatus();
          if (bootstrap.phase === "complete") {
            if (active) await enterApplication(bootstrap);
            return;
          }
          if (bootstrap.phase !== "empty") {
            throw new Error("로컬 신원 권위에 명시적 복구가 필요합니다.");
          }
        }
      } catch (reason) {
        if (active) {
          if (reason instanceof GoogleRegistrationRequired) { setGoogleRegistration(reason); setError(""); setChecking(false); return; }
          setError(failureMessage(reason, "앱을 시작하지 못했어요."));
          setChecking(false);
        }
        return;
      }
      if (!centralEnabled) {
        if (active) setChecking(false);
        return;
      }
      const pendingRecoveryCode = loadPendingCentralRecoveryCode();
      if (pendingRecoveryCode) {
        if (active) {
          setScreen("recovery-code");
          setChecking(false);
        }
        return;
      }

      const existing = loadCentralSession();
      if (!existing) {
        if (active) setChecking(false);
        return;
      }
      try {
        setStatus("로그인 정보와 방 목록을 확인하는 중");
        const central = await refreshCentral();
        if (!active) return;
        if (!central.person) {
          if (central.error) throw central.error;
          setChecking(false); return;
        }
        setScreen("servers"); setChecking(false);
      } catch (reason) {
        if (isCentralAuthenticationError(reason)) {
          if (active) {
            setError("로그인이 만료됐어요. 다시 로그인해 주세요.");
            setChecking(false);
          }
          return;
        }
        if (active) {
          setError(failureMessage(reason, "로그인 정보를 이 기기에 저장하지 못했어요. 다시 시도해 주세요."));
          setChecking(false);
        }
      }
    }
    void initialize();
    return () => {
      active = false;
    };
  }, [centralEnabled, deviceToken, onComplete, webEntry, refreshCentral]);

  async function googleLogin() {
    if (busy) return;
    const controller = new AbortController();
    googleAbortController.current = controller;
    setBusy(true);
    setError("");
    try {
      if (webEntry) {
        setStatus("Google 로그인 화면을 여는 중");
        await startCentralWebGoogle(controller.signal);
        return;
      }
      await loginCentralGoogle(setStatus, controller.signal);
      await finishCentralStartup();
    } catch (reason) {
      if (reason instanceof GoogleRegistrationRequired) { setGoogleRegistration(reason); setError(""); setChecking(false); return; }
      setChecking(false);
      setError(
        typeof reason === "object" &&
          reason !== null &&
          "name" in reason &&
          reason.name === "AbortError"
          ? "Google 로그인을 취소했어요."
          : failureMessage(reason, "Google 로그인을 완료하지 못했어요.")
      );
    } finally {
      if (googleAbortController.current === controller) {
        googleAbortController.current = null;
      }
      setBusy(false);
    }
  }

  async function continueLocalGuest() {
    const name = displayName.trim();
    if (!name || busy) return;
    setBusy(true);
    setError("");
    try {
      const localAuthority = await saveLocalProfile(
        name,
        bootstrapRequestId.current
      );
      await enterApplication(localAuthority);
    } catch (reason) {
      setError(failureMessage(reason, "프로필을 저장하지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  if (checking) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center bg-[#101114] p-5">
        <main className="grid place-items-center gap-3" aria-label="앱 시작 준비">
          <LoaderCircle size={28} className="animate-spin text-[#8d96ff]" />
          <p role="status" className="text-[12px] font-bold text-text-muted">
            {status}
          </p>
        </main>
      </div>
    );
  }

  if (!centralEnabled) {
    return (
      <div className="fixed inset-0 z-[400] grid place-items-center overflow-y-auto bg-[#101114] p-5">
        <main className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl">
          <div className="flex justify-end empty:hidden"><AppUpdateIndicator /></div>
          <header className="grid gap-2">
            <h1 className="text-2xl font-black text-text-primary">어떻게 사용할까요?</h1>
            <p className="text-[13px] font-semibold leading-5 text-text-muted">
              로그인 서버가 설정되지 않았어요. 이 기기에서 사용할 이름을 입력해 주세요.
            </p>
          </header>
          {error && (
            <div
              role="alert"
              className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
            >
              {error}
            </div>
          )}
          <label className="grid gap-2 text-[11px] font-black text-text-secondary">
            게스트 표시 이름
            <input
              autoFocus
              maxLength={80}
              value={displayName}
              className="min-h-10 rounded-md bg-[#2b2d31] px-3 text-[13px] text-text-primary outline-none"
              onChange={(event) => setDisplayName(event.currentTarget.value)}
            />
          </label>
          <button
            type="button"
            className="min-h-10 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
            disabled={!displayName.trim() || busy}
            onClick={() => void continueLocalGuest()}
          >
            게스트로 계속
          </button>
        </main>
      </div>
    );
  }

  if (googleRegistration) return (
    <div className="fixed inset-0 z-[400] grid place-items-center overflow-y-auto bg-[#101114] p-5">
      <main className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl" aria-label="시작 로그인">
        <GoogleRegistrationChoice request={googleRegistration}
          onCancel={() => { setGoogleRegistration(null); setError(""); setScreen("choice"); setAccountSettingsOpen(false); }}
          onComplete={async () => { await finishCentralStartup(); setGoogleRegistration(null); setError(""); }} />
      </main>
    </div>
  );

  return (
    <div className="fixed inset-0 z-[400] grid place-items-center overflow-y-auto bg-[#101114] p-5">
      {centralUnavailable && <ConnectionBanner message={directory?.status === "error" ? "로그인 정보를 확인하지 못했어요. 다시 시도해 주세요."
        : directory?.error ? "로그인 서버에 연결하지 못했고 저장된 목록을 읽지 못했어요. 서버 목록을 다시 확인해 주세요."
        : webEntry ? "로그인 서버에 연결하지 못했어요. 연결을 다시 확인하고 있어요." : "로그인 서버에 연결하지 못했어요. 이 기기의 서버는 계속 쓸 수 있어요."} />}
      <main
        className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
        aria-label="시작 로그인"
      >
        <div className="flex items-center justify-end gap-2 empty:hidden">
        <AppUpdateIndicator />
        {(screen === "servers" || screen === "choice") && !accountSettingsOpen && <button type="button" className="ops-button flex min-h-11 max-w-full items-center gap-2 justify-self-end" aria-label="계정 설정" disabled={busy} onClick={() => setAccountSettingsOpen(true)}>
          <span className="dc-member-avatar" style={{ width: 28, height: 28 }} aria-hidden="true">{centralPerson?.avatar_url ? <img src={centralPerson?.avatar_url} alt="" referrerPolicy="no-referrer" /> : centralPerson?.display_name.slice(0, 1) || "•"}</span>
          <span className="truncate">{centralPerson?.display_name || "계정"}</span>
        </button>}
        </div>
        {accountSettingsOpen ? <>
          <button type="button" className="ops-button min-h-11 justify-self-start" disabled={busy} onClick={() => setAccountSettingsOpen(false)}>← 돌아가기</button>
          <CentralAccountSettings disabled={busy} showEntryLink={false} />
        </> : <>
        <header className="grid gap-2">
          <h1 className="text-2xl font-black text-text-primary">
            {screen === "recovery-code"
              ? "복구 코드를 보관하세요"
              : screen === "servers"
                ? hostingState === "retired" ? "이 컴퓨터는 더 이상 이 계정의 서버가 아니에요" : ownerConflict ? "남길 서버를 선택해 주세요" : ownedServer ? `이 계정의 서버는 ‘${ownedServer.alias}’예요` : "서버 목록"
                : "먼저 로그인해 주세요"}
          </h1>
          <p className="text-[13px] font-semibold leading-5 text-text-muted empty:hidden">
            {screen === "recovery-code"
              ? "다른 기기에서 이 게스트 계정을 되찾을 때 필요해요."
              : screen === "servers"
                ? ""
              : "대화는 방을 여는 컴퓨터에만 저장돼요."}
          </p>
        </header>
        {!webEntry && hostingChecked && hostingState === "account_deleted" && <LocalAccountDataChoice disabled={busy} />}


        {screen === "choice" && (
          <div className="grid gap-3">
            <button
              type="button"
              className="flex min-h-12 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[14px] font-black text-white disabled:opacity-60"
              disabled={busy}
              onClick={() => void googleLogin()}
            >
              {busy ? (
                <LoaderCircle size={17} className="animate-spin" />
              ) : (
                <LogIn size={17} />
              )}{" "}
              Google로 계속
            </button>
            {busy && (
              <button
                type="button"
                className="min-h-10 rounded-md border border-white/10 px-4 text-[12px] font-black text-text-primary"
                onClick={() => googleAbortController.current?.abort()}
              >
                Google 로그인 취소
              </button>
            )}

          </div>
        )}

        {screen === "servers" && (
          <section className="grid gap-2">
            <div className="flex items-center justify-between gap-3">
              <p className="text-[12px] font-semibold text-text-muted">{centralPerson?.display_name}님의 서버</p>
              <button type="button" className="grid h-11 w-11 place-items-center rounded-lg text-text-muted hover:bg-white/5 hover:text-text-primary disabled:opacity-50" aria-label="서버 목록 새로고침" title="새로고침" disabled={busy} onClick={() => void refreshServers()}><RefreshCw size={16} /></button>
            </div>
            {!ownerConflict && centralServers.length === 0 && <p className="text-[12px] text-text-muted">{centralPerson?.identity_kind === "guest" || accountDeviceOnly ? "초대 링크를 받으면 여기에서 바로 들어갈 수 있어요." : "아직 내 서버가 없어요. AgentsAssemble 앱을 설치한 컴퓨터에서 같은 계정으로 로그인하면 서버가 생겨요."}</p>}
            {localHostError && <div role="alert" className="text-sm text-red-300">{localHostError}<p>서버 목록 새로고침으로 다시 확인해 주세요.</p></div>}
            {!webEntry && hostingState === "account_deleted" && localHost?.server_id && <FreshHostRegistrationButton serverId={localHost.server_id} deviceToken={deviceToken} disabled={busy} onRegistered={refreshServers} />}
            <ServerRestoreCard onOpen={() => enterApplication()} />
            <CentralServerList deviceToken={deviceToken} deviceConnect={Boolean(ownedServer)} conflict={ownerConflict} key={centralPerson?.person_id} servers={centralServers} liveServers={directory?.live?.servers || []} centralUnavailable={centralUnavailable} connectingServerId={connectingServerId} busy={busy} profileName={centralPerson?.display_name} localHost={!ownedServer && !hostingState && !accountDeviceOnly ? localHost : null} onOpenLocal={!webEntry && !ownedServer && ownedServers.length === 0 && !hostingState && !accountDeviceOnly ? (name) => selectCentralServer(undefined, name) : undefined} onOpen={selectCentralServer} onRefresh={refreshServers} />
            <button type="button" className="mt-2 min-h-11 w-fit text-[13px] text-text-muted hover:text-text-primary hover:underline disabled:opacity-50" disabled={busy} onClick={() => void logout()}>로그아웃</button>
          </section>
        )}

        {screen !== "servers" && <CentralGuestEntry screen={screen} setScreen={setScreen}
          disabled={busy} onBusyChange={setBusy} onComplete={finishCentralStartup} />}


        {busy && screen === "choice" && (
          <p role="status" className="text-center text-[11px] font-bold text-text-muted">
            {status}
          </p>
        )}
        </>}
        {error && (
          <div
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            {error}
            {screen === "choice" && loadCentralSession() && <button type="button" className="ops-button mt-2 block" disabled={busy} onClick={() => void refreshServers()}>다시 확인</button>}
          </div>
        )}
      </main>
    </div>
  );
}
