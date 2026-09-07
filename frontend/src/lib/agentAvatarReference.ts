import { AGENT_AVATAR_HEX_LENGTH, AGENT_AVATAR_ID_PREFIX, AGENT_AVATAR_REFERENCE_PREFIX } from "../types/generated/AGENT_AVATAR_WIRE";

export function parseAgentAvatarReference(value: unknown): { assetId: string; url: string } | null {
  if (typeof value !== "string" || !value.startsWith(AGENT_AVATAR_REFERENCE_PREFIX)) return null;
  const assetId = value.slice(AGENT_AVATAR_REFERENCE_PREFIX.length);
  if (!assetId.startsWith(AGENT_AVATAR_ID_PREFIX)) return null;
  const hex = assetId.slice(AGENT_AVATAR_ID_PREFIX.length);
  if (hex.length !== AGENT_AVATAR_HEX_LENGTH || !/^[0-9a-f]+$/.test(hex)) return null;
  return { assetId, url: value };
}
