export const GUEST_SESSION_EXPIRED_MESSAGE =
  "Guest session expired or was revoked. Ask the host for a new invite.";

export class ApiError extends Error {
  status: number;
  code: string;
  resolution?: "rejected" | "unresolved";

  constructor(status: number, message: string, code = "", resolution?: "rejected" | "unresolved") {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.resolution = resolution;
  }
}

export function isUnauthorizedApiError(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}
