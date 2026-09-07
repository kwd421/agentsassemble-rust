export const GUEST_SESSION_EXPIRED_MESSAGE =
  "방 접속이 만료되거나 해제됐어요. 호스트에서 새 접속 링크를 받아 주세요.";

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
