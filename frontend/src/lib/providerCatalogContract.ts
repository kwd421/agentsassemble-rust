import type { ProviderAvailability } from "../types/generated/ProviderAvailability";
import type { ProviderCatalog } from "../types/generated/ProviderCatalog";
import { assertExactKeys, strictRecord } from "./strictJsonContract";

const CATALOG_KEYS = ["status", "catalog_revision", "providers"] as const;
const PROVIDER_KEYS = [
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
  "credential_available",
  "controls",
] as const;
const CONTROL_KEYS = ["key", "label", "kind", "options", "default_value"] as const;
const OPTION_KEYS = ["value", "label"] as const;

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
  const option = exactRecord(value, OPTION_KEYS, ["metadata"]);
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
  const provider = exactRecord(value, PROVIDER_KEYS, [
    "discovery_error_code",
    "discovery_error",
  ]);
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
    const catalog = exactRecord(value, CATALOG_KEYS, ["discovered_at"]);
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
