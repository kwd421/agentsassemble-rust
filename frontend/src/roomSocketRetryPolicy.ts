const UNCERTAIN_RETRY_BASE_MS = 500;
const UNCERTAIN_RETRY_MAX_MS = 30_000;
const UNCERTAIN_ATTEMPT_LIMIT = 8;

export interface UncertainCommandRetryState {
  retryAttempt: number;
  retryCountedGeneration: number;
  retryNotBefore: number;
}

export interface PendingCommandRetryState extends UncertainCommandRetryState {
  timerId: number | null;
  retryTimerId: number | null;
  everSent: boolean;
}

export type UncertainRetryDecision = "already_counted" | "retry" | "exhausted";

function uncertainCommandRetryDelay(attemptCount: number): number | null {
  if (attemptCount >= UNCERTAIN_ATTEMPT_LIMIT) return null;
  return Math.min(
    UNCERTAIN_RETRY_MAX_MS,
    UNCERTAIN_RETRY_BASE_MS * 2 ** Math.min(attemptCount - 1, 6)
  );
}

export function scheduleUncertainCommandRetry(
  command: UncertainCommandRetryState,
  generation: number
): UncertainRetryDecision {
  if (command.retryCountedGeneration === generation) return "already_counted";
  command.retryAttempt += 1;
  command.retryCountedGeneration = generation;
  const retryDelay = uncertainCommandRetryDelay(command.retryAttempt);
  if (retryDelay !== null) command.retryNotBefore = Date.now() + retryDelay;
  return retryDelay === null ? "exhausted" : "retry";
}
