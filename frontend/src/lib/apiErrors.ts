export const GUEST_SESSION_EXPIRED_MESSAGE =
  "Guest session expired or was revoked. Ask the host for a new invite.";

export class ApiError extends Error {
  status: number;
  code: string;

  constructor(status: number, message: string, code = "") {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

export function isUnauthorizedApiError(error: unknown): boolean {
  return error instanceof ApiError && error.status === 401;
}
