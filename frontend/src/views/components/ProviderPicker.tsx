import { useMemo, useState } from "react";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { isProviderUnavailable } from "../../lib/providerCatalogGroups";
import { Search, Star } from "../../ui/icons";
import ProviderLogo from "./ProviderLogo";
import { RUN_MODE_LABEL, vendorEntries, vendorForProvider, type RunMode, type VendorEntry } from "./providerVendors";

// The add-agent dialog's first step is a short searchable list instead of a wall
// of tiles. It can be narrowed by how an AI runs (구독 / API / 로컬), with starred
// favourites on top. The settings step shows the chosen AI as a card offering its
// run modes. A provider that is not installed stays selectable; the setup below
// the card offers to install it and then signs in.
const FAVOURITES_KEY = "agentsassemble.aiPicker.favourites.v1";

// Favourites are a per-viewer convenience: unreadable storage just means none.
function loadFavourites(): string[] {
  try {
    const stored = JSON.parse(localStorage.getItem(FAVOURITES_KEY) || "[]");
    return Array.isArray(stored) ? stored.filter((item): item is string => typeof item === "string") : [];
  } catch { return []; }
}

function saveFavourites(ids: string[]) {
  try { localStorage.setItem(FAVOURITES_KEY, JSON.stringify(ids)); } catch { /* not remembered */ }
}

// The app or service behind an AI (e.g. ChatGPT runs through Codex), when it is not the AI's own name.
function appNames(entry: VendorEntry): string[] {
  return [...new Set(entry.modes.map(({ provider }) => provider.display_name))].filter((name) => name && name !== entry.name);
}

export default function ProviderPicker({ providers, selectedId, onSelect, view, remoteCatalog = false, disabled = false }: {
  providers: NativeCliProviderAvailability[];
  selectedId: string;
  view: "list" | "card";
  onSelect: (provider: NativeCliProviderAvailability) => void;
  remoteCatalog?: boolean;
  disabled?: boolean;
}) {
  const entries = useMemo(() => vendorEntries(providers), [providers]);
  const selected = vendorForProvider(entries, selectedId);
  const [query, setQuery] = useState("");
  const [modeFilter, setModeFilter] = useState<RunMode | null>(null);
  const [favourites, setFavourites] = useState(loadFavourites);
  const needle = query.trim().toLocaleLowerCase();
  const matches = entries.filter((entry) => (!modeFilter || entry.modes.some(({ mode }) => mode === modeFilter))
    && (!needle || entry.name.toLocaleLowerCase().includes(needle)
      || entry.modes.some(({ provider }) => provider.display_name.toLocaleLowerCase().includes(needle))));
  const availableModes = (Object.keys(RUN_MODE_LABEL) as RunMode[])
    .filter((mode) => entries.some((entry) => entry.modes.some((item) => item.mode === mode)));

  function toggleFavourite(id: string) {
    const next = favourites.includes(id) ? favourites.filter((item) => item !== id) : [...favourites, id];
    setFavourites(next);
    saveFavourites(next);
  }
  const missing = (entry: VendorEntry) => !remoteCatalog && entry.modes.every(({ provider }) => isProviderUnavailable(provider));

  function choose(entry: VendorEntry) {
    const candidates = modeFilter ? entry.modes.filter(({ mode }) => mode === modeFilter) : entry.modes;
    const ready = candidates.find(({ provider }) => remoteCatalog || !isProviderUnavailable(provider)) ?? candidates[0];
    onSelect(ready.provider);
    setQuery("");
  }

  if (view === "list" || !selected) {
    // One list: starred first, then ready apps, then ones that still need installing.
    const rank = (entry: VendorEntry) => (favourites.includes(entry.id) ? 0 : 2) + (missing(entry) ? 1 : 0);
    const ordered = [...matches].sort((a, b) => rank(a) - rank(b));
    return <section className="dc-provider-picker" aria-label="AI 고르기">
      <label className="dc-search">
        <Search size={16} aria-hidden />
        <input autoFocus type="search" value={query} placeholder="AI 검색" aria-label="AI 검색"
          onChange={(event) => setQuery(event.target.value)} />
      </label>
      {availableModes.length > 1 && <div className="dc-provider-filters" role="radiogroup" aria-label="실행 방식으로 거르기">
        {[null, ...availableModes].map((mode) => <button key={mode ?? "all"} type="button" role="radio"
          aria-checked={modeFilter === mode} onClick={() => setModeFilter(mode)}>
          {mode ? RUN_MODE_LABEL[mode] : "전체"}
        </button>)}
      </div>}
      <div className="dc-provider-picker-scroll">
      {matches.length === 0 && <p className="dc-provider-empty">찾는 AI가 없어요.</p>}
      <div role="list" aria-label="AI 목록" className="dc-provider-list">
        {ordered.map((entry) => {
          const starred = favourites.includes(entry.id);
          return <div key={entry.id} className="dc-provider-row-wrap">
            {/* Keep the search field focused through the press. Otherwise its
                blur re-render can replace the row between pointer down and up,
                so the first click never fires. */}
            <button type="button" role="listitem" className="dc-provider-row"
              onMouseDown={(event) => event.preventDefault()}
              disabled={disabled} aria-label={[entry.name, ...appNames(entry)].join(" ")} onClick={() => choose(entry)}>
              <ProviderLogo providerId={entry.modes[0].provider.id} providerKind={entry.modes[0].provider.provider_kind} size={20} />
              <span className="dc-provider-row-name">{entry.name}</span>
              {missing(entry) && <span className="dc-provider-row-meta">설치 필요</span>}
            </button>
            <button type="button" className="dc-provider-star" aria-pressed={starred}
              onMouseDown={(event) => event.preventDefault()}
              aria-label={`${entry.name} 즐겨찾기`} onClick={() => toggleFavourite(entry.id)}>
              <Star size={16} fill={starred ? undefined : "none"} aria-hidden />
            </button>
          </div>;
        })}
      </div>
      </div>
    </section>;
  }

  const current = selected.modes.find(({ provider }) => provider.id === selectedId);
  return <section className="dc-provider-selected" aria-label="고른 AI">
    <ProviderLogo providerId={selectedId} providerKind={current?.provider.provider_kind} size={32} />
    <span className="dc-provider-selected-name">{selected.name}</span>
    {/* One way to run it is a fact, not a choice: a tag, not a lone toggle button. */}
    {selected.modes.filter(({ mode }) => mode).length === 1 && current?.mode &&
      <span className="dc-provider-mode-tag">{RUN_MODE_LABEL[current.mode]}</span>}
    {selected.modes.filter(({ mode }) => mode).length > 1 && <span className="dc-provider-modes" role="radiogroup" aria-label="실행 방식">
      {selected.modes.map(({ mode, provider }) => mode && <button key={mode} type="button" role="radio"
        aria-checked={provider.id === selectedId} disabled={disabled} onClick={() => onSelect(provider)}>
        {RUN_MODE_LABEL[mode]}
      </button>)}
    </span>}
  </section>;
}
