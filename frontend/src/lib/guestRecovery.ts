export type GuestRecoveryRequest = {
  recoveryCode: string;
  roomId: string;
};

function cleanRecoveryCode(value: string): string {
  // The server owns credential syntax. Preserve opaque, case-sensitive codes;
  // this parser only recognizes the recovery UI entrance and bounds URL input.
  const code = value.trim();
  return code.length <= 256 ? code : "";
}

export function guestRecoveryRequestFromUrl(
  url = window.location.href
): GuestRecoveryRequest | null {
  try {
    const parsed = new URL(url);
    const fragment = new URLSearchParams(parsed.hash.replace(/^#/, ""));
    const recoveryCode = cleanRecoveryCode(fragment.get("recovery") || "");
    const roomId = String(parsed.searchParams.get("room") || "").trim().slice(0, 128);
    if (parsed.searchParams.get("recover") !== "1" || !recoveryCode || !roomId) {
      return null;
    }

    return { recoveryCode, roomId };
  } catch {
    return null;
  }
}

export function consumeGuestRecoveryRequestFromUrl(
  url = window.location.href
): GuestRecoveryRequest | null {
  const request = guestRecoveryRequestFromUrl(url);
  if (!request) return null;
  const parsed = new URL(url);
  parsed.hash = "";
  parsed.searchParams.delete("recover");
  parsed.searchParams.delete("room");
  window.history.replaceState({}, "", `${parsed.pathname}${parsed.search}` || "/");
  return request;
}
