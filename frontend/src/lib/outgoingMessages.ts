import { UploadFailure, validateTransferProgress, type MessageAttachmentUploadClient, type TransferStatus } from "../api/messageAttachmentTransfers";
import type { LobbyAttachmentRef } from "../api/messageAttachments";
import { RoomSocketSayError, type RoomCommandAck, type RoomSayRequest } from "../roomSocketTypes";
import type { RoomEvent } from "../types/generated/RoomEvent";
import { createSecureRequestId } from "./secureRequestId";

export type OutgoingAuthority = Readonly<{ roomId: string; participantId: string; generation: number }>;
export type OutgoingDraft = Readonly<{
  text: string; replyToEventId?: string; files: readonly File[];
  /** Everything else of the publication (for example a vote's question and options). */
  request?: Readonly<Omit<RoomSayRequest, "message" | "attachments" | "replyToEventId">>;
}>;
export type OutgoingPhase = "queued" | "admitting" | "deferred" | "transferring" | "validating" | "ready" | "sending" | "sent" | "cancelling" | "cancelled" | "failed" | "outcome_unknown";
export type OutgoingFailure = Readonly<{ code: string; retryable: boolean }>;
export type OutgoingFile = Readonly<{
  file: File; phase: OutgoingPhase; sentBytes: number; acceptedBytes: number;
  totalBytes: number; attachment?: LobbyAttachmentRef; failure?: OutgoingFailure;
}>;
export type OutgoingMessage = Readonly<{
  id: string; hostId: string; personId: string; authority: OutgoingAuthority;
  text: string; replyToEventId?: string; phase: OutgoingPhase; files: readonly OutgoingFile[];
  sentBytes: number; acceptedBytes: number; totalBytes: number;
  failure?: OutgoingFailure; eventId?: string;
}>;
export interface OutgoingClock {
  now(): number;
  setTimeout(callback: () => void, ms: number): unknown;
  clearTimeout(timer: unknown): void;
}
export type OutgoingOptions = {
  hostId: string; personId: string;
  publish(authority: OutgoingAuthority, request: RoomSayRequest): Promise<RoomCommandAck>;
  clock?: OutgoingClock;
};
type Job = { abort: AbortController; done: Promise<void> };
type FileState = {
  file: File; phase: OutgoingPhase; sentBytes: number; acceptedBytes: number;
  attachment?: LobbyAttachmentRef; failure?: OutgoingFailure;
  startedAt?: number; nonProgress?: number; transfer?: TransferStatus; operation: string; notBefore: number; job?: Job;
};
type MessageState = {
  id: string; authority: OutgoingAuthority; text: string; replyToEventId?: string;
  request?: OutgoingDraft["request"];
  files: FileState[]; phase: OutgoingPhase; failure?: OutgoingFailure; eventId?: string;
  retryPublication?: () => Promise<RoomCommandAck>;
  publicationStarted?: boolean;
};
const defaultClock: OutgoingClock = {
  now: () => Date.now(), setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: timer => clearTimeout(timer as ReturnType<typeof setTimeout>),
};
function failure(error: unknown): OutgoingFailure {
  if (error instanceof UploadFailure) return { code: error.code, retryable: error.retryable && error.code !== "upload_missing" };
  if (error instanceof RoomSocketSayError) return {
    code: error.category,
    retryable: ["muted", "socket_closed", "command_timeout", "capacity", "persistence_failed"].includes(error.category),
  };
  return { code: "upload_interrupted", retryable: true };
}
// One data request carries many pieces: a request per piece would cost a round trip
// (and an upload grant) per 64 KiB and waste the transport's window. About twenty
// requests per file keeps progress visible; the bounds are 4 and 64 pieces.
function segmentBytes(size: number, piece: number) {
  return Math.min(64, Math.max(4, Math.ceil(size / 20 / piece))) * piece;
}
function sameAuthority(a: OutgoingAuthority, b: OutgoingAuthority) {
  return a.roomId === b.roomId && a.participantId === b.participantId && a.generation === b.generation;
}

/** One instance per host/person, shared by all that person's rooms and drafts. */
export class OutgoingMessages {
  private rooms = new Map<string, { authority: OutgoingAuthority; uploads: MessageAttachmentUploadClient }>();
  private messages = new Map<string, MessageState>();
  private listeners = new Set<() => void>();
  private active = 0;
  private legacyActive = 0;
  private clock: OutgoingClock;
  private timer?: unknown;
  private closed = false;
  constructor(private readonly options: OutgoingOptions) { this.clock = options.clock ?? defaultClock; }

  admit(authority: OutgoingAuthority, uploads: MessageAttachmentUploadClient): void {
    if (this.closed) throw new Error("outgoing_closed");
    const old = this.rooms.get(authority.roomId);
    if (old && sameAuthority(old.authority, authority)) return;
    if (old && old.authority.participantId !== authority.participantId) this.leaveRoom(authority.roomId);
    this.rooms.set(authority.roomId, { authority: Object.freeze({ ...authority }), uploads });
    for (const message of this.messages.values()) {
      if (message.authority.roomId !== authority.roomId || ["sent", "cancelled", "cancelling"].includes(message.phase)) continue;
      message.authority = Object.freeze({ ...authority });
      // Dispatched publication is never repeated by admission, even if its old ACK is lost.
      if (message.phase === "sending" || message.phase === "outcome_unknown") {
        message.phase = "outcome_unknown"; message.retryPublication = undefined;
        message.failure = { code: "outcome_unknown", retryable: true };
        continue;
      }
      for (const file of message.files) {
        file.job?.abort.abort();
        if (file.attachment) continue;
        file.startedAt = undefined; file.nonProgress = 0; file.transfer = undefined; file.operation = createSecureRequestId();
        file.sentBytes = file.acceptedBytes = file.notBefore = 0;
        if (message.phase !== "failed") file.phase = "queued";
      }
    }
    this.changed();
  }

  send(authority: OutgoingAuthority, draft: OutgoingDraft): string {
    const room = this.rooms.get(authority.roomId);
    if (this.closed || !room || !sameAuthority(room.authority, authority)) throw new Error("outgoing_authority_retired");
    if (!draft.files.length || draft.files.length > room.uploads.limit) throw new UploadFailure("attachment_count", false);
    draft.files.forEach(file => room.uploads.preflight(file));
    const id = createSecureRequestId();
    this.messages.set(id, {
      id, authority: room.authority, text: draft.text, replyToEventId: draft.replyToEventId, request: draft.request, phase: "queued",
      files: draft.files.map(file => ({ file, phase: "queued", sentBytes: 0, acceptedBytes: 0, operation: createSecureRequestId(), notBefore: 0 })),
    });
    this.changed();
    return id;
  }

  snapshot(): readonly OutgoingMessage[] {
    return Object.freeze([...this.messages.values()].map(message => {
      const files = Object.freeze(message.files.map(file => Object.freeze({
        file: file.file, phase: file.phase, sentBytes: file.sentBytes, acceptedBytes: file.acceptedBytes,
        totalBytes: file.file.size, attachment: file.attachment, failure: file.failure && Object.freeze({ ...file.failure }),
      })));
      return Object.freeze({
        id: message.id, hostId: this.options.hostId, personId: this.options.personId,
        authority: message.authority, text: message.text, replyToEventId: message.replyToEventId,
        phase: message.phase, files, failure: message.failure && Object.freeze({ ...message.failure }), eventId: message.eventId,
        sentBytes: files.reduce((sum, file) => sum + file.sentBytes, 0),
        acceptedBytes: files.reduce((sum, file) => sum + file.acceptedBytes, 0),
        totalBytes: files.reduce((sum, file) => sum + file.totalBytes, 0),
      });
    }));
  }
  subscribe(listener: () => void): () => void { this.listeners.add(listener); return () => { this.listeners.delete(listener); }; }

  async cancel(id: string): Promise<boolean> {
    const message = this.messages.get(id);
    if (!message || ["sending", "sent", "outcome_unknown", "cancelling", "cancelled"].includes(message.phase)) return false;
    message.phase = "cancelling";
    const uploads = this.rooms.get(message.authority.roomId)?.uploads;
    const jobs: Promise<void>[] = [];
    for (const file of message.files) {
      if (file.job) { file.job.abort.abort(); jobs.push(file.job.done); }
      else if (file.transfer && !file.attachment && uploads) {
        jobs.push(this.retire(file.transfer, uploads)); file.transfer = undefined;
      }
      if (!file.attachment) file.phase = "cancelling";
    }
    this.changed();
    try { await Promise.all(jobs); }
    catch (error) { message.failure = failure(error); }
    if (this.messages.get(id) === message) {
      message.phase = "cancelled";
      for (const file of message.files) if (!file.attachment) { file.phase = "cancelled"; file.transfer = undefined; }
      this.changed();
    }
    return true;
  }

  retry(id: string): boolean {
    const message = this.messages.get(id);
    if (!message || !this.current(message) || !(message.phase === "cancelled" && message.failure?.retryable !== false || message.phase === "outcome_unknown" || message.phase === "failed" && message.failure?.retryable)) return false;
    const cancelled = message.phase === "cancelled";
    message.phase = "queued"; message.failure = undefined;
    for (const file of message.files) {
      if (file.attachment || file.job) continue;
      file.phase = "queued"; file.failure = undefined; file.notBefore = 0;
      file.startedAt = undefined; file.nonProgress = 0;
      if (!file.transfer) {
        if (cancelled) file.operation = createSecureRequestId();
        file.sentBytes = file.acceptedBytes = 0;
      }
    }
    this.changed(); return true;
  }

  async discard(id: string): Promise<boolean> {
    const message = this.messages.get(id);
    if (!message || message.phase === "sending") return false;
    if (!["sent", "cancelled", "outcome_unknown"].includes(message.phase)) await this.cancel(id);
    this.messages.delete(id); this.changed(); return true;
  }

  observe(authority: OutgoingAuthority, events: readonly RoomEvent[]): void {
    for (const message of this.messages.values()) {
      if (!sameAuthority(message.authority, authority) || !this.current(message) ||
          !message.publicationStarted || ["cancelling", "cancelled"].includes(message.phase)) continue;
      const ids = message.files.map(file => file.attachment?.id);
      const event = events.find(event => event.room_id === authority.roomId && event.type === "message_final" &&
        event.actor.participant_id === authority.participantId && Array.isArray(event.attachments) &&
        event.attachments.length === ids.length && event.attachments.every((attachment, index) =>
          attachment !== null && typeof attachment === "object" && !Array.isArray(attachment) && attachment.id === ids[index]));
      if (event) { message.phase = "sent"; message.eventId = event.id; message.failure = undefined; message.retryPublication = undefined; }
    }
    this.changed();
  }

  leaveRoom(roomId: string): void {
    for (const message of this.messages.values()) {
      if (message.authority.roomId !== roomId) continue;
      void this.cancel(message.id);
      this.messages.delete(message.id);
    }
    this.rooms.delete(roomId); this.changed();
  }
  close(): void {
    for (const room of this.rooms.keys()) this.leaveRoom(room);
    this.closed = true; this.clock.clearTimeout(this.timer); this.listeners.clear();
  }

  private current(message: MessageState): boolean {
    const room = this.rooms.get(message.authority.roomId);
    return !this.closed && this.messages.get(message.id) === message && !!room && sameAuthority(room.authority, message.authority);
  }
  private changed() {
    for (const listener of this.listeners) listener();
    queueMicrotask(() => this.pump());
  }
  private pump() {
    this.clock.clearTimeout(this.timer); this.timer = undefined;
    let wake = Infinity;
    for (const message of this.messages.values()) {
      if (!this.current(message) || ["failed", "cancelling", "cancelled", "sending", "sent", "outcome_unknown"].includes(message.phase)) continue;
      if (message.files.every(file => !!file.attachment)) { void this.publish(message); continue; }
      for (const file of message.files) {
        if (file.attachment || file.job || !["queued", "deferred"].includes(file.phase)) continue;
        if (file.notBefore > this.clock.now()) { wake = Math.min(wake, file.notBefore); continue; }
        const uploads = this.rooms.get(message.authority.roomId)!.uploads;
        // An older host takes whole files against a two-per-person budget sized by the
        // file, so those go one at a time as they always did.
        const legacy = !uploads.transfer;
        if (this.active === 3 || legacy && this.legacyActive === 1) continue;
        const authority = message.authority;
        const abort = new AbortController();
        this.active++; if (legacy) this.legacyActive++;
        // Start in a microtask so the job owns its cancellation before any network work.
        const done = Promise.resolve().then(() => this.upload(message, file, uploads, authority, abort)).finally(() => {
          file.job = undefined; this.active--; if (legacy) this.legacyActive--; this.changed();
        });
        file.job = { abort, done };
      }
    }
    if (wake !== Infinity) this.timer = this.clock.setTimeout(() => this.pump(), wake - this.clock.now());
  }
  private async retire(transfer: TransferStatus, uploads: MessageAttachmentUploadClient) {
    if (transfer.attachment) return;
    try { await uploads.cancel(transfer.handle); }
    catch (error) {
      // Finished/expired transfers need no deletion. Other cleanup failures remain visible.
      if (!(error instanceof UploadFailure && ["upload_missing", "upload_already_finished"].includes(error.code))) throw error;
    }
  }
  private async upload(message: MessageState, file: FileState, uploads: MessageAttachmentUploadClient, authority: OutgoingAuthority, abort: AbortController) {
    const signal = abort.signal;
    file.startedAt ??= this.clock.now();
    let deadline: unknown;
    const stalled = () => new UploadFailure("upload_stalled", true);
    const expiry = new Promise<never>((_, reject) => {
      deadline = this.clock.setTimeout(() => { const error = stalled(); reject(error); abort.abort(error); },
        Math.max(0, file.startedAt! + 10 * 60 * 1000 - this.clock.now()));
    });
    const response = <T>(pending: Promise<T>) => Promise.race([pending, expiry]);
    const progress = (next: TransferStatus, lower: number, upper: number) => {
      const checked = validateTransferProgress(next, transfer, file.file.size, lower, upper);
      if (!valid()) return checked;
      file.nonProgress = checked.accepted_offset > (transfer?.accepted_offset ?? 0) || checked.attachment ? 0 : (file.nonProgress ?? 0) + 1;
      if (file.nonProgress >= 8 || this.clock.now() - file.startedAt! >= 10 * 60 * 1000) throw stalled();
      file.transfer = checked;
      return checked;
    };
    let transfer = file.transfer;
    const valid = () => !signal.aborted && this.current(message) && sameAuthority(message.authority, authority);
    const update = (phase: OutgoingPhase) => {
      file.phase = phase;
      if (message.phase !== "failed") message.phase = phase;
      this.changed();
    };
    try {
      if (!valid()) return;
      update("admitting");
      if (!uploads.transfer) {
        update("transferring"); file.sentBytes = file.file.size;
        const attachment = await response(uploads.legacy(file.file, signal));
        if (valid()) { file.attachment = Object.freeze({ ...attachment }); file.acceptedBytes = file.file.size; update("ready"); }
        return;
      }
      transfer = transfer
        ? progress(await response(uploads.status(transfer.handle, signal)), transfer.accepted_offset, file.sentBytes)
        : progress(await response(uploads.begin(file.file, file.operation, signal)), 0, 0);
      let recoveredOffset: number | undefined;
      while (valid()) {
        file.transfer = transfer; file.acceptedBytes = transfer.accepted_offset;
        file.sentBytes = Math.max(file.sentBytes, file.acceptedBytes);
        if (transfer.attachment) { file.attachment = Object.freeze({ ...transfer.attachment }); update("ready"); return; }
        const offset = transfer.accepted_offset;
        if (offset === file.file.size) {
          update("validating");
          try { transfer = progress(await response(uploads.finish(transfer.handle, signal)), offset, offset); }
          catch (error) {
            if (error instanceof UploadFailure && (!error.retryable || ["upload_request_busy", "upload_stalled"].includes(error.code))) throw error;
            transfer = progress(await response(uploads.status(transfer.handle, signal)), offset, offset);
            if (!transfer.attachment) throw error;
          }
        } else {
          update("transferring");
          const body = file.file.slice(offset, Math.min(file.file.size, offset + segmentBytes(file.file.size, transfer.piece_size)));
          file.sentBytes = Math.max(file.sentBytes, offset + body.size); this.changed();
          try { transfer = progress(await response(uploads.data(transfer.handle, offset, body, signal)), offset, offset + body.size); }
          catch (error) {
            if (recoveredOffset === offset || error instanceof UploadFailure && (!error.retryable || ["upload_request_busy", "upload_stalled"].includes(error.code))) throw error;
            recoveredOffset = offset;
            transfer = progress(await response(uploads.status(transfer.handle, signal)), offset, offset + body.size);
          }
        }
      }
    } catch (error) {
      if (!this.current(message) || !sameAuthority(message.authority, authority) || signal.aborted && signal.reason?.code !== "upload_stalled") return;
      if (error instanceof UploadFailure && ["upload_deferred", "upload_request_busy"].includes(error.code)) {
        file.nonProgress = (file.nonProgress ?? 0) + 1;
        if (file.nonProgress < 8 && this.clock.now() - file.startedAt! < 10 * 60 * 1000) {
          file.notBefore = this.clock.now() + (error.retryAfterMs ?? 250); update("deferred");
          return;
        }
        error = stalled();
      }
      file.failure = failure(error); file.phase = "failed";
      if (!message.failure || message.failure.retryable) message.failure = file.failure; message.phase = "failed"; this.changed();
    } finally {
      this.clock.clearTimeout(deadline);
      if (!valid() && transfer) {
        if (transfer.attachment && this.current(message) && sameAuthority(message.authority, authority) && message.phase === "cancelling") {
          file.attachment = Object.freeze({ ...transfer.attachment }); file.acceptedBytes = file.file.size; file.phase = "ready";
        }
        try {
          await this.retire(transfer, uploads);
          if (signal.reason?.code === "upload_stalled" && this.current(message) && sameAuthority(message.authority, authority)) {
            file.transfer = undefined; file.operation = createSecureRequestId();
          }
        }
        catch (error) {
          if (this.messages.get(message.id) === message && sameAuthority(message.authority, authority)) {
            message.failure = failure(error); this.changed();
          }
        }
      }
    }
  }
  private async publish(message: MessageState) {
    message.phase = "ready"; this.changed();
    if (!this.current(message) || message.phase !== "ready") return;
    const authority = message.authority;
    message.phase = "sending"; message.publicationStarted = true;
    try {
      const pending = message.retryPublication ? message.retryPublication() : this.options.publish(authority, {
        ...message.request, message: message.text, replyToEventId: message.replyToEventId,
        attachments: message.files.map(file => file.attachment!),
      });
      this.changed();
      const ack = await pending;
      if (!this.current(message) || !sameAuthority(message.authority, authority) || (message.phase as OutgoingPhase) === "sent") return;
      // ACK commits publication; keep the row until delivery supplies the real message.
      const event = ack.result?.event as { id: string } | undefined;
      message.eventId = event?.id;
      message.phase = "sent"; message.failure = undefined; message.retryPublication = undefined;
    } catch (error) {
      if (!this.current(message) || !sameAuthority(message.authority, authority) || (message.phase as OutgoingPhase) === "sent") return;
      if (!(error instanceof RoomSocketSayError) || error.category === "outcome_unknown") {
        message.phase = "outcome_unknown"; message.failure = { code: "outcome_unknown", retryable: true };
        message.retryPublication = error instanceof RoomSocketSayError ? error.retry : undefined;
      } else { message.phase = "failed"; message.failure = failure(error); }
    }
    this.changed();
  }
}
