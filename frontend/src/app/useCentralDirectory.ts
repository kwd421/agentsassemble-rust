import { useCallback, useEffect, useRef, useState } from "react";
import { CENTRAL_SESSION_CLEARED_EVENT, bootstrapCentral, centralIdentityConfigured, loadCentralSession, isCentralAuthenticationError,
  type CentralBootstrap, type CentralPerson, type CentralSession } from "../lib/central/identity";
import { loadCentralDirectoryCache, type CentralServerDisplay } from "../lib/central/directoryCache";
import { isCentralTemporaryError } from "../lib/central/connectionError";

// Focus/visible checks are throttled; online and explicit actions refresh immediately.
const FOCUS_CHECK_MS = 5 * 60_000;

export type CentralDirectoryState = {
  status: "connected" | "central-unconfirmed" | "authentication-required" | "error";
  person: CentralPerson | null;
  servers: CentralServerDisplay[];
  live: CentralBootstrap | null;
  error?: unknown;
};

function retainedDirectory(session: CentralSession): CentralDirectoryState {
  try {
    return { status: "central-unconfirmed", person: session.person,
      servers: loadCentralDirectoryCache(session.person.person_id), live: null };
  } catch (error) {
    return { status: "central-unconfirmed", person: session.person, servers: [], live: null, error };
  }
}

export function useCentralDirectory(autoStart = false) {
  const [directory, setDirectory] = useState<CentralDirectoryState | null>(() => {
    const session = autoStart && centralIdentityConfigured() ? loadCentralSession() : null;
    return session ? retainedDirectory(session) : null;
  });
  const active = useRef(true);
  const flight = useRef<Promise<CentralDirectoryState> | null>(null);
  const flightToken = useRef<string | undefined>(undefined);
  const controller = useRef<AbortController | null>(null);
  const lastSuccess = useRef<{ token: string | undefined; at: number } | null>(null);
  const refresh = useCallback((): Promise<CentralDirectoryState> => {
    const session = loadCentralSession();
    if (flight.current && flightToken.current === session?.token) return flight.current;
    controller.current?.abort();
    flightToken.current = session?.token;
    const abort = new AbortController();
    controller.current = abort;
    const request = (async () => {
      let next: CentralDirectoryState;
      try {
        const live = await bootstrapCentral(abort.signal);
        next = { status: live ? "connected" : "authentication-required", person: live?.person || null,
          servers: live?.servers || [], live };
      } catch (error) {
        if (abort.signal.aborted) throw error;
        const current = loadCentralSession();
        if (isCentralTemporaryError(error) && current && current.token === session?.token) {
          next = retainedDirectory(current);
        } else {
          next = { status: isCentralAuthenticationError(error) || !current ? "authentication-required" : "error",
            person: null, servers: [], live: null, error };
        }
      }
      const currentToken = loadCentralSession()?.token;
      if (abort.signal.aborted || currentToken !== session?.token) {
        throw new DOMException("로그인 서버 확인 요청이 바뀌었어요.", "AbortError");
      }
      if (active.current && controller.current === abort) {
        if (next.status === "connected") lastSuccess.current = { token: currentToken, at: Date.now() };
        setDirectory(next);
      }
      return next;
    })();
    flight.current = request;
    void request.finally(() => { if (flight.current === request) flight.current = null; }).catch(() => undefined);
    return request;
  }, []);

  useEffect(() => {
    const cleared = () => {
      controller.current?.abort();
      flight.current = null;
      lastSuccess.current = null;
      setDirectory({ status: "authentication-required", person: null, servers: [], live: null });
    };
    window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, cleared);
    return () => window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, cleared);
  }, []);

  useEffect(() => {
    active.current = true;
    if (autoStart && centralIdentityConfigured() && loadCentralSession()) void refresh().catch(() => undefined);
    return () => { active.current = false; controller.current?.abort(); flight.current = null; };
  }, [autoStart, refresh]);
  useEffect(() => {
    if (!directory || directory.status === "authentication-required") return;
    const retry = () => { void refresh().catch(() => undefined); };
    const focus = () => {
      const checked = lastSuccess.current;
      if (checked && checked.token === loadCentralSession()?.token && Date.now() - checked.at < FOCUS_CHECK_MS) return;
      retry();
    };
    const visible = () => { if (document.visibilityState === "visible") focus(); };
    window.addEventListener("focus", focus);
    document.addEventListener("visibilitychange", visible);
    window.addEventListener("online", retry);
    return () => {
      window.removeEventListener("focus", focus);
      document.removeEventListener("visibilitychange", visible);
      window.removeEventListener("online", retry);
    };
  }, [directory, refresh]);
  return { directory, refresh };
}
