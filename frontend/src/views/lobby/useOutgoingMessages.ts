import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import {
  createMessageAttachmentUploadClient,
  UploadFailure,
  type MessageAttachmentUploadClient,
} from "../../api/messageAttachmentTransfers";
import type { MessageAttachmentAuthority } from "../../api/messageAttachments";
import type { LobbyEvent } from "../../api";
import {
  OutgoingMessages,
  type OutgoingAuthority,
  type OutgoingDraft,
  type OutgoingMessage,
} from "../../lib/outgoingMessages";
import { roomMessagePayload } from "../../lib/roomMessagePayload";
import { RoomSocketSayError, type RoomSocketHandle } from "../../roomSocketTypes";
import type { RoomEvent } from "../../types/generated/RoomEvent";

// One owner per room incarnation and viewer. It outlives the view so that files keep
// uploading while the person looks at another room; publication needs this room's
// socket, so a message that finishes while the room is closed waits as a retryable
// failure until the person comes back.
type Owner = {
  model: OutgoingMessages;
  socket: RoomSocketHandle | null;
  identity: string;
  generation: number;
  authority?: OutgoingAuthority;
  uploads?: MessageAttachmentUploadClient;
  snapshot: readonly OutgoingMessage[];
};
const owners = new Map<string, Owner>();
const NONE: readonly OutgoingMessage[] = Object.freeze([]);

function ownerFor(key: string, hostId: string, personId: string): Owner {
  const existing = owners.get(key);
  if (existing) return existing;
  const owner: Owner = {
    socket: null, identity: "", generation: 0, snapshot: NONE,
    model: new OutgoingMessages({
      hostId, personId,
      publish: (_authority, request) => owner.socket?.ready()
        ? owner.socket.command("message.send", roomMessagePayload(request))
        : Promise.reject(new RoomSocketSayError("방 연결이 준비되지 않았어요. 연결된 뒤 다시 보내 주세요.", "socket_closed")),
    }),
  };
  owner.model.subscribe(() => { owner.snapshot = owner.model.snapshot(); });
  owners.set(key, owner);
  return owner;
}

export function uploadFailureMessage(error: unknown, fileLimit?: number): string {
  if (!(error instanceof UploadFailure)) return error instanceof Error ? error.message : "파일을 올리지 못했어요.";
  if (error.code === "file_size_limit") {
    return fileLimit ? `파일은 최대 ${Math.floor(fileLimit / (1024 * 1024))}MB까지 올릴 수 있어요.` : "파일이 너무 커요.";
  }
  if (error.code === "room_attachment_quota_reached" || error.code === "attachment_quota_reached") return "이 방의 저장 공간이 가득 찼어요.";
  if (error.code === "attachment_type_unsupported") return "올릴 수 없는 파일 형식이에요.";
  return "파일을 올리지 못했어요.";
}

export type OutgoingController = {
  /** False until this room's upload client is known; attaching waits for it. */
  ready: boolean;
  limit: number;
  /** Empty when the file can be staged, otherwise the reason to show. */
  check(file: File): string;
  send(draft: OutgoingDraft): void;
  messages: readonly OutgoingMessage[];
  failureText(message: OutgoingMessage): string;
  cancel(id: string): void;
  retry(id: string): void;
  discard(id: string): void;
};

export function useOutgoingMessages({ roomUid, roomId, participantId, authority, socket, events }: {
  roomUid: string;
  roomId: string;
  participantId: string;
  authority: MessageAttachmentAuthority | undefined;
  socket: RoomSocketHandle | null;
  events: readonly LobbyEvent[];
}): OutgoingController {
  const key = `${roomUid || roomId}\n${participantId}`;
  const owner = useMemo(() => ownerFor(key, roomUid || roomId, participantId), [key, roomUid, roomId, participantId]);
  const [uploads, setUploads] = useState<MessageAttachmentUploadClient | undefined>(owner.uploads);

  useEffect(() => {
    owner.socket = socket;
    return () => { if (owner.socket === socket) owner.socket = null; };
  }, [owner, socket]);

  const identity = authority && participantId ? JSON.stringify([roomId, participantId, authority]) : "";
  useEffect(() => {
    setUploads(owner.identity === identity ? owner.uploads : undefined);
    if (!identity || !authority) return undefined;
    if (owner.identity === identity && owner.uploads) return undefined;
    const controller = new AbortController();
    void createMessageAttachmentUploadClient(roomId, authority, controller.signal).then((client) => {
      if (controller.signal.aborted) return;
      if (owner.identity !== identity) { owner.identity = identity; owner.generation += 1; }
      owner.authority = { roomId, participantId, generation: owner.generation };
      owner.uploads = client;
      owner.model.admit(owner.authority, client);
      setUploads(client);
    }).catch(() => {
      // Attaching stays unavailable; text messages do not depend on this.
    });
    return () => controller.abort();
    // `authority` is covered by `identity`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [owner, identity, roomId, participantId]);

  const all = useSyncExternalStore(
    useCallback((listener: () => void) => owner.model.subscribe(listener), [owner]),
    () => owner.snapshot,
  );

  // Delivery resolves an uncertain publication, and lets the row give way to the real message.
  const delivered = useMemo(() => {
    const ids = new Set<string>();
    for (const event of events) {
      ids.add(event.id);
      if (event.record_id) ids.add(event.record_id);
      for (const attachment of event.attachments || []) ids.add(attachment.id);
    }
    return ids;
  }, [events]);
  // Observing notifies the model's listeners, so it must not run on every snapshot:
  // only when the room's events change or the set of unresolved messages does.
  const unresolved = all.filter((message) => message.phase === "outcome_unknown" || message.phase === "sending")
    .map((message) => `${message.id}:${message.phase}`).join(",");
  useEffect(() => {
    if (!owner.authority || !unresolved) return;
    owner.model.observe(owner.authority, events.filter((event) => event.attachments?.length).map((event) => ({
      id: event.record_id || event.id, room_id: roomId, type: "message_final",
      actor: { participant_id: event.actor_id }, attachments: event.attachments,
    }) as unknown as RoomEvent));
  }, [owner, unresolved, events, roomId]);
  useEffect(() => {
    for (const message of all) {
      const shown = message.files.some((file) => file.attachment && delivered.has(file.attachment.id)) ||
        (message.eventId ? delivered.has(message.eventId) : false);
      if (message.phase === "sent" && shown) void owner.model.discard(message.id);
    }
  }, [owner, all, delivered]);

  const messages = useMemo(
    () => all.filter((message) => message.authority.roomId === roomId && message.phase !== "cancelled" &&
      !(message.phase === "sent" && message.files.some((file) => file.attachment && delivered.has(file.attachment.id)))),
    [all, roomId, delivered],
  );

  return useMemo<OutgoingController>(() => ({
    ready: Boolean(uploads),
    limit: uploads?.limit ?? 8,
    check: (file) => {
      if (!uploads) return "잠시 후 다시 시도해 주세요.";
      if (!Number.isSafeInteger(file.size) || file.size < 1) return "빈 파일은 올릴 수 없어요.";
      try { uploads.preflight(file); return ""; }
      catch (error) { return uploadFailureMessage(error, uploads.fileLimit); }
    },
    send: (draft) => {
      if (!owner.authority) throw new Error("잠시 후 다시 시도해 주세요.");
      try { owner.model.send(owner.authority, draft); }
      catch (error) { throw new Error(uploadFailureMessage(error, uploads?.fileLimit)); }
    },
    messages,
    failureText: (message) => message.phase === "outcome_unknown" ? ""
      : uploadFailureMessage(new UploadFailure(message.failure?.code || "", false), uploads?.fileLimit),
    cancel: (id) => { void owner.model.cancel(id).then(() => owner.model.discard(id)); },
    retry: (id) => { owner.model.retry(id); },
    discard: (id) => { void owner.model.discard(id); },
  }), [owner, uploads, messages]);
}
