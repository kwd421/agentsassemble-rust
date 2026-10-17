import { useCallback, useEffect, useRef, useState } from "react";
import { CENTRAL_SESSION_CLEARED_EVENT, bootstrapCentral, centralIdentityConfigured, loadCentralSession, isCentralAuthenticationError,
  type CentralBootstrap, type CentralPerson, type CentralSession } from "../lib/central/identity";
import { loadCentralDirectoryCache, type CentralServerDisplay } from "../lib/central/directoryCache";
import { isCentralTemporaryError } from "../lib/central/connectionError";
import { centralThrottledWaitMs } from "../lib/central/centralRequestGovernor";

// Focus/visible checks are throttled; online and explicit actions refresh immediately.
const FOCUS_CHECK_MS = 5 * 60_000;

// Bounded directory retry after a temporary/429 failure: generic outages
// back off 30 s doubling to a 5 min cap, while a 429 honors the server's
// Retry-After and the governor block (up to 10 min). A pending timer is
// kept instead of reset so repeated focus churn cannot starve the retry.
const DIRECTORY_RETRY_OFFLINE_BASE_MS = 30_000;
const DIRECTORY_RETRY_OFFLINE_CAP_MS = 300_000;
const DIRECTORY_RETRY_RATE_LIMIT_MAX_MS = 600_000;

function directoryRetryDelayMs(error: unknown, failures: number, accountId: string): number {
  const serverWait = (error as { retryAfterMs?: unknown } | null)?.retryAfterMs;
  const serverMs = typeof serverWait === "number" && Number.isFinite(serverWait) && serverWait >= 0
    ? serverWait : null;
  const governorMs = centralThrottledWaitMs("/v1/bootstrap", accountId);
  const honored = Math.max(serverMs ?? 0, governorMs);
  if (honored > 0) return Math.min(Math.max(Math.ceil(honored), 1_000), DIRECTORY_RETRY_RATE_LIMIT_MAX_MS);
  const step = Number.isSafeInteger(failures) ? Math.min(Math.max(failures - 1, 0), 3) : 0;
  return Math.min(DIRECTORY_RETRY_OFFLINE_BASE_MS * 2 ** step, DIRECTORY_RETRY_OFFLINE_CAP_MS);
}

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
  // Bounded retry for an unconfirmed directory: one pending timer that
  // re-runs refresh when it elapses. Owned here, derived from the refresh
  // outcome; focus/visible/online/explicit paths share it and never add a
  // second one.
  const retryTimer = useRef<number | null>(null);
  const tempFailures = useRef(0);
  const retryToken = useRef<string | undefined>(undefined);
  const clearDirectoryRetry = useCallback(() => {
    if (retryTimer.current !== null) window.clearTimeout(retryTimer.current);
    retryTimer.current = null;
  }, []);
  const refreshRef = useRef<() => Promise<CentralDirectoryState>>(() => Promise.reject(new Error("unmounted")));
  const scheduleDirectoryRetry = useCallback((error: unknown, accountId: string) => {
    if (!active.current || retryTimer.current !== null) return;
    tempFailures.current += 1;
    const delay = directoryRetryDelayMs(error, tempFailures.current, accountId);
    retryTimer.current = window.setTimeout(() => {
      retryTimer.current = null;
      void refreshRef.current().catch(() => undefined);
    }, delay);
  }, []);
  const refresh = useCallback((): Promise<CentralDirectoryState> => {
    const session = loadCentralSession();
    if (retryToken.current !== session?.token) {
      retryToken.current = session?.token;
      tempFailures.current = 0;
      clearDirectoryRetry();
    }
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
          // A superseded request never schedules: only the current refresh
          // owns the single retry timer.
          if (controller.current === abort) scheduleDirectoryRetry(error, current.person.person_id);
        } else {
          next = { status: isCentralAuthenticationError(error) || !current ? "authentication-required" : "error",
            person: null, servers: [], live: null, error };
          tempFailures.current = 0;
          clearDirectoryRetry();
        }
      }
      const currentToken = loadCentralSession()?.token;
      if (abort.signal.aborted || currentToken !== session?.token) {
        throw new DOMException("로그인 서버 확인 요청이 바뀌었어요.", "AbortError");
      }
      if (active.current && controller.current === abort) {
        if (next.status === "connected") {
          lastSuccess.current = { token: currentToken, at: Date.now() };
          tempFailures.current = 0;
          clearDirectoryRetry();
        }
        setDirectory(next);
      }
      return next;
    })();
    flight.current = request;
    void request.finally(() => { if (flight.current === request) flight.current = null; }).catch(() => undefined);
    return request;
  }, [clearDirectoryRetry, scheduleDirectoryRetry]);
  refreshRef.current = refresh;

  useEffect(() => {
    const cleared = () => {
      controller.current?.abort();
      flight.current = null;
      lastSuccess.current = null;
      tempFailures.current = 0;
      clearDirectoryRetry();
      setDirectory({ status: "authentication-required", person: null, servers: [], live: null });
    };
    window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, cleared);
    return () => window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, cleared);
  }, [clearDirectoryRetry]);

  useEffect(() => {
    active.current = true;
    if (autoStart && centralIdentityConfigured() && loadCentralSession()) void refresh().catch(() => undefined);
    return () => { active.current = false; controller.current?.abort(); flight.current = null; clearDirectoryRetry(); };
  }, [autoStart, refresh, clearDirectoryRetry]);
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
