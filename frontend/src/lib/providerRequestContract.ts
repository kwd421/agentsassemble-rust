import type { PendingProviderRequest } from "../types/generated/PendingProviderRequest";
import type { ProviderRequest } from "../types/generated/ProviderRequest";
import type { ProviderRequestOption } from "../types/generated/ProviderRequestOption";
import type { ProviderRequestQuestion } from "../types/generated/ProviderRequestQuestion";
import { assertExactKeys, strictRecord, type ExactGeneratedKeys } from "./strictJsonContract";

const PENDING_FIELDS = ["session_id", "request", "expires_at", "state"] as const;
const PENDING_KEYS: ExactGeneratedKeys<PendingProviderRequest, typeof PENDING_FIELDS> = PENDING_FIELDS;
const REQUEST_FIELDS = ["provider_request_id", "request_kind", "title", "description", "timeout_seconds", "prompt"] as const;
const REQUEST_KEYS: ExactGeneratedKeys<ProviderRequest, typeof REQUEST_FIELDS> = REQUEST_FIELDS;
const OPTION_FIELDS = ["id", "label", "kind", "description"] as const;
const OPTION_KEYS: ExactGeneratedKeys<ProviderRequestOption, typeof OPTION_FIELDS> = OPTION_FIELDS;
const QUESTION_FIELDS = ["id", "header", "question", "options", "multiple", "is_other", "is_secret"] as const;
const QUESTION_KEYS: ExactGeneratedKeys<ProviderRequestQuestion, typeof QUESTION_FIELDS> = QUESTION_FIELDS;

export function parseProviderRequest(value: unknown): ProviderRequest {
  const request = strictRecord(value, "provider request");
  assertExactKeys(request, REQUEST_KEYS, "provider request");
  for (const key of ["provider_request_id", "title", "description"]) requireString(request[key]);
  if (!Number.isInteger(request.timeout_seconds)) throw new Error("Invalid request deadline");
  const prompt = strictRecord(request.prompt, "provider request prompt");
  switch (prompt.response_kind) {
    case "option":
      assertExactKeys(prompt, ["response_kind", "options"], "permission choices");
      if (request.request_kind !== "permission") throw new Error("Invalid request kind");
      parseOptions(prompt.options);
      break;
    case "answers":
      assertExactKeys(prompt, ["response_kind", "questions"], "provider questions");
      if (request.request_kind !== "user_input" || !Array.isArray(prompt.questions)) throw new Error("Invalid provider questions");
      for (const raw of prompt.questions) {
        const question = strictRecord(raw, "provider question");
        assertExactKeys(question, QUESTION_KEYS, "provider question");
        for (const key of ["id", "header", "question"]) requireString(question[key]);
        for (const key of ["multiple", "is_other", "is_secret"]) {
          if (typeof question[key] !== "boolean") throw new Error("Invalid question flag");
        }
        parseOptions(question.options);
      }
      break;
    case "acknowledge":
      assertExactKeys(prompt, ["response_kind", "action_url"], "external action");
      if (request.request_kind !== "external_action") throw new Error("Invalid request kind");
      if (prompt.action_url !== null) {
        requireString(prompt.action_url);
        const url = new URL(prompt.action_url as string);
        if (url.protocol !== "https:" || url.username || url.password) throw new Error("Invalid action URL");
      }
      break;
    default: throw new Error("Unsupported provider request");
  }
  return request as unknown as ProviderRequest;
}

export function pendingProviderRequestsAreValid(value: unknown): value is PendingProviderRequest[] {
  try {
    if (!Array.isArray(value)) return false;
    const ids = new Set<string>();
    for (const raw of value) {
      const pending = strictRecord(raw, "pending provider request");
      assertExactKeys(pending, PENDING_KEYS, "pending provider request");
      requireString(pending.session_id);
      requireString(pending.expires_at);
      if (!Number.isFinite(Date.parse(pending.expires_at as string)) || (pending.state !== "open" && pending.state !== "resolving")) return false;
      const request = parseProviderRequest(pending.request);
      if (ids.has(request.provider_request_id)) return false;
      ids.add(request.provider_request_id);
    }
    return true;
  } catch { return false; }
}

function parseOptions(value: unknown): void {
  if (!Array.isArray(value)) throw new Error("Invalid request options");
  for (const raw of value) {
    const option = strictRecord(raw, "provider request option");
    assertExactKeys(option, OPTION_KEYS, "provider request option");
    for (const key of OPTION_KEYS) requireString(option[key]);
  }
}

function requireString(value: unknown): void {
  if (typeof value !== "string") throw new Error("Invalid provider request text");
}
