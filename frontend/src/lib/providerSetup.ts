import { PROVIDER_SETUP_BASE, PROVIDER_SETUP_DESTINATIONS } from "../types/generated/PROVIDER_SETUP";

export function providerSetupDestination(providerId: string) {
  return PROVIDER_SETUP_DESTINATIONS.find((destination) => destination.provider_id === providerId);
}

export function providerSetupLink(providerId: string): string | null {
  const destination = providerSetupDestination(providerId);
  return destination ? `${PROVIDER_SETUP_BASE}/${destination.provider_id}` : null;
}
