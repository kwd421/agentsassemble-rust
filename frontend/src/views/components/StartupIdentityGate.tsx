import AppUpdateIndicator from "./AppUpdateIndicator";
import { userFacingError } from "../../lib/apiErrors";
import { loadDeletionReceipt } from "../../lib/central/accountDeletion";
import CentralGuestEntry, { type CentralGuestScreen } from "./CentralGuestEntry";
import ServerRestoreCard from "./ServerRestoreCard";
import LocalAccountDataChoice from "./LocalAccountDataChoice";
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import GoogleRegistrationChoice from "./GoogleRegistrationChoice";
import { useCentralDirectory } from "../../app/useCentralDirectory";
import { loadMemberAnchors } from "../../lib/central/memberAnchors";
import { isCentralTemporaryError } from "../../lib/central/connectionError";
import { centralDailyQuotaNotice } from "../../lib/central/centralRequestGovernor";
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
  RefreshCw,
} from "lucide-react";
import GoogleGLogo from "./GoogleGLogo";

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

// One retry of an unconfirmed central answer before this computer's row stops checking.
const CENTRAL_RETRY_DELAY_MS = 3000;

// Keep native strings and browser errors available without exposing them by default.
// One plain sentence pair, never raw diagnostics: the cause line is the
// error's own product sentence when it has one (internal checks fall back).
function failureMessage(reason: unknown, message: string, onRetry?: () => void): ReactNode {
  const cause = userFacingError(reason, message);
  return <>
    <p className="font-semibold">{message}</p>
    <p>{cause !== message ? cause : "잠시 후 다시 시도해 주세요."}</p>
    {onRetry && <button type="button" className="dc-startup-error-retry" onClick={onRetry}>다시 시도</button>}
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
  // An unconfirmed central answer is not yet an offline verdict: retry once, then trust the result.
  const [centralUnreachable, setCentralUnreachable] = useState(false);
  useEffect(() => {
    setCentralUnreachable(false);
    if (directory?.status !== "central-unconfirmed") return;
    const timer = window.setTimeout(() => {
      refreshCentral().then(
        (next) => setCentralUnreachable(next.status === "central-unconfirmed"),
        (reason: unknown) => { if ((reason as { name?: unknown } | null)?.name !== "AbortError") setCentralUnreachable(true); },
      );
    }, CENTRAL_RETRY_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [directory?.status, refreshCentral]);
  const localChecking = directory?.status === "central-unconfirmed" && !centralUnreachable;
  const centralPerson = directory?.person;
  const accountDeviceOnly = Boolean(centralPerson && deviceAccount === centralPerson.person_id);
  const centralServers = directory?.servers || [];
  const ownerConflict = directory?.live?.owner_server_conflict;
  const ownedServers = centralServers.filter(server => server.relation === "owner");
  const ownedServer = ownedServers.length === 1 ? ownedServers[0] : undefined;
  // The owned row renders as this computer only when every local-open guard agrees.
  const ownedIsLocal = !webEntry && !ownerConflict && !hostingState && !accountDeviceOnly
    && Boolean(localHost?.server_id) && ownedServer?.server_id === localHost?.server_id;
  // When the account's owned server is shown as a remote device instead, keep every
  // guard and button as they are and state the local cause in one plain line.
  const ownedRemoteReason = !webEntry && ownedServer && !ownerConflict && !ownedIsLocal && !localHostError
    && hostingState !== "retired" && hostingState !== "account_deleted"
    ? (!localHost?.server_id
      ? "이 컴퓨터에서 서버 데이터를 찾지 못했어요."
      : localHost.server_id !== ownedServer.server_id
        ? "이 컴퓨터의 서버가 계정에 등록된 서버와 달라요."
        : "이 컴퓨터가 다른 기기로 등록돼 있어요.")
    : "";
  // Only someone who deleted their account on this device needs the signed-out
  // way back to the deletion status (and restore window).
  // A receipt that cannot be read still means a deletion happened here: show
  // the link and let the status panel report the problem, never crash login.
  const retainedDeletionReceipt = (() => {
    try { return Boolean(loadDeletionReceipt()); } catch { return true; }
  })();
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  // Starting failed: nothing on this screen can work until the app starts.
  const [startupFailed, setStartupFailed] = useState(false);
  const [status, setStatus] = useState("앱을 준비하는 중이에요");
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
    setStatus("앱을 준비하는 중이에요");
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

  // Startup goes straight into the app: once a signed-in account has an
  // openable shell (local owned match, joined member host/anchor, or an
  // online owned remote), enter immediately instead of waiting for the local
  // host or a manual 열기. A sole offline owned server with no member stays
  // on the chooser notice; a mismatch never gates (the app shows it quietly).
  const straightIn = useRef(false);
  useEffect(() => {
    if (screen !== "servers" || webEntry || straightIn.current || autoOpened.current) return;
    if (!centralPerson || ownerConflict || hostingState === "retired" || hostingState === "account_deleted") return;
    const hasMember = centralServers.some((server) => server.relation === "member");
    let hasAnchor = false;
    try {
      hasAnchor = Boolean(centralPerson.person_id && loadMemberAnchors(centralPerson.person_id).length > 0);
    } catch {
      hasAnchor = false;
    }
    if (hasMember || hasAnchor) {
      straightIn.current = true;
      onComplete();
      return;
    }
    if (ownedServers.length === 0) return;
    const ownedIsLocalNow = Boolean(localHost?.server_id) && ownedServer?.server_id === localHost?.server_id;
    if (ownedIsLocalNow) {
      // The local auto-open below owns registration; a member-less owned
      // local still enters through it, not here.
      return;
    }
    // A single online owned remote opens by itself (no manual 열기); an
    // offline owned remote with no member stays for the chooser notice.
    const liveOwned = (directory?.live?.servers || []).find((server) => server.server_id === ownedServer?.server_id);
    const ownedOnline = Boolean(liveOwned?.endpoint?.status === "published" && liveOwned.endpoint.mode === "event_secure_v1");
    if (ownedOnline && ownedServer && !busy) {
      straightIn.current = true;
      autoOpened.current = true;
      void selectCentralServer(ownedServer);
    }
  }, [screen, webEntry, centralPerson, ownerConflict, hostingState, centralServers, ownedServers.length, ownedServer, localHost?.server_id, directory?.live?.servers, busy, onComplete]);

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
            // The app shell opens without waiting for the local host: device
            // info resolves in the background while central login proceeds.
            void (async () => {
              try {
                await retryPendingLocalDemotion(deviceToken);
                const device = await requestDesktopHostDeviceInfo();
                const restriction = device.hosting_state;
                if (active) { setLocalHost(device); setHostingState(restriction); setHostingChecked(true); setLocalHostError(""); }
              } catch (reason) {
                if (active) {
                  setHostingChecked(true);
                  setLocalHostError(failureMessage(reason, "이 기기의 서버 정보를 확인하지 못했어요."));
                }
              }
            })();
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
          setStartupFailed(true);
          setError(failureMessage(reason, "앱을 시작하지 못했어요.", () => window.location.reload()));
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
        setStatus("로그인 정보를 확인하는 중이에요");
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
        setStatus("Google 로그인 창을 여는 중이에요");
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
        <main className="grid w-full max-w-[480px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-8 shadow-2xl">
          <div className="flex justify-end empty:hidden"><AppUpdateIndicator /></div>
          <header className="grid gap-2">
            <h1 className="text-2xl font-bold text-text-primary">어떻게 사용할까요?</h1>
            <p className="text-sm leading-5 text-text-muted">
              로그인 서버가 설정되지 않았어요. 이 컴퓨터에서 쓸 이름을 입력해 주세요.
            </p>
          </header>
          {error && (
            <div
              role="alert"
              className="dc-startup-error"
            >
              {error}
            </div>
          )}
          <label className="dc-startup-field">
            표시 이름
            <input
              autoFocus
              maxLength={80}
              value={displayName}
              placeholder="다른 참가자에게 보일 이름"
              className="dc-startup-input"
              onChange={(event) => setDisplayName(event.currentTarget.value)}
            />
          </label>
          <button
            type="button"
            className="dc-startup-primary disabled:opacity-50"
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
      <main className="grid w-full max-w-[480px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-8 shadow-2xl" aria-label="시작 로그인">
        <GoogleRegistrationChoice request={googleRegistration}
          onCancel={() => { setGoogleRegistration(null); setError(""); setScreen("choice"); setAccountSettingsOpen(false); }}
          onComplete={async () => { await finishCentralStartup(); setGoogleRegistration(null); setError(""); }} />
      </main>
    </div>
  );

  return (
    <div className="fixed inset-0 z-[400] grid place-items-center content-center overflow-y-auto bg-[#101114] p-5">
      {centralUnavailable && <ConnectionBanner message={centralDailyQuotaNotice(directory?.person?.person_id ?? "") ?? (directory?.status === "error" ? "로그인 정보를 확인하지 못했어요. 다시 시도해 주세요."
        : directory?.error ? "로그인 서버에 연결하지 못했고 저장된 목록을 읽지 못했어요. 서버 목록을 다시 확인해 주세요."
        : webEntry ? "로그인 서버에 연결하지 못했어요. 연결을 다시 확인하고 있어요." : "로그인 서버에 연결하지 못했어요. 이 기기의 서버는 계속 쓸 수 있어요.")} />}
      <main
        className="grid w-full max-w-[480px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-8 shadow-2xl"
        aria-label="시작 로그인"
      >
        <div className="flex items-center justify-end gap-2 empty:hidden">
        <AppUpdateIndicator />
        {centralPerson && (screen === "servers" || screen === "choice") && !accountSettingsOpen && <button type="button" className="ops-button flex min-h-11 max-w-full items-center gap-2 justify-self-end" aria-label="계정 설정" disabled={busy} onClick={() => setAccountSettingsOpen(true)}>
          <span className="dc-member-avatar" style={{ width: 28, height: 28 }} aria-hidden="true">{centralPerson.avatar_url ? <img src={centralPerson.avatar_url} alt="" referrerPolicy="no-referrer" /> : centralPerson.display_name.slice(0, 1) || "•"}</span>
          <span className="truncate">{centralPerson.display_name}</span>
        </button>}
        </div>
        {accountSettingsOpen ? <>
          <button type="button" className="ops-button min-h-11 justify-self-start" disabled={busy} onClick={() => setAccountSettingsOpen(false)}>← 돌아가기</button>
          <CentralAccountSettings disabled={busy} showEntryLink={false} />
        </> : <>
        <header className={screen === "choice" && !centralPerson ? "grid gap-2 text-center" : "grid gap-2"}>
          <h1 className="text-2xl font-bold leading-[28px] text-text-primary">
            {screen === "recovery-code"
              ? "복구 코드를 보관하세요"
              : screen === "servers"
                ? hostingState === "retired" ? "이 컴퓨터는 더 이상 이 계정의 서버가 아니에요" : ownerConflict ? "남길 서버를 선택해 주세요" : ownedServer ? `이 계정의 서버는 ‘${ownedServer.alias}’예요` : "서버 목록"
                : "AgentsAssemble에 오신 것을 환영합니다"}
          </h1>
          <p className="text-[13px] font-semibold leading-5 text-text-muted empty:hidden">
            {screen === "recovery-code"
              ? "다른 기기에서 이 게스트 계정을 되찾을 때 필요해요."
              : ""}
          </p>
        </header>
        {error && (
          <div
            role="alert"
            className="dc-startup-error"
          >
            {error}
            {screen === "choice" && loadCentralSession() && <button type="button" className="ops-button mt-2 block" disabled={busy} onClick={() => void refreshServers()}>다시 확인</button>}
          </div>
        )}
        {!webEntry && hostingChecked && hostingState === "account_deleted" && <LocalAccountDataChoice disabled={busy} />}


        {screen === "choice" && (
          <div className="grid gap-3">
            <button
              type="button"
              className="dc-google-signin min-h-12 w-full"
              disabled={busy || startupFailed}
              onClick={() => void googleLogin()}
            >
              {busy ? (
                <LoaderCircle size={17} className="animate-spin" />
              ) : (
                <GoogleGLogo />
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
            <ServerRestoreCard onOpen={() => enterApplication()} liveHost={Boolean(localHost?.server_id) && !hostingState} />
            {ownedRemoteReason && <p className="text-[13px] text-text-muted">{ownedRemoteReason}</p>}
            <CentralServerList deviceToken={deviceToken} deviceConnect={Boolean(ownedServer)} conflict={ownerConflict} key={centralPerson?.person_id} servers={centralServers} liveServers={directory?.live?.servers || []} centralUnavailable={centralUnavailable} localChecking={localChecking} connectingServerId={connectingServerId} busy={busy} profileName={centralPerson?.display_name} localHost={!webEntry && !hostingState && !accountDeviceOnly ? localHost : null} onOpenLocal={!webEntry && !ownerConflict && (ownedServers.length === 0 || ownedServer?.server_id === localHost?.server_id) && !hostingState && !accountDeviceOnly ? (name) => selectCentralServer(undefined, name) : undefined} onOpen={selectCentralServer} onRefresh={refreshServers} />
            <button type="button" className="mt-2 min-h-11 w-fit text-[13px] text-text-muted hover:text-text-primary hover:underline disabled:opacity-50" disabled={busy} onClick={() => void logout()}>로그아웃</button>
          </section>
        )}

        {screen !== "servers" && <CentralGuestEntry screen={screen} setScreen={setScreen}
          disabled={busy || startupFailed} onBusyChange={setBusy} onComplete={finishCentralStartup} />}


        {busy && screen === "choice" && (
          <p role="status" className="text-center text-[11px] font-bold text-text-muted">
            {status}
          </p>
        )}
        </>}
      </main>
      {!centralPerson && screen === "choice" && !accountSettingsOpen && retainedDeletionReceipt && <button type="button" className="mt-4 min-h-11 w-fit justify-self-center text-[13px] text-text-muted hover:text-text-primary hover:underline disabled:opacity-50" disabled={busy} onClick={() => setAccountSettingsOpen(true)}>탈퇴 진행 상황</button>}
    </div>
  );
}
