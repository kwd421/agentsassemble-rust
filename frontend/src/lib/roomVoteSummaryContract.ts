export interface VoteSummary {
  vote_id: string;
  question: string;
  options: string[];
  vote_duration_seconds: number;
  vote_deadline_at: string;
  created_by: string;
  created_at: string;
  tallies: Record<string, number>;
  own_choice: string;
  total_votes: number;
  closed: boolean;
  closed_at: string;
  close_reason: "deadline" | "manual" | "";
}

const SUMMARY_KEYS = new Set([
  "vote_id",
  "question",
  "options",
  "vote_duration_seconds",
  "vote_deadline_at",
  "created_by",
  "created_at",
  "tallies",
  "own_choice",
  "total_votes",
  "closed",
  "closed_at",
  "close_reason",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}

function hasExactKeys(value: Record<string, unknown>, keys: ReadonlySet<string>): boolean {
  const actual = Object.keys(value);
  return actual.length === keys.size && actual.every((key) => keys.has(key));
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isTimestamp(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && Number.isFinite(Date.parse(value));
}

export function voteSummaryResultIsValid(
  payload: Record<string, unknown>,
  value: unknown
): value is VoteSummary {
  if (
    Object.keys(payload).length !== 1 ||
    typeof payload.vote_id !== "string" ||
    !payload.vote_id ||
    !isRecord(value) ||
    !hasExactKeys(value, SUMMARY_KEYS) ||
    value.vote_id !== payload.vote_id ||
    typeof value.question !== "string" ||
    !value.question.trim() ||
    typeof value.created_by !== "string" ||
    !value.created_by.trim() ||
    !isTimestamp(value.created_at) ||
    !Array.isArray(value.options) ||
    !value.options.every((option) => typeof option === "string" && option.length > 0) ||
    new Set(value.options).size !== value.options.length ||
    !isCount(value.vote_duration_seconds) ||
    typeof value.vote_deadline_at !== "string" ||
    !isRecord(value.tallies) ||
    typeof value.own_choice !== "string" ||
    !isCount(value.total_votes) ||
    typeof value.closed !== "boolean" ||
    typeof value.closed_at !== "string" ||
    !["", "deadline", "manual"].includes(String(value.close_reason))
  ) {
    return false;
  }

  const optionSet = new Set(value.options);
  const tallyEntries = Object.entries(value.tallies);
  if (
    tallyEntries.length !== optionSet.size ||
    tallyEntries.some(([option, count]) => !optionSet.has(option) || !isCount(count)) ||
    (value.own_choice !== "" && !optionSet.has(value.own_choice))
  ) {
    return false;
  }
  const tallyTotal = tallyEntries.reduce((total, [, count]) => total + Number(count), 0);
  if (!Number.isSafeInteger(tallyTotal) || tallyTotal !== value.total_votes) return false;

  const duration = value.vote_duration_seconds;
  if (duration === 0) {
    if (value.vote_deadline_at !== "") return false;
  } else {
    if (!isTimestamp(value.vote_deadline_at)) return false;
    if (Date.parse(value.vote_deadline_at) - Date.parse(value.created_at) !== duration * 1000) {
      return false;
    }
  }

  if (!value.closed) return value.closed_at === "" && value.close_reason === "";
  return (
    isTimestamp(value.closed_at) &&
    (value.close_reason === "manual" ||
      (value.close_reason === "deadline" && duration > 0))
  );
}
