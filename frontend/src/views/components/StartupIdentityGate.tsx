import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  Check,
  Copy,
  KeyRound,
  LoaderCircle,
  LogIn,
  UserRound,
} from "lucide-react";

import { fetchAccountStatus } from "../../api/identity";
import { saveUserProfile } from "../../api/room";
import {
  bootstrapCentral,
  centralIdentityConfigured,
  clearPendingCentralRecoveryCode,
  createCentralGuest,
  isCentralAuthenticationError,
  loadCentralSession,
  loadPendingCentralRecoveryCode,
  loginCentralGoogle,
  recoverCentralGuest,
  registerLocalServer,
} from "../../lib/centralIdentity";
import {
  fetchDesktopOperatorRuntime,
  initializeDesktopBootstrap,
  isDesktopWebview,
  requestDesktopBootstrapStatus,
  type DesktopBootstrapGrant,
} from "../../lib/desktopBridge";
import {
  rememberGuestProfile,
  rememberStartupIdentitySelection,
} from "../../lib/deviceIdentity";
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
import { DEFAULT_USER_PROFILE } from "../../lib/userProfileModel";
import GoogleAccountSettings from "./GoogleAccountSettings";

type Screen = "choice" | "guest" | "recover" | "recovery-code";

async function saveLocalProfile(
  displayName: string,
  deviceToken: string,
  bootstrapRequestId: string
) {
  const name = displayName.trim();
  if (!name) return;
  if (isDesktopWebview()) {
    const current = await requestDesktopBootstrapStatus();
    const bootstrap =
      current.phase === "empty"
        ? await initializeDesktopBootstrap(bootstrapRequestId, name)
        : current;
    if (bootstrap.phase !== "complete" || !bootstrap.profile) {
      throw new Error("로컬 신원 권위를 안전하게 초기화하지 못했습니다.");
    }
    rememberGuestProfile({
      displayName: bootstrap.profile.display_name,
      avatarImage: bootstrap.profile.avatar_image_url,
    });
    return bootstrap;
  }
  const saved = await saveUserProfile(
    {
      ...DEFAULT_USER_PROFILE,
      displayName: name,
      avatarLabel: name.slice(0, 2).toUpperCase(),
    },
    { deviceToken }
  );
  rememberGuestProfile({
    displayName: saved.displayName,
    avatarImage: saved.avatarImage,
  });
  return undefined;
}

export default function StartupIdentityGate({
  deviceToken,
  onComplete,
}: {
  deviceToken: string;
  onComplete: () => void;
}) {
  const centralEnabled = centralIdentityConfigured();
  const [screen, setScreen] = useState<Screen>("choice");
  const [displayName, setDisplayName] = useState("");
  const [recoveryInput, setRecoveryInput] = useState("");
  const [issuedRecoveryCode, setIssuedRecoveryCode] = useState("");
  const [savedRecoveryCode, setSavedRecoveryCode] = useState(false);
  const [copied, setCopied] = useState(false);
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("저장된 사용자 확인 중");
  const [error, setError] = useState("");
  const googleAbortController = useRef<AbortController | null>(null);
  const bootstrapRequestId = useRef(globalThis.crypto.randomUUID());

  useEffect(
    () => () => {
      googleAbortController.current?.abort();
    },
    []
  );

  async function enterApplication(expectedDesktopAuthority?: DesktopBootstrapGrant) {
    setChecking(true);
    setStatus("로컬 엔진과 방 목록을 준비하는 중");
    const desktop = isDesktopWebview();
    const desktopAuthority = desktop
      ? expectedDesktopAuthority || (await requestDesktopBootstrapStatus())
      : undefined;
    if (desktop && desktopAuthority?.phase !== "complete") {
      throw new Error("완료된 데스크톱 권위가 방 목록을 소유하지 않습니다.");
    }
    const response = desktop
      ? await fetchDesktopOperatorRuntime("/api/rooms", { cache: "no-store" })
      : await fetch("/api/rooms", { cache: "no-store" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const payload = parseStrictRoomDirectory(await response.json());
    if (
      desktopAuthority &&
      (payload.server_id !== desktopAuthority.server_id ||
        payload.authority_lineage_id !== desktopAuthority.authority_lineage_id)
    ) {
      throw new Error("방 목록 권위가 네이티브 bootstrap 계보와 일치하지 않습니다.");
    }
    const current = loadRoomDockItems().map(hydratePersistedRoom);
    const synchronized = mergeServerRoomsIntoDock(
      current,
      payload.rooms,
      window.location.origin,
      payload.server_id
    );
    persistRoomDockItems(synchronized.map(persistableRoom));
    bindRoomDirectoryAuthority(payload);
    rememberStartupIdentitySelection();
    onComplete();
  }

  async function continueAfterRecoveryCode() {
    if (!savedRecoveryCode || busy) return;
    setBusy(true);
    clearPendingCentralRecoveryCode();
    await registerLocalServer(deviceToken);
    await enterApplication();
  }

  useEffect(() => {
    let active = true;
    async function initialize() {
      if (isDesktopWebview()) {
        try {
          const bootstrap = await requestDesktopBootstrapStatus();
          if (bootstrap.phase === "complete") {
            if (active) await enterApplication(bootstrap);
            return;
          }
          if (bootstrap.phase !== "empty") {
            throw new Error("로컬 신원 권위에 명시적 복구가 필요합니다.");
          }
        } catch (reason) {
          if (active) {
            setError(
              reason instanceof Error
                ? reason.message
                : "로컬 신원 권위를 확인하지 못했습니다."
            );
            setChecking(false);
          }
          return;
        }
        if (!centralEnabled) {
          if (active) setChecking(false);
          return;
        }
      }
      if (centralEnabled) {
        const pendingRecoveryCode = loadPendingCentralRecoveryCode();
        if (pendingRecoveryCode) {
          if (active) {
            setIssuedRecoveryCode(pendingRecoveryCode);
            setSavedRecoveryCode(false);
            setCopied(false);
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
          setStatus("중앙 신원과 서버 목록을 확인하는 중");
          await bootstrapCentral();
          const localAuthority = await saveLocalProfile(
            existing.person.display_name,
            deviceToken,
            bootstrapRequestId.current
          );
          await registerLocalServer(deviceToken);
          if (active) await enterApplication(localAuthority);
        } catch (reason) {
          if (isCentralAuthenticationError(reason)) {
            if (active) {
              setError("중앙 로그인이 만료됐습니다. 다시 로그인해 주세요.");
              setChecking(false);
            }
            return;
          }
          if (active) {
            setError(
              reason instanceof Error
                ? reason.message
                : "중앙 신원과 로컬 권위를 동기화하지 못했습니다."
            );
            setChecking(false);
          }
        }
        return;
      }
      try {
        const account = await fetchAccountStatus({ deviceToken });
        if (!active) return;
        if (account.account) {
          await enterApplication();
        } else {
          setChecking(false);
        }
      } catch (reason) {
        if (active) {
          setError(
            reason instanceof Error
              ? reason.message
              : "저장된 사용자와 방 목록을 확인하지 못했습니다."
          );
          setChecking(false);
        }
      }
    }
    void initialize();
    return () => {
      active = false;
    };
  }, [centralEnabled, deviceToken, onComplete]);

  async function createGuest() {
    const name = displayName.trim();
    if (!name || busy) return;
    setBusy(true);
    setError("");
    setStatus("복구 가능한 게스트 신원을 만드는 중");
    try {
      const result = await createCentralGuest(name);
      await saveLocalProfile(
        result.person.display_name || name,
        deviceToken,
        bootstrapRequestId.current
      );
      await registerLocalServer(deviceToken);
      setIssuedRecoveryCode(result.recovery_code);
      setSavedRecoveryCode(false);
      setCopied(false);
      setScreen("recovery-code");
    } catch (reason) {
      const pending = loadPendingCentralRecoveryCode();
      if (pending) {
        setIssuedRecoveryCode(pending);
        setSavedRecoveryCode(false);
        setCopied(false);
        setScreen("recovery-code");
      } else {
        setError(
          reason instanceof Error
            ? reason.message
            : "게스트 신원을 만들지 못했습니다."
        );
      }
    } finally {
      setBusy(false);
    }
  }

  async function recoverGuest() {
    if (!recoveryInput.trim() || busy) return;
    setBusy(true);
    setError("");
    setStatus("게스트 신원을 복구하고 이전 코드를 폐기하는 중");
    try {
      const result = await recoverCentralGuest(recoveryInput);
      await saveLocalProfile(
        result.person.display_name || "Guest",
        deviceToken,
        bootstrapRequestId.current
      );
      await registerLocalServer(deviceToken);
      setIssuedRecoveryCode(result.recovery_code);
      setSavedRecoveryCode(false);
      setCopied(false);
      setScreen("recovery-code");
    } catch (reason) {
      const pending = loadPendingCentralRecoveryCode();
      if (pending) {
        setIssuedRecoveryCode(pending);
        setSavedRecoveryCode(false);
        setCopied(false);
        setScreen("recovery-code");
      } else {
        setError(
          reason instanceof Error
            ? reason.message
            : "게스트 신원을 복구하지 못했습니다."
        );
      }
    } finally {
      setBusy(false);
    }
  }

  async function googleLogin() {
    if (busy) return;
    const controller = new AbortController();
    googleAbortController.current = controller;
    setBusy(true);
    setError("");
    try {
      const session = await loginCentralGoogle(setStatus, controller.signal);
      const localAuthority = await saveLocalProfile(
        session.person.display_name,
        deviceToken,
        bootstrapRequestId.current
      );
      await registerLocalServer(deviceToken);
      await enterApplication(localAuthority);
    } catch (reason) {
      setChecking(false);
      setError(
        typeof reason === "object" &&
          reason !== null &&
          "name" in reason &&
          reason.name === "AbortError"
          ? "Google 로그인을 취소했습니다."
          : reason instanceof Error
          ? reason.message
          : "Google 로그인을 완료하지 못했습니다."
      );
    } finally {
      if (googleAbortController.current === controller) {
        googleAbortController.current = null;
      }
      setBusy(false);
    }
  }

  async function copyRecoveryCode() {
    try {
      await navigator.clipboard.writeText(issuedRecoveryCode);
      setCopied(true);
    } catch {
      setError("복사 권한이 거부됐습니다. 코드를 직접 선택해 복사해 주세요.");
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
        deviceToken,
        bootstrapRequestId.current
      );
      await enterApplication(localAuthority);
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "로컬 프로필을 저장하지 못했습니다.");
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
          <header className="grid gap-2">
            <span className="text-[11px] font-black uppercase tracking-[0.16em] text-[#8d96ff]">
              AgentsAssemble
            </span>
            <h1 className="text-2xl font-black text-text-primary">어떻게 사용할까요?</h1>
            <p className="text-[13px] font-semibold leading-5 text-text-muted">
              중앙 디렉터리가 설정되지 않아 기존 로컬 신원 모드로 시작합니다.
            </p>
          </header>
          {error && (
            <p
              role="alert"
              className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
            >
              {error}
            </p>
          )}
          {!isDesktopWebview() && (
            <>
              <GoogleAccountSettings
                identity={{ deviceToken }}
                onAccountConnected={() => void enterApplication()}
              />
              <div className="flex items-center gap-3 text-[10px] font-black uppercase tracking-wider text-text-muted">
                <span className="h-px flex-1 bg-white/10" /> 또는{" "}
                <span className="h-px flex-1 bg-white/10" />
              </div>
            </>
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

  return (
    <div className="fixed inset-0 z-[400] grid place-items-center overflow-y-auto bg-[#101114] p-5">
      <main
        className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6 shadow-2xl"
        aria-label="시작 로그인"
      >
        <header className="grid gap-2">
          <span className="text-[11px] font-black uppercase tracking-[0.16em] text-[#8d96ff]">
            AgentsAssemble
          </span>
          <h1 className="text-2xl font-black text-text-primary">
            {screen === "recovery-code" ? "복구 코드를 보관하세요" : "먼저 로그인해 주세요"}
          </h1>
          <p className="text-[13px] font-semibold leading-5 text-text-muted">
            {screen === "recovery-code"
              ? "이 코드는 다른 기기에서 같은 게스트 신원과 서버 목록을 복구할 때 필요합니다. 중앙에는 코드 원문을 저장하지 않습니다."
              : "Google 계정은 내 서버 목록을 기기 간 동기화할 때만 사용합니다. 방과 메시지는 각 서버에 그대로 남습니다."}
          </p>
        </header>

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
            <button
              type="button"
              className="flex min-h-12 items-center justify-center gap-2 rounded-md bg-[#2b2d31] px-4 text-[14px] font-black text-text-primary"
              onClick={() => {
                setError("");
                setScreen("guest");
              }}
            >
              <UserRound size={17} /> 새 게스트로 계속
            </button>
            <button
              type="button"
              className="min-h-10 text-[12px] font-black text-[#aeb4ff]"
              onClick={() => {
                setError("");
                setScreen("recover");
              }}
            >
              이미 복구 코드가 있습니다
            </button>
          </div>
        )}

        {screen === "guest" && (
          <section className="grid gap-3 rounded-lg bg-[#1b1c20] p-4">
            <button
              type="button"
              className="flex w-fit items-center gap-1 text-[11px] font-black text-text-muted"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={14} /> 뒤로
            </button>
            <label className="grid gap-1.5 text-[11px] font-black text-text-secondary">
              표시 이름
              <input
                autoFocus
                type="text"
                maxLength={80}
                value={displayName}
                placeholder="다른 참가자에게 보일 이름"
                className="min-h-10 rounded-md border border-transparent bg-[#2b2d31] px-3 text-[13px] font-semibold text-text-primary outline-none focus:border-[#5865f2]"
                onChange={(event) => setDisplayName(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void createGuest();
                }}
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
              disabled={!displayName.trim() || busy}
              onClick={() => void createGuest()}
            >
              {busy ? (
                <LoaderCircle size={16} className="animate-spin" />
              ) : (
                <ArrowRight size={16} />
              )}{" "}
              {busy ? "만드는 중…" : "게스트 만들기"}
            </button>
          </section>
        )}

        {screen === "recover" && (
          <section className="grid gap-3 rounded-lg bg-[#1b1c20] p-4">
            <button
              type="button"
              className="flex w-fit items-center gap-1 text-[11px] font-black text-text-muted"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={14} /> 뒤로
            </button>
            <label className="grid gap-1.5 text-[11px] font-black text-text-secondary">
              게스트 복구 코드
              <input
                autoFocus
                autoComplete="one-time-code"
                spellCheck={false}
                value={recoveryInput}
                placeholder="XXXX-XXXX-…"
                className="min-h-10 rounded-md bg-[#2b2d31] px-3 font-mono text-[13px] text-text-primary outline-none"
                onChange={(event) =>
                  setRecoveryInput(event.currentTarget.value.toUpperCase())
                }
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
              disabled={!recoveryInput.trim() || busy}
              onClick={() => void recoverGuest()}
            >
              {busy ? (
                <LoaderCircle size={16} className="animate-spin" />
              ) : (
                <KeyRound size={16} />
              )}{" "}
              {busy ? "복구 중…" : "같은 게스트로 로그인"}
            </button>
          </section>
        )}

        {screen === "recovery-code" && (
          <section className="grid gap-4 rounded-lg bg-[#1b1c20] p-4">
            <label className="grid gap-2 text-[11px] font-black text-text-secondary">
              새 복구 코드
              <input
                readOnly
                value={issuedRecoveryCode}
                onFocus={(event) => event.currentTarget.select()}
                className="min-h-12 rounded-md bg-[#2b2d31] px-3 font-mono text-[13px] font-black tracking-wide text-text-primary outline-none"
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#3a3c42] px-4 text-[12px] font-black text-text-primary"
              onClick={() => void copyRecoveryCode()}
            >
              {copied ? <Check size={16} /> : <Copy size={16} />} {copied ? "복사됨" : "복구 코드 복사"}
            </button>
            <p className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]">
              이 코드를 잃으면 다른 기기에서 이 게스트 신원을 복구할 수 없습니다. 비밀번호 관리자나 안전한 오프라인 장소에 보관하세요. 복구 직후에는 이전 코드가 폐기됩니다.
            </p>
            <label className="flex items-start gap-2 text-[12px] font-bold leading-5 text-text-secondary">
              <input
                type="checkbox"
                className="mt-1"
                checked={savedRecoveryCode}
                onChange={(event) => setSavedRecoveryCode(event.currentTarget.checked)}
              />
              복구 코드를 안전한 곳에 저장했습니다.
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
              disabled={!savedRecoveryCode || busy}
              onClick={() => void continueAfterRecoveryCode()}
            >
              {busy ? <LoaderCircle size={16} className="animate-spin" /> : <ArrowRight size={16} />} 계속
            </button>
          </section>
        )}

        {busy && screen === "choice" && (
          <p role="status" className="text-center text-[11px] font-bold text-text-muted">
            {status}
          </p>
        )}
        {error && (
          <p
            role="alert"
            className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]"
          >
            {error}
          </p>
        )}
      </main>
    </div>
  );
}
