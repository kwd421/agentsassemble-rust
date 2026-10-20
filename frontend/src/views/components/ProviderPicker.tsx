import { useMemo, useState } from "react";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { isProviderUnavailable } from "../../lib/providerCatalogGroups";
import { Plus, Search } from "../../ui/icons";
import ProviderLogo from "./ProviderLogo";
import { RUN_MODE_LABEL, vendorEntries, vendorForProvider, type VendorEntry } from "./providerVendors";

// One "+" card instead of a wall of tiles: it opens a short searchable list, and
// the chosen AI's card then offers how it runs (구독 / API / 로컬).
// A provider that is not installed stays selectable; the setup below the card
// offers to install it and then signs in.
// The app or service behind an AI (e.g. ChatGPT runs through Codex), when it is not the AI's own name.
function appNames(entry: VendorEntry): string[] {
  return [...new Set(entry.modes.map(({ provider }) => provider.display_name))].filter((name) => name && name !== entry.name);
}

export default function ProviderPicker({ providers, selectedId, onSelect, remoteCatalog = false, disabled = false }: {
  providers: NativeCliProviderAvailability[];
  selectedId: string;
  onSelect: (provider: NativeCliProviderAvailability) => void;
  remoteCatalog?: boolean;
  disabled?: boolean;
}) {
  const entries = useMemo(() => vendorEntries(providers), [providers]);
  const selected = vendorForProvider(entries, selectedId);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const needle = query.trim().toLocaleLowerCase();
  const matches = entries.filter((entry) => !needle || entry.name.toLocaleLowerCase().includes(needle)
    || entry.modes.some(({ provider }) => provider.display_name.toLocaleLowerCase().includes(needle)));
  const missing = (entry: VendorEntry) => !remoteCatalog && entry.modes.every(({ provider }) => isProviderUnavailable(provider));

  function choose(entry: VendorEntry) {
    const ready = entry.modes.find(({ provider }) => remoteCatalog || !isProviderUnavailable(provider)) ?? entry.modes[0];
    onSelect(ready.provider);
    setOpen(false);
    setQuery("");
  }

  if (open || !selected) {
    if (!open) {
      return <button type="button" className="dc-provider-add-card" disabled={disabled} onClick={() => setOpen(true)}>
        <Plus size={20} aria-hidden />AI 고르기
      </button>;
    }
    // One list: ready apps first, ones that still need installing after.
    const ordered = [...matches.filter((entry) => !missing(entry)), ...matches.filter(missing)];
    return <section className="dc-provider-picker" aria-label="AI 고르기">
      <label className="dc-provider-search">
        <Search size={16} aria-hidden />
        <input autoFocus type="search" value={query} placeholder="AI 검색" aria-label="AI 검색"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); setOpen(false); setQuery(""); } }} />
      </label>
      <div className="dc-provider-picker-scroll">
      {matches.length === 0 && <p className="dc-provider-empty">찾는 AI가 없어요.</p>}
      <div role="list" aria-label="AI 목록" className="dc-provider-list">
        {ordered.map((entry) => <button key={entry.id} type="button" role="listitem" className="dc-provider-row"
          disabled={disabled} aria-label={[entry.name, ...appNames(entry)].join(" ")} onClick={() => choose(entry)}>
          <ProviderLogo providerId={entry.modes[0].provider.id} providerKind={entry.modes[0].provider.provider_kind} size={20} />
          <span className="dc-provider-row-name">{entry.name}</span>
          {missing(entry) && <span className="dc-provider-row-meta">설치 필요</span>}
        </button>)}
      </div>
      </div>
    </section>;
  }

  const current = selected.modes.find(({ provider }) => provider.id === selectedId);
  return <section className="dc-provider-selected" aria-label="고른 AI">
    <ProviderLogo providerId={selectedId} providerKind={current?.provider.provider_kind} size={32} />
    <span className="dc-provider-selected-name">{selected.name}</span>
    {selected.modes.some(({ mode }) => mode) && <span className="dc-provider-modes" role="radiogroup" aria-label="실행 방식">
      {selected.modes.map(({ mode, provider }) => mode && <button key={mode} type="button" role="radio"
        aria-checked={provider.id === selectedId} disabled={disabled} onClick={() => onSelect(provider)}>
        {RUN_MODE_LABEL[mode]}
      </button>)}
    </span>}
    <button type="button" className="dc-provider-change" disabled={disabled} onClick={() => setOpen(true)}>바꾸기</button>
  </section>;
}
