import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
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
import { hasRemoteWorkspace, remoteSessionTransport, sessionHostServerId, subscribeRemoteWorkspace } from "../../lib/remote/remoteWorkspace";
import { roomMessagePayload } from "../../lib/roomMessagePayload";
import { RoomSocketSayError, type RoomSocketHandle } from "../../roomSocketTypes";
import type { RoomEvent } from "../../types/generated/RoomEvent";

// One owner per host and person: its scheduler shares the three upload slots across
// every room that person has open there. A room is registered under it with the exact authority
// generation it was admitted with; a publication is dispatched only through that
// generation's socket, so nothing admitted before a reconnect, a re-admission or an
// account change can speak through what replaced it. Uploads keep running while the
// person looks at another room; a message that finishes then waits as a retryable
// failure, because publishing needs the room's socket.
type RoomEntry = {
  /** Who is acting (participant and HTTP authority); a change means another person or session. */
  principal: string;
  identity: string;
  generation: number;
  socket: RoomSocketHandle | null;
  authority?: OutgoingAuthority;
  uploads?: MessageAttachmentUploadClient;
};
type Owner = {
  model: OutgoingMessages;
  rooms: Map<string, RoomEntry>;
  generations: number;
  snapshot: readonly OutgoingMessage[];
  /** The remote bearer this owner publishes through; absent for the local host. */
  serverToken?: string;
};
const owners = new Map<string, Owner>();
const NONE: readonly OutgoingMessage[] = Object.freeze([]);
const socketIds = new WeakMap<RoomSocketHandle, number>();
let nextSocketId = 0;
function socketId(socket: RoomSocketHandle | null): number {
  if (!socket) return 0;
  let id = socketIds.get(socket);
  if (!id) { id = ++nextSocketId; socketIds.set(socket, id); }
  return id;
}

function ownerFor(key: string, hostId: string, personId: string, serverToken?: string): Owner {
  const existing = owners.get(key);
  if (existing) return existing;
  const owner: Owner = {
    rooms: new Map(), generations: 0, snapshot: NONE, serverToken,
    model: new OutgoingMessages({
      hostId, personId,
      publish: (authority, request) => {
        const room = owner.rooms.get(authority.roomId);
        return room?.authority && room.generation === authority.generation && room.socket?.ready()
          ? room.socket.command("message.send", roomMessagePayload(request))
          : Promise.reject(new RoomSocketSayError("방 연결이 준비되지 않았어요. 연결된 뒤 다시 보내 주세요.", "socket_closed"));
      },
    }),
  };
  let sweeping = false;
  owner.model.subscribe(() => {
    owner.snapshot = owner.model.snapshot();
    // Rooms nobody is looking at: finished rows have no view left to clear them.
    if (sweeping) return;
    sweeping = true;
    try {
      for (const [roomId, room] of [...owner.rooms]) if (!room.socket) sweep(key, owner, roomId);
    } finally { sweeping = false; }
  });
  owners.set(key, owner);
  return owner;
}
// A room nobody is looking at, with nothing left to send, is forgotten; an owner
// without rooms is closed.
function forget(key: string, owner: Owner, roomId: string) {
  const room = owner.rooms.get(roomId);
  if (!room || room.socket) return;
  if (owner.model.snapshot().some((message) => message.authority.roomId === roomId)) return;
  owner.model.leaveRoom(roomId);
  owner.rooms.delete(roomId);
  if (owner.rooms.size === 0 && owners.get(key) === owner) { owner.model.close(); owners.delete(key); }
}
// A room without a watching view: its finished rows have nothing left to clear them.
function sweep(key: string, owner: Owner, roomId: string) {
  for (const message of owner.model.snapshot()) {
    if (message.authority.roomId === roomId && (message.phase === "sent" || message.phase === "cancelled")) {
      void owner.model.discard(message.id);
    }
  }
  forget(key, owner, roomId);
}
function leave(key: string, roomId: string) {
  const owner = owners.get(key);
  if (!owner?.rooms.has(roomId)) return;
  owner.model.leaveRoom(roomId); owner.rooms.delete(roomId);
  if (owner.rooms.size === 0) { owner.model.close(); owners.delete(key); }
}
// Ending a host's channel (sign-out, another account or server) ends only the
// queues owned by that host. Each owner is keyed by its host (local or the
// session's server_id), so installing or closing host B never touches host A.
subscribeRemoteWorkspace(() => {
  for (const [key, owner] of [...owners.entries()]) {
    // Local owners have no bearer and never close on remote changes; every
    // remote owner is bound to its own bearer, so only that host's close
    // ends its queues.
    if (!owner.serverToken) continue;
    if (!remoteSessionTransport(owner.serverToken)) { owner.model.close(); owners.delete(key); }
  }
});

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
  /** The room's lifetime: a recreated room with the same id is a different room. */
  roomUid: string;
  roomId: string;
  participantId: string;
  authority: MessageAttachmentAuthority | undefined;
  socket: RoomSocketHandle | null;
  events: readonly LobbyEvent[];
}): OutgoingController {
  // The owner is resolved from this room's own authority only: a remote room
  // uses its session's live server, never whichever host was installed last.
  // An unknown or closed session fails closed (no key, nothing sends).
  const hostId = !authority || !participantId ? ""
    : authority.kind === "remote"
      ? (sessionHostServerId(authority.sessionToken) ?? (hasRemoteWorkspace() ? "" : window.location.origin))
      : "local";
  const serverToken = authority?.kind === "remote" ? authority.sessionToken : undefined;
  if (serverToken && !remoteSessionTransport(serverToken)) {
    // Fail closed below via the empty key; the effect leaves any previous owner.
  }
  const key = hostId && (!serverToken || remoteSessionTransport(serverToken)) ? `${hostId}\n${participantId}` : "";
  const principal = hostId && authority ? JSON.stringify([roomUid, participantId, authority]) : "";
  const identity = principal ? `${principal}#${socketId(socket)}` : "";
  const [admitted, setAdmitted] = useState(0);
  const lastKey = useRef("");

  useEffect(() => {
    // Another person, another host, or no authority at all (read-only, removed, signed
    // out): nothing the previous one queued for this room may still be sent.
    if (lastKey.current && lastKey.current !== key) leave(lastKey.current, roomId);
    lastKey.current = key;
    if (!key || !identity || !authority) return undefined;
    const owner = ownerFor(key, hostId, participantId, serverToken);
    let room = owner.rooms.get(roomId);
    if (!room) { room = { principal, identity: "", generation: 0, socket: null }; owner.rooms.set(roomId, room); }
    const entry = room;
    if (entry.identity !== identity) {
      // Retire the previous generation at once, in the model too: its transfers stop and
      // nothing starts or publishes until the new admission. The same person keeps the
      // queued messages across a new socket; a different session starts clean.
      if (entry.principal !== principal) owner.model.leaveRoom(roomId);
      else owner.model.suspend(roomId);
      entry.principal = principal; entry.identity = identity; entry.generation = ++owner.generations;
      entry.authority = undefined; entry.uploads = undefined;
    }
    entry.socket = socket;
    const controller = new AbortController();
    if (!entry.authority) {
      const generation = entry.generation;
      void createMessageAttachmentUploadClient(roomId, authority, controller.signal).then((client) => {
        if (controller.signal.aborted || entry.generation !== generation || owners.get(key) !== owner) return;
        entry.uploads = client;
        entry.authority = { roomId, participantId, generation };
        owner.model.admit(entry.authority, client);
        setAdmitted(generation);
      }).catch(() => {
        // Attaching stays unavailable; text messages do not depend on this.
      });
    }
    setAdmitted(entry.authority ? entry.generation : 0);
    return () => {
      controller.abort();
      if (entry.socket === socket) entry.socket = null;
      // Unless the effect re-enters at once, the room is now unwatched: clear what is finished.
      queueMicrotask(() => { if (!entry.socket && owners.get(key) === owner) sweep(key, owner, roomId); });
    };
    // `authority` is covered by `identity`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, identity, roomId, socket]);

  const owner = key ? owners.get(key) : undefined;
  const entry = owner?.rooms.get(roomId);
  const uploads = entry?.authority && entry.generation === admitted ? entry.uploads : undefined;
  const all = useSyncExternalStore(
    useCallback((listener: () => void) => owner ? owner.model.subscribe(listener) : () => {}, [owner]),
    () => owner?.snapshot ?? NONE,
  );

  const delivered = useMemo(() => {
    const ids = new Set<string>();
    for (const event of events) { ids.add(event.id); if (event.record_id) ids.add(event.record_id); }
    return ids;
  }, [events]);
  // Observing notifies the model's listeners, so it must not run on every snapshot:
  // only when the room's events change or the set of unresolved messages does.
  const unresolved = all.filter((message) => message.authority.roomId === roomId &&
      (message.phase === "outcome_unknown" || message.phase === "sending"))
    .map((message) => `${message.id}:${message.phase}`).join(",");
  useEffect(() => {
    if (!owner || !entry?.authority || !unresolved) return;
    owner.model.observe(entry.authority, events.filter((event) => event.attachments?.length).map((event) => ({
      id: event.record_id || event.id, room_id: roomId, type: "message_final",
      actor: { participant_id: event.actor_id }, attachments: event.attachments,
    }) as unknown as RoomEvent));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [owner, admitted, unresolved, events, roomId]);
  // The model decides that a message was sent and which event it became; the row
  // gives way only when that very event is on screen.
  const shown = useCallback((message: OutgoingMessage) =>
    message.phase === "sent" && Boolean(message.eventId) && delivered.has(message.eventId!), [delivered]);
  useEffect(() => {
    if (!owner) return;
    for (const message of all) {
      if (message.authority.roomId === roomId && shown(message)) void owner.model.discard(message.id);
    }
  }, [owner, all, roomId, shown]);

  const messages = useMemo(
    () => all.filter((message) => message.authority.roomId === roomId && message.phase !== "cancelled" && !shown(message)),
    [all, roomId, shown],
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
      if (!owner || !uploads || !entry?.authority) throw new Error("잠시 후 다시 시도해 주세요.");
      try { owner.model.send(entry.authority, draft); }
      catch (error) { throw new Error(uploadFailureMessage(error, uploads.fileLimit)); }
    },
    messages,
    failureText: (message) => message.phase === "outcome_unknown" ? ""
      : uploadFailureMessage(new UploadFailure(message.failure?.code || "", false), uploads?.fileLimit),
    cancel: (id) => { void owner?.model.cancel(id).then(() => owner.model.discard(id)); },
    retry: (id) => { owner?.model.retry(id); },
    discard: (id) => { void owner?.model.discard(id); },
  }), [owner, entry, uploads, messages]);
}
