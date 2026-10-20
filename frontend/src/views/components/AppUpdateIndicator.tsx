import { useEffect, useRef, useState } from "react";
import { Download } from "../../ui/icons";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { isBundledDesktopWebview } from "../../lib/desktopBridge";

type UpdateState = { status: "none" }
  | { status: "available" | "ready" | "installing"; version: string };

export default function AppUpdateIndicator() {
  const [state, setState] = useState<UpdateState>({ status: "none" });
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const installButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!isBundledDesktopWebview()) return;
    let active = true;
    let received = false;
    function accept(next: UpdateState) {
      if (!active) return;
      setState(next);
    }
    let unlisten: (() => void) | undefined;
    void listen<UpdateState>("application-update-state", ({ payload }) => {
      received = true;
      accept(payload);
    }).then(async (stop) => {
      unlisten = stop;
      if (!active) { stop(); return; }
      const snapshot = await invoke<UpdateState>("app_update_state");
      if (!received) accept(snapshot);
    }).catch(() => {
      if (active) setError("업데이트 상태를 확인하지 못했어요.");
    });
    return () => { active = false; unlisten?.(); };
  }, []);
  useEffect(() => {
    if (!open) return;
    installButton.current?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  if (!isBundledDesktopWebview() || (state.status !== "ready" && state.status !== "installing")) return null;
  const version = state.version;
  const installing = pending || state.status === "installing";
  function close() { setOpen(false); trigger.current?.focus(); }
  async function install() {
    setPending(true); setError("");
    try { await invoke("app_update_install"); }
    catch { setError("업데이트를 시작하지 못했어요. 다시 시도해 주세요."); }
    finally { setPending(false); }
  }
  return <div className="relative" ref={root} onKeyDown={(event) => {
    if (event.key === "Escape") { event.stopPropagation(); close(); }
  }}>
    <button ref={trigger} type="button" className="dc-head-icon"
      data-tip="업데이트" aria-label={`새 버전 ${version}`}
      aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen(!open)}>
      <Download size={20} />
      <span aria-hidden="true" className="dc-head-badge" />
    </button>
    {open && <div role="dialog" aria-label="앱 업데이트" className="dc-head-popover">
      <p className="dc-head-popover-title">업데이트가 준비됐어요</p>
      <p className="dc-head-popover-copy">버전 {version} · 다시 시작하면 적용돼요.</p>
      {error && <p role="alert" className="dc-head-popover-copy text-danger">{error}</p>}
      <div className="mt-3 flex justify-end gap-2">
        <button type="button" className="dc-button dc-button--secondary" onClick={close}>나중에</button>
        <button ref={installButton} type="button" className="dc-button dc-button--primary"
          disabled={installing} onClick={() => void install()}>
          {installing ? "업데이트하는 중…" : "다시 시작"}
        </button>
      </div>
    </div>}
  </div>;
}
