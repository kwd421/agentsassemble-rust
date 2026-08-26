import { useState, type ReactNode } from "react";

import {
  getOrCreateBrowserCredential,
  hasStartupIdentitySelection,
  loadRememberedGuestProfile,
} from "../../lib/deviceIdentity";
import { isDesktopWebview } from "../../lib/desktopBridge";
import StartupIdentityGate from "./StartupIdentityGate";

const GUEST_SESSION_STORAGE_KEY = "agentsassemble.roomGuestSession.v1";

function hasStoredGuestSession(): boolean {
  try {
    return Boolean(window.localStorage.getItem(GUEST_SESSION_STORAGE_KEY));
  } catch {
    return false;
  }
}

function startupIdentityBypassRequested(): boolean {
  try {
    const url = new URL(window.location.href);
    const query = url.searchParams;
    const fragment = new URLSearchParams(url.hash.replace(/^#/, ""));
    const pathname = url.pathname.replace(/\/+$/, "") || "/";
    return Boolean(
      query.get("guest") === "1" ||
        query.has("invite") ||
        query.get("recover") === "1" ||
        query.has("pair") ||
        pathname === "/join" ||
        pathname === "/pair" ||
        fragment.has("invite") ||
        fragment.has("recovery") ||
        fragment.has("pairing") ||
        fragment.has("operatorPairing")
    );
  } catch {
    return false;
  }
}

function startupIdentityRunsOnThisOrigin(): boolean {
  try {
    const url = new URL(window.location.href);
    const hostname = url.hostname.toLowerCase();
    const loopbackHosts = new Set([
      "localhost",
      "127.0.0.1",
      "::1",
      "[::1]",
    ]);
    const nativeShell =
      url.protocol === "tauri:" ||
      url.protocol === "asset:" ||
      hostname === "tauri.localhost";
    const configuredCentral = String(
      import.meta.env.VITE_AGENTSASSEMBLE_CENTRAL_URL || ""
    )
      .trim()
      .replace(/\/+$/, "");
    let centralOrigin = "";
    if (configuredCentral) {
      try {
        centralOrigin = new URL(configuredCentral).origin;
      } catch {
        centralOrigin = "";
      }
    }
    return nativeShell || loopbackHosts.has(hostname) || url.origin === centralOrigin;
  } catch {
    return false;
  }
}

export default function StartupIdentityBoundary({ children }: { children: ReactNode }) {
  const [ready, setReady] = useState(
    () => {
      if (isDesktopWebview()) return false;
      if (startupIdentityBypassRequested()) return true;
      return (
        !startupIdentityRunsOnThisOrigin() ||
        hasStartupIdentitySelection() ||
        Boolean(loadRememberedGuestProfile()) ||
        hasStoredGuestSession()
      );
    }
  );
  const [browserCredential] = useState(() => {
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
