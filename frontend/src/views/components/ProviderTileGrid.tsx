import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { PROVIDER_GROUPS, isProviderUnavailable, projectProvidersByCatalogGroup } from "../../lib/providerCatalogGroups";
import ProviderLogo from "./ProviderLogo";
import { resolveProviderPresentation } from "./providerBranding";

export default function ProviderTileGrid({ providers, selectedId, onSelect, remoteCatalog = false, disabled = false }: {
  providers: NativeCliProviderAvailability[];
  selectedId: string;
  onSelect: (provider: NativeCliProviderAvailability) => void;
  remoteCatalog?: boolean;
  disabled?: boolean;
}) {
  const grouped = projectProvidersByCatalogGroup(providers);
  return <>{PROVIDER_GROUPS.filter(({ id }) => grouped[id].length > 0).map(({ id, label }) => (
    <section className="dc-agent-section dc-agent-provider-category" key={id}>
      <h3 className="dc-agent-section-title">{label}</h3>
      <div className="dc-agent-provider-grid" role="list" aria-label={`${label} AI`}>
        {grouped[id].map((provider) => {
          const { providerName } = resolveProviderPresentation({ providerId: provider.id,
            providerKind: provider.provider_kind, providerDisplayName: provider.display_name });
          return <button key={provider.id} type="button" role="listitem" aria-label={providerName}
            title={providerName} data-active={provider.id === selectedId}
            data-unavailable={!remoteCatalog && isProviderUnavailable(provider)} disabled={disabled}
            onClick={() => onSelect(provider)}>
            <ProviderLogo providerId={provider.id} providerKind={provider.provider_kind} size={22} />
            <span className="min-w-0 preserve-words">{providerName}</span>
          </button>;
        })}
      </div>
    </section>
  ))}</>;
}
