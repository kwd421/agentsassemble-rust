import type { ProviderAvailability } from "../types/generated/ProviderAvailability";
import type { ProviderCatalog } from "../types/generated/ProviderCatalog";
import type { ProviderControl } from "../types/generated/ProviderControl";
import type { ProviderControlOption } from "../types/generated/ProviderControlOption";
import {
  assertExactKeys,
  strictRecord,
  type ExactGeneratedKeys,
} from "./strictJsonContract";

const CATALOG_OPTIONAL_KEYS = ["discovered_at"] as const;
const GENERATED_CATALOG_KEYS = [
  "status",
  "catalog_revision",
  "discovered_at",
  "providers",
] as const satisfies readonly (keyof ProviderCatalog)[];
const CATALOG_KEYS: ExactGeneratedKeys<
  ProviderCatalog,
  typeof GENERATED_CATALOG_KEYS
> = GENERATED_CATALOG_KEYS;
const CATALOG_REQUIRED_KEYS = CATALOG_KEYS.filter(
  (key) => !CATALOG_OPTIONAL_KEYS.includes(key as "discovered_at")
);
const PROVIDER_OPTIONAL_KEYS = ["discovery_error_code", "discovery_error"] as const;
const GENERATED_PROVIDER_KEYS = [
  "id",
  "display_name",
  "provider_kind",
  "runtime_kind",
  "catalog_group",
  "workspace_required",
  "connection_kind",
  "default_model",
  "interactive",
  "startable",
  "available",
  "discovery_status",
  "catalog_source",
  "discovery_error_code",
  "discovery_error",
  "credential_available",
  "controls",
] as const satisfies readonly (keyof ProviderAvailability)[];
const PROVIDER_KEYS: ExactGeneratedKeys<
  ProviderAvailability,
  typeof GENERATED_PROVIDER_KEYS
> = GENERATED_PROVIDER_KEYS;
const PROVIDER_REQUIRED_KEYS = PROVIDER_KEYS.filter(
  (key) => !PROVIDER_OPTIONAL_KEYS.includes(
    key as (typeof PROVIDER_OPTIONAL_KEYS)[number]
  )
);
const GENERATED_CONTROL_KEYS = [
  "key",
  "label",
  "kind",
  "options",
  "default_value",
] as const satisfies readonly (keyof ProviderControl)[];
const CONTROL_KEYS: ExactGeneratedKeys<
  ProviderControl,
  typeof GENERATED_CONTROL_KEYS
> = GENERATED_CONTROL_KEYS;
const OPTION_OPTIONAL_KEYS = ["metadata"] as const;
const GENERATED_OPTION_KEYS = ["value", "label", "metadata"] as const satisfies readonly (
  keyof ProviderControlOption
)[];
const OPTION_KEYS: ExactGeneratedKeys<
  ProviderControlOption,
  typeof GENERATED_OPTION_KEYS
> = GENERATED_OPTION_KEYS;
const OPTION_REQUIRED_KEYS = OPTION_KEYS.filter(
  (key) => !OPTION_OPTIONAL_KEYS.includes(key as "metadata")
);

function exactRecord(
  value: unknown,
  required: readonly string[],
  optional: readonly string[] = [],
): Record<string, unknown> {
  const record = strictRecord(value, "provider catalog");
  assertExactKeys(record, required, "provider catalog", optional);
  return record;
}

function optionIsValid(value: unknown): boolean {
  const option = exactRecord(value, OPTION_REQUIRED_KEYS, OPTION_OPTIONAL_KEYS);
  return (
    typeof option.value === "string" &&
    typeof option.label === "string" &&
    (option.metadata === undefined ||
      (Boolean(option.metadata) &&
        typeof option.metadata === "object" &&
        !Array.isArray(option.metadata)))
  );
}

function controlIsValid(value: unknown): boolean {
  const control = exactRecord(value, CONTROL_KEYS);
  return (
    typeof control.key === "string" &&
    Boolean(control.key) &&
    typeof control.label === "string" &&
    Boolean(control.label) &&
    (control.kind === "select" || control.kind === "combobox") &&
    typeof control.default_value === "string" &&
    Array.isArray(control.options) &&
    control.options.every(optionIsValid) &&
    new Set(control.options.map((option) => (option as { value: string }).value)).size ===
      control.options.length
  );
}

function providerIsValid(value: unknown): boolean {
  const provider = exactRecord(
    value,
    PROVIDER_REQUIRED_KEYS,
    PROVIDER_OPTIONAL_KEYS,
  );
  const stringKeys = [
    "id",
    "display_name",
    "provider_kind",
    "runtime_kind",
    "connection_kind",
    "default_model",
    "discovery_error_code",
    "discovery_error",
  ] as const;
  const booleanKeys = [
    "workspace_required",
    "interactive",
    "startable",
    "available",
    "credential_available",
  ] as const;
  return (
    stringKeys.every(
      (key) => provider[key] === undefined || typeof provider[key] === "string",
    ) &&
    Boolean(provider.id) &&
    Boolean(provider.display_name) &&
    Boolean(provider.provider_kind) &&
    Boolean(provider.runtime_kind) &&
    Boolean(provider.connection_kind) &&
    booleanKeys.every((key) => typeof provider[key] === "boolean") &&
    ["harness", "api", "local"].includes(String(provider.catalog_group)) &&
    ["loading", "ready", "failed"].includes(String(provider.discovery_status)) &&
    ["discovered", "static_manifest"].includes(String(provider.catalog_source)) &&
    Array.isArray(provider.controls) &&
    provider.controls.every(controlIsValid) &&
    new Set(provider.controls.map((control) => (control as { key: string }).key)).size ===
      provider.controls.length
  );
}

export function providerCatalogIsValid(value: unknown): value is ProviderCatalog {
  try {
    const catalog = exactRecord(value, CATALOG_REQUIRED_KEYS, CATALOG_OPTIONAL_KEYS);
    if (
      !["loading", "ready", "failed"].includes(String(catalog.status)) ||
      typeof catalog.catalog_revision !== "string" ||
      (catalog.discovered_at !== undefined && typeof catalog.discovered_at !== "string") ||
      !Array.isArray(catalog.providers) ||
      !catalog.providers.every(providerIsValid)
    ) {
      return false;
    }
    return new Set(
      catalog.providers.map((provider) => (provider as ProviderAvailability).id),
    ).size === catalog.providers.length;
  } catch {
    return false;
  }
}
