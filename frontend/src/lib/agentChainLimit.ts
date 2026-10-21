/** AI messages in a row free access allows before a person speaks; 0 = no limit. */
export const AGENT_CHAIN_LIMITS = [12, 50, 100, 0] as const;

/** A host may offer other lengths later; any whole count reads. */
export function agentChainLimitIsValid(value: unknown): value is number {
  return Number.isSafeInteger(value) && Number(value) >= 0;
}
