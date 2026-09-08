import type { PendingProviderRequest } from "../types/generated/PendingProviderRequest";

export const pendingRequest: PendingProviderRequest = {
  session_id: "session", state: "open", expires_at: "2026-09-08T23:00:00Z",
  request: { provider_request_id: "00000000-0000-4000-8000-000000000001", request_kind: "user_input", title: "Access answer", description: "Enter answer", timeout_seconds: 600,
    prompt: { response_kind: "answers", questions: [{ id: "secret", header: "Access", question: "Answer", is_secret: true, multiple: false, is_other: true, options: [] }] } },
};
