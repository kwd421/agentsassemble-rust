import type { ProviderCatalogSnapshot } from "./roomSocketTypes";

export function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

export function agentSessionIsValid(value: unknown, expectedRoomId = ""): boolean {
  if (!isRecord(value)) return false;
  return Boolean(
    typeof value.room_id === "string" &&
    value.room_id &&
    (!expectedRoomId || value.room_id === expectedRoomId) &&
    typeof value.session_id === "string" &&
    value.session_id &&
    value.participant_id === value.session_id &&
    typeof value.display_name === "string" &&
    value.display_name &&
    typeof value.runtime_status === "string" &&
    [
      "stopped",
      "available",
      "starting",
      "idle",
      "busy",
      "paused",
      "recovering",
      "stopping",
      "error",
      "disconnected",
    ].includes(value.runtime_status) &&
    value.process_ownership === "server" &&
    value.external_owned === false &&
    typeof value.provider_kind === "string" &&
    value.provider_kind &&
    typeof value.model === "string" &&
    value.model &&
    !("workspace" in value) &&
    !("executable" in value) &&
    !("workspace_identity" in value) &&
    !("executable_identity" in value) &&
    !("runtime_profile_key" in value) &&
    !("runtime_profile_version" in value) &&
    !("runtime_handle_id" in value) &&
    !("provider_session_id" in value) &&
    !("lifecycle_intent_action" in value) &&
    !("lifecycle_intent_id" in value) &&
    !("lifecycle_intent_status" in value)
  );
}

export function providerCatalogIsValid(value: unknown): value is ProviderCatalogSnapshot {
  if (!isRecord(value)) return false;
  if (
    (value.status !== "loading" && value.status !== "ready" && value.status !== "failed") ||
    typeof value.catalog_revision !== "string" ||
    !Array.isArray(value.providers)
  ) return false;
  return value.providers.every((provider) => Boolean(
    isRecord(provider) &&
    typeof provider.id === "string" &&
    provider.id &&
    typeof provider.display_name === "string" &&
    typeof provider.provider_kind === "string" &&
    typeof provider.runtime_kind === "string" &&
    typeof provider.startable === "boolean" &&
    typeof provider.available === "boolean" &&
    typeof provider.discovery_status === "string" &&
    Array.isArray(provider.controls) &&
    provider.controls.every((control) => Boolean(
      isRecord(control) &&
      typeof control.key === "string" &&
      typeof control.label === "string" &&
      typeof control.kind === "string" &&
      typeof control.default_value === "string" &&
      Array.isArray(control.options)
    ))
  ));
}
