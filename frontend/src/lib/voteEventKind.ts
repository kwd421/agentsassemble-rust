export const VOTE_TRANSITION_KINDS = [
  "vote_cast",
  "vote_withdraw",
  "vote_close",
] as const;

export type VoteTransitionKind = (typeof VOTE_TRANSITION_KINDS)[number];

export function isVoteTransitionKind(value: string): value is VoteTransitionKind {
  return (VOTE_TRANSITION_KINDS as readonly string[]).includes(value);
}
