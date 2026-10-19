import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isBundledDesktopWebview } from "../../lib/desktopBridge";

export default function AppUpdateCheck() {
  const [pending, setPending] = useState(false);
  const [result, setResult] = useState("");
  const [error, setError] = useState("");
  if (!isBundledDesktopWebview()) return null;
  async function check() {
    setPending(true); setResult(""); setError("");
    try {
      const version = await invoke<string | null>("app_update_check");
      setResult(version === null ? "최신 버전이에요" : `새 버전 ${version}`);
    } catch (reason) {
      setError(typeof reason === "string" ? reason : "업데이트를 확인하지 못했어요. 다시 시도해 주세요.");
    } finally { setPending(false); }
  }
  return <div className="dc-user-settings-update">
    <button type="button" disabled={pending} onClick={() => void check()}>
      {pending ? "확인하는 중…" : "업데이트 확인"}
    </button>
    <p role="status">{result}</p>
    {error && <p role="alert">{error}</p>}
  </div>;
}
