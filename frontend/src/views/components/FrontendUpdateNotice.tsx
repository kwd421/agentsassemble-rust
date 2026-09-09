import { useEffect, useState } from "react";
import { readRuntimeVersion } from "../../api/runtimeVersion";
import { PROTOCOL_VERSION } from "../../types/generated/PROTOCOL_VERSION";

type Observation = "pending" | "current" | "unavailable" | "updated" | "protocol_changed";

export default function FrontendUpdateNotice({ connected }: { connected: boolean }) {
  const [baseline] = useState(() => {
    const value = document.documentElement.dataset.agentsassembleBuild;
    return value && /^[a-f0-9]{64}$/.test(value) ? value : null;
  });
  const [observation, setObservation] = useState<Observation>(baseline ? "pending" : "unavailable");
  const [checking, setChecking] = useState(false);
  const [revision, setRevision] = useState(0);
  const updated = observation === "updated" || observation === "protocol_changed";
  useEffect(() => {
    if (!baseline || updated) return;
    let disposed = false;
    let request: AbortController | null = null;
    let deadline = 0;
    const check = async () => {
      if (disposed || request || document.visibilityState === "hidden") return;
      const controller = new AbortController();
      request = controller;
      setChecking(true);
      deadline = window.setTimeout(() => controller.abort(), 5_000);
      try {
        const version = await readRuntimeVersion(controller.signal);
        if (disposed) return;
        if (version.frontend_build_id === null) setObservation("unavailable");
        else if (version.protocol_version !== PROTOCOL_VERSION) setObservation("protocol_changed");
        else setObservation(version.frontend_build_id === baseline ? "current" : "updated");
      } catch {
        if (!disposed) setObservation("unavailable");
      } finally {
        window.clearTimeout(deadline);
        request = null;
        if (!disposed) setChecking(false);
      }
    };
    const checkVisible = () => { void check(); };
    checkVisible();
    const timer = window.setInterval(checkVisible, 15_000);
    window.addEventListener("focus", checkVisible);
    document.addEventListener("visibilitychange", checkVisible);
    return () => {
      disposed = true;
      window.clearInterval(timer);
      window.clearTimeout(deadline);
      request?.abort();
      window.removeEventListener("focus", checkVisible);
      document.removeEventListener("visibilitychange", checkVisible);
    };
  }, [baseline, connected, revision, updated]);

  if (observation === "pending" || observation === "current") return null;
  return <aside role="status" className="ops-inner rounded-xl p-4 text-sm" style={{
    position: "fixed", top: 16, left: 16, right: 16, marginInline: "auto", zIndex: 300,
    width: "calc(100% - 32px)", maxWidth: 540, display: "flex", alignItems: "center", gap: 12,
  }}>
    <p style={{ flex: 1 }}>{updated
      ? observation === "protocol_changed" ? "새 버전에 맞춰 화면을 새로고침해 주세요." : "새 화면 버전이 준비됐어요."
      : "화면 버전을 확인하지 못했어요."}</p>
    <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44, flexShrink: 0 }}
      disabled={checking} onClick={() => {
        if (updated || !baseline) window.location.reload();
        else setRevision((value) => value + 1);
      }}>{updated || !baseline ? "새로고침" : checking ? "확인 중" : "다시 확인"}</button>
  </aside>;
}
