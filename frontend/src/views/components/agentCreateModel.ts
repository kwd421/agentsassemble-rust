import type {
  NativeCliProviderAvailability,
  ProviderControl,
} from "../../roomSocketClient";

export function deriveAgentCreateStatus({
  status,
  workspacePath,
  selectedProvider,
  selectedProviderMissing,
  hasProviders,
  invalidControl,
  existingSessionId,
  workspaceRequired,
}: {
  status: string;
  workspacePath: string;
  selectedProvider: NativeCliProviderAvailability | undefined;
  selectedProviderMissing: boolean;
  hasProviders: boolean;
  invalidControl: ProviderControl | undefined;
  existingSessionId: string;
  workspaceRequired: boolean;
}): string {
  if (status) return status;
  if (selectedProvider && !selectedProvider.available) {
    return selectedProvider.discovery_error || "CLI를 찾지 못했습니다";
  }
  if (selectedProvider?.discovery_status === "loading") {
    return "모델 목록을 불러오는 중입니다";
  }
  if (
    selectedProvider?.catalog_source === "stale_cache" &&
    selectedProvider.discovery_error
  ) {
    return selectedProvider.discovery_error;
  }
  if (selectedProvider?.discovery_status === "failed" && selectedProvider.available) {
    return selectedProvider.discovery_error || "모델 목록을 불러오지 못했습니다";
  }
  if (selectedProviderMissing) {
    return "선택한 provider가 현재 catalog에 없습니다.";
  }
  if (!selectedProvider && !selectedProviderMissing && hasProviders) {
    return "사용할 provider를 선택하세요.";
  }
  if (!existingSessionId && invalidControl) {
    return `${invalidControl.label}의 유효한 기본값이 없어 직접 선택해야 합니다.`;
  }
  if (
    !existingSessionId &&
    selectedProvider &&
    workspaceRequired &&
    !workspacePath.trim()
  ) {
    return "작업 폴더를 선택하세요. 이 폴더에서 세션이 실행됩니다.";
  }
  return "";
}

export function defaultAgentDisplayName(
  provider: NativeCliProviderAvailability,
  settings: Record<string, string>
): string {
  const providerName = provider.display_name.trim();
  const modelControl = provider.controls.find((control) => control.key === "model");
  const modelOption = modelControl?.options.find((option) => option.value === settings.model);
  const modelName = String(modelOption?.label || "").trim();
  if (!modelName) return providerName;

  const providerToken = providerName.toLocaleLowerCase();
  const modelToken = modelName.toLocaleLowerCase();
  if (
    modelToken === providerToken ||
    modelToken.startsWith(`${providerToken} `) ||
    modelToken.startsWith(`${providerToken}-`)
  ) {
    return modelName;
  }
  return `${providerName} ${modelName}`;
}
