import {
  deleteJsonServerOperator,
  fetchJsonServerOperator,
  postJsonServerOperator,
} from "./http";
import { isDesktopWebview } from "../lib/desktopBridge";

export interface ProviderCredentialStatus {
  configured: boolean;
  source: "keyring" | "environment" | "missing";
}

const DEEPSEEK_CREDENTIAL_PATH = "/api/provider-credentials/deepseek";

function credentialPath(providerId: string): string {
  if (providerId !== "deepseek") {
    throw new Error(`Unsupported API credential provider: ${providerId}`);
  }
  if (!isDesktopWebview()) {
    throw new Error("Provider credential controls require the desktop Rust runtime.");
  }
  return DEEPSEEK_CREDENTIAL_PATH;
}

function providerCredentialStatus(value: unknown): ProviderCredentialStatus {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("Provider credential status is invalid.");
  }
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  if (
    keys.length !== 2 ||
    keys[0] !== "configured" ||
    keys[1] !== "source" ||
    typeof record.configured !== "boolean" ||
    typeof record.source !== "string" ||
    !new Set(["keyring", "environment", "missing"]).has(record.source) ||
    record.configured !== (record.source !== "missing")
  ) {
    throw new Error("Provider credential status is invalid.");
  }
  return record as unknown as ProviderCredentialStatus;
}

export async function fetchProviderCredentialStatus(
  providerId: string
): Promise<ProviderCredentialStatus> {
  return providerCredentialStatus(
    await fetchJsonServerOperator<unknown>(credentialPath(providerId))
  );
}

export async function setProviderCredential(
  providerId: string,
  apiKey: string,
  _options: { workspaceId?: string } = {}
): Promise<ProviderCredentialStatus> {
  return providerCredentialStatus(
    await postJsonServerOperator<unknown>(credentialPath(providerId), {
      api_key: apiKey,
    })
  );
}

export async function deleteProviderCredential(
  providerId: string
): Promise<ProviderCredentialStatus> {
  return providerCredentialStatus(
    await deleteJsonServerOperator<unknown>(credentialPath(providerId))
  );
}
