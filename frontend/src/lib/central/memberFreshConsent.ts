/**
 * Fresh-consent bridge for member host epoch/key rotation.
 *
 * A successful re-join through a new invite mints a live admission whose
 * transport already serves the host's current epoch. Silent reconcile must
 * adopt that admission (lifting the epoch/key refusal and replacing any
 * dead registry entry) instead of parking on `refused` or
 * `transport-inactive` until the next tap. The invite entry publishes here;
 * `useMemberHosts` consumes in reconcile.
 */
export const MEMBER_FRESH_CONSENT_EVENT = "member-fresh-consent";

export type MemberFreshConsentDetail = {
  serverId: string;
  admission: unknown;
};

export function publishMemberFreshConsentAdmission(serverId: string, admission: unknown): void {
  if (!serverId || !admission) return;
  try {
    window.dispatchEvent(
      new CustomEvent<MemberFreshConsentDetail>(MEMBER_FRESH_CONSENT_EVENT, {
        detail: { serverId, admission },
      }),
    );
  } catch {
    // Publishing stays best-effort; the anchor save still drives reconcile.
  }
}
