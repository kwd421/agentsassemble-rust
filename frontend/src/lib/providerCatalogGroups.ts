import type { NativeCliProviderAvailability } from "../roomSocketClient";

export type ProviderCatalogGroup = "harness" | "api" | "local";

export const PROVIDER_GROUPS = [
  { id: "harness", label: "구독 에이전트" },
  { id: "api", label: "API 키" },
  { id: "local", label: "내 컴퓨터" },
] as const;

export function providerCatalogGroup(
  provider: NativeCliProviderAvailability,
  model?: string
): ProviderCatalogGroup {
  const group = provider.controls.find((control) => control.key === "model")
    ?.options.find((option) => option.value === model)?.metadata?.catalog_group ?? provider.catalog_group;
  if (typeof group === "string" && ["harness", "api", "local"].includes(group)) {
    return group as ProviderCatalogGroup;
  }
  throw new Error("Provider catalog group is outside the current room contract.");
}

export function isProviderUnavailable(provider: NativeCliProviderAvailability): boolean {
  return provider.discovery_status !== "loading" && !provider.available;
}

export function projectProvidersByCatalogGroup(
  providers: NativeCliProviderAvailability[]
): Record<ProviderCatalogGroup, NativeCliProviderAvailability[]> {
  return Object.fromEntries(PROVIDER_GROUPS.map(({ id }) => [
    id,
    providers.filter((provider) => providerCatalogGroup(provider) === id)
      .sort((a, b) => Number(isProviderUnavailable(a)) - Number(isProviderUnavailable(b))),
  ])) as Record<ProviderCatalogGroup, NativeCliProviderAvailability[]>;
}

export function providerGroupLabel(group: ProviderCatalogGroup): string {
  return PROVIDER_GROUPS.find((item) => item.id === group)?.label || group;
}
