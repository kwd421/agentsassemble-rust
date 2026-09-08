import { useState } from "react";
import { readProviderUsage } from "../../../api/providerUsage";
import { isDesktopWebview } from "../../../lib/desktopBridge";
import type { ProviderUsage } from "../../../types/generated/ProviderUsage";
import type { NativeCliProviderAvailability } from "../../../roomSocketClient";

export default function MemberUsage({ displayName, provider }: {
  displayName: string;
  provider?: NativeCliProviderAvailability;
}) {
  const [usage, setUsage] = useState<ProviderUsage | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const supported = provider?.usage_supported === true;
  const local = isDesktopWebview();
  async function read() {
    if (!provider) return;
    setBusy(true);
    setError("");
    setUsage(null);
    try { setUsage(await readProviderUsage(provider.id)); }
    catch (cause) { setError(cause instanceof Error ? cause.message : "사용량을 조회하지 못했어요."); }
    finally { setBusy(false); }
  }
  return (
    <section className="dc-member-detail-section" aria-label={`${displayName} 사용량`}>
      <h3>사용량</h3>
      {!supported ? <p className="dc-member-detail-note preserve-words">
        이 Provider는 확인 가능한 정확한 잔여량을 제공하지 않습니다.
      </p> : !local ? <p className="dc-member-detail-note preserve-words">
        계정 사용량은 서버 운영자의 데스크톱 앱에서 확인할 수 있어요.
      </p> : <>
        <button type="button" className="ops-button" style={{ minHeight: 44, padding: "0 12px" }} disabled={busy} onClick={() => void read()}>
          {busy ? "조회 중…" : "계정 사용량 조회"}
        </button>
        {error && <p role="alert" className="dc-member-detail-note preserve-words">{error}</p>}
        {usage && <div role="status">
          <p className="dc-member-detail-note">조회 시각: {new Date(usage.observed_at).toLocaleString()}</p>
          {usage.quota.balances.map((balance) => <p key={balance.currency} className="dc-member-detail-note preserve-words">
            {balance.currency} 잔액 {balance.total_balance} · 지급 {balance.granted_balance} · 충전 {balance.topped_up_balance}
          </p>)}
          {!usage.quota.is_available && <p className="dc-member-detail-note">현재 계정 잔액으로 API를 사용할 수 없어요.</p>}
        </div>}
      </>}
    </section>
  );
}
