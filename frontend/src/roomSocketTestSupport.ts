export class FakeWebSocket {
  readyState: number = WebSocket.CONNECTING;
  sent: Array<Record<string, unknown>> = [];
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;

  send(raw: string) {
    this.sent.push(JSON.parse(raw) as Record<string, unknown>);
  }

  open() {
    this.readyState = WebSocket.OPEN;
    this.onopen?.(new Event("open"));
  }

  receive(message: Record<string, unknown>) {
    if (this.readyState === WebSocket.CLOSED) return;
    this.onmessage?.({ data: JSON.stringify(message) } as MessageEvent);
  }

  receiveRaw(message: string) {
    if (this.readyState === WebSocket.CLOSED) return;
    this.onmessage?.({ data: message } as MessageEvent);
  }

  close() {
    if (this.readyState === WebSocket.CLOSED) return;
    this.readyState = WebSocket.CLOSED;
    this.onclose?.({} as CloseEvent);
  }
}

export async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}
