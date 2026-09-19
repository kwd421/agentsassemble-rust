import ProviderInstallPrompt from "./ProviderInstallPrompt";
import ProviderUpdatePrompt from "./ProviderUpdatePrompt";
import type { ProviderAvailability } from "../../types/generated/ProviderAvailability";
import { useState } from "react";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { providerSetupDestination, providerSetupLink } from "../../lib/providerSetup";

export default function ProviderSetupActions({ providerId, provider, localAvailable = true, onUpdating, onUpdated }: {
  providerId: string;
  provider?: ProviderAvailability | null;
  localAvailable?: boolean;
  onUpdating?: (updating: boolean) => void;
  onUpdated?: () => void;
}) {
  const [status, setStatus] = useState("");
  const destination = providerSetupDestination(providerId);
  const link = providerSetupLink(providerId);
  if (!destination || !link) return null;
  const desktop = isDesktopWebview();
  if (desktop) {
    if (!localAvailable || !provider) return null;
    if (provider.discovery_error_code !== "command_missing") {
      return provider.update_supported && provider.available && provider.discovery_status !== "loading" &&
        provider.discovery_error_code !== "authentication_required"
        ? <ProviderUpdatePrompt key={providerId} providerId={providerId} provider={provider} onUpdating={onUpdating} onUpdated={onUpdated} /> : null;
    }
  }
  return <section className="dc-agent-section" aria-label={`${destination.display_name} 설치 및 로그인 도움말`}>
    {desktop && <ProviderInstallPrompt key={providerId} providerId={providerId} displayName={destination.display_name}
      installable={Boolean(provider?.install_supported)} onUpdating={onUpdating} onInstalled={onUpdated} />}
    {!desktop && <>
      <p className="dc-agent-hint preserve-words">이 PC의 로그인·설치 상태는 AgentsAssemble 앱에서 확인할 수 있어요.</p>
      <a className="ops-button rounded-lg px-4 py-2" href={link}
        style={{ minHeight: 44, display: "inline-flex", alignItems: "center", marginTop: 8 }}
        onClick={() => setStatus("앱이 열리지 않으면 이 PC에 AgentsAssemble이 설치되어 있는지 확인해 주세요.")}>
        이 PC에서 {destination.display_name} 설정 열기
      </a>
    </>}
    {!desktop && <a href={destination.help_url} target="_blank" rel="noopener noreferrer"
      style={{ minHeight: 44, display: "flex", alignItems: "center" }}>공식 설치·업데이트 안내</a>}
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </section>;
}
