// One channel-direction window; FIFO waiters share capacity across bodies.
export const BODY_WINDOW = "x-agentsassemble-body-window";
export const BODY_WINDOW_CONFIRMED = "x-agentsassemble-body-window-confirmed";
export const WINDOW_BYTES = 384 * 1024;
export class CreditWindow {
  private used = 0;
  private sent = new Map<number, { sent: number; emitted: number; consumed: number; boundaries: { offset: number; at: number }[] }>();
  private waiting: { id: number; bytes: number; resolve: (accepted: boolean) => void }[] = [];
  private timer?: ReturnType<typeof setTimeout>;
  private timeout: () => void;
  constructor(timeout: () => void) { this.timeout = timeout; }
  async reserve(id: number, bytes: number): Promise<boolean> {
    if (!this.sent.has(id)) this.sent.set(id, { sent: 0, emitted: 0, consumed: 0, boundaries: [] });
    const accepted = await new Promise<boolean>(resolve => { this.waiting.push({ id, bytes, resolve }); this.drain(); });
    return accepted;
  }
  emitted(id: number, offset: number) {
    const body = this.sent.get(id);
    if (body && offset > body.emitted) {
      body.emitted = offset; body.boundaries.push({ offset, at: performance.now() }); this.clock();
    }
  }
  credit(id: number, offset: number) {
    const body = this.sent.get(id);
    if (!body || !Number.isSafeInteger(offset) || offset <= body.consumed || offset > body.emitted || !body.boundaries.some(end => end.offset === offset)) throw new Error("Invalid body credit");
    this.used -= offset - body.consumed; body.consumed = offset; body.boundaries = body.boundaries.filter(end => end.offset > offset);
    this.clock(); this.drain();
  }
  release(id: number) {
    const body = this.sent.get(id);
    if (body) this.used -= body.sent - body.consumed;
    this.sent.delete(id);
    this.clock();
    this.waiting = this.waiting.filter(wait => { if (wait.id !== id) return true; wait.resolve(false); return false; });
    this.drain();
  }
  close() {
    clearTimeout(this.timer); this.timer = undefined;
    for (const wait of this.waiting.splice(0)) wait.resolve(false);
    this.sent.clear(); this.used = 0;
  }
  private drain() {
    for (;;) {
      // Keep one piece available to other consumers even if a body stops pulling.
      const index = this.waiting.findIndex(wait => {
        const body = this.sent.get(wait.id)!;
        return this.used + wait.bytes <= WINDOW_BYTES && body.sent - body.consumed + wait.bytes <= WINDOW_BYTES - 64 * 1024;
      });
      if (index < 0) return;
      const [wait] = this.waiting.splice(index, 1);
      this.used += wait.bytes; this.sent.get(wait.id)!.sent += wait.bytes;
      wait.resolve(true);
    }
  }
  private clock() {
    clearTimeout(this.timer);
    const oldest = Math.min(...[...this.sent.values()].map(body => body.boundaries[0]?.at ?? Infinity));
    this.timer = oldest === Infinity ? undefined : setTimeout(this.timeout, Math.max(0, oldest + 30_000 - performance.now()));
  }
}
