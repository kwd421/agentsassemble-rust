import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import { providerBrandKey, PROVIDER_BRANDS, type ProviderBrandKey } from "./providerBranding";

// The add-agent picker lists companies, not runtimes: one card per AI, and the
// way it runs (subscription app, API key, local model) is chosen inside it.
// Routers stay as their own entries.
export type RunMode = "subscription" | "api" | "local";
export const RUN_MODE_LABEL: Record<RunMode, string> = { subscription: "구독", api: "API", local: "로컬" };

type VendorDefinition = { id: string; name: string; modes: Partial<Record<RunMode, ProviderBrandKey>> };

const VENDORS: VendorDefinition[] = [
  { id: "chatgpt", name: "ChatGPT", modes: { subscription: "codex" } },
  { id: "claude", name: "Claude", modes: { subscription: "claude" } },
  { id: "grok", name: "Grok", modes: { subscription: "grok" } },
  { id: "deepseek", name: "DeepSeek", modes: { api: "deepseek" } },
  { id: "cerebras", name: "Cerebras", modes: { api: "cerebras" } },
  { id: "ollama", name: "Ollama", modes: { local: "ollama" } },
  { id: "lmstudio", name: "LM Studio", modes: { local: "lmstudio" } },
];

// OAuth-only products are left out of the picker for now (owner, 2026-10-20).
const HIDDEN: ProviderBrandKey[] = ["cursor", "antigravity", "freebuff"];

export type VendorEntry = {
  id: string;
  name: string;
  router: boolean;
  /** The catalog provider behind each available way to run it. */
  modes: { mode: RunMode | null; provider: NativeCliProviderAvailability }[];
};

const brandOf = (provider: NativeCliProviderAvailability) => providerBrandKey(provider.id, provider.provider_kind);

/** Groups the catalog into the picker's entries; anything unmapped stays reachable as a router entry. */
export function vendorEntries(providers: NativeCliProviderAvailability[]): VendorEntry[] {
  const used = new Set<string>();
  const vendors: VendorEntry[] = [];
  for (const vendor of VENDORS) {
    const modes = (Object.entries(vendor.modes) as [RunMode, ProviderBrandKey][]).flatMap(([mode, brand]) => {
      const provider = providers.find((item) => brandOf(item) === brand);
      if (!provider) return [];
      used.add(provider.id);
      return [{ mode, provider }];
    });
    if (modes.length) vendors.push({ id: vendor.id, name: vendor.name, router: false, modes });
  }
  const routers = providers.filter((provider) => !used.has(provider.id) && !HIDDEN.includes(brandOf(provider) as ProviderBrandKey))
    .map((provider) => {
      const brand = brandOf(provider);
      return { id: provider.id, name: brand ? PROVIDER_BRANDS[brand].productName : provider.display_name, router: true,
        modes: [{ mode: null, provider }] };
    });
  return [...vendors, ...routers];
}

export function vendorForProvider(entries: VendorEntry[], providerId: string): VendorEntry | undefined {
  return entries.find((entry) => entry.modes.some(({ provider }) => provider.id === providerId));
}
