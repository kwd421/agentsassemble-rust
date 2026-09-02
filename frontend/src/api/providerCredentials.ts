import {
  deleteJsonServerOperator,
  fetchJsonServerOperator,
  postJsonServerOperator,
} from "./http";
import { isDesktopWebview } from "../lib/desktopBridge";

export interface ProviderCredentialStatus {
  configured: boolean;
  source: "keyring" | "missing";
}

const CREDENTIAL_PATHS: Readonly<Record<string, string>> = {
  deepseek: "/api/provider-credentials/deepseek",
  cerebras: "/api/provider-credentials/cerebras",
  openrouter: "/api/provider-credentials/openrouter",
  vercel: "/api/provider-credentials/vercel",
  llmgateway: "/api/provider-credentials/llmgateway",
  tokenrouter: "/api/provider-credentials/tokenrouter",
  custom_api: "/api/provider-credentials/custom_api",
};

function credentialPath(providerId: string): string {
  const path = CREDENTIAL_PATHS[providerId];
  if (!path) {
    throw new Error(`Unsupported API credential provider: ${providerId}`);
  }
  if (!isDesktopWebview()) {
    throw new Error("Provider credential controls require the desktop Rust runtime.");
  }
  return path;
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
    !new Set(["keyring", "missing"]).has(record.source) ||
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
  apiKey: string
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
