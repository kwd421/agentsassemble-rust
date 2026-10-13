export class UploadFailure extends Error {
  constructor(readonly code: string, readonly retryable: boolean, readonly retryAfterMs?: number) {
    super(code);
    if (retryAfterMs !== undefined) this.retryAfterMs = Math.min(5000, Math.max(1, retryAfterMs));
  }
}
