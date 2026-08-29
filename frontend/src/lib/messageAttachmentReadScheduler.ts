import {
  fetchMessageAttachmentBlob,
  type LobbyAttachmentRef,
  type MessageAttachmentAuthority,
} from "../api/messageAttachments";

export const MESSAGE_ATTACHMENT_READ_CONCURRENCY = 4;

type ReadMode = "view" | "download";
type ReadTransport = (
  attachment: LobbyAttachmentRef,
  roomId: string,
  authority: MessageAttachmentAuthority,
  mode: ReadMode,
  signal: AbortSignal,
  beforeDispatch: () => void
) => Promise<Blob>;
type TaskState = "queued" | "active" | "settled";

type ReadTask = {
  attachment: LobbyAttachmentRef;
  roomId: string;
  authority: MessageAttachmentAuthority;
  mode: ReadMode;
  controller: AbortController;
  state: TaskState;
  transportStarted: boolean;
  callerSettled: boolean;
  resolve: (blob: Blob) => void;
  reject: (error: unknown) => void;
  detachCaller: () => void;
};

export type MessageAttachmentReadScheduler = Readonly<{
  read: (
    attachment: LobbyAttachmentRef,
    mode: ReadMode,
    signal: AbortSignal
  ) => Promise<Blob>;
}>;

export type MessageAttachmentReadOwner = Readonly<{
  forAuthority: (
    roomId: string,
    authority: MessageAttachmentAuthority
  ) => MessageAttachmentReadScheduler;
}>;

function aborted(signal: AbortSignal) {
  return signal.reason || new DOMException("Attachment read aborted.", "AbortError");
}

export function createMessageAttachmentReadOwner(
  transport: ReadTransport = fetchMessageAttachmentBlob
): MessageAttachmentReadOwner {
  let queue: ReadTask[] = [];
  const active = new Set<ReadTask>();

  function settleCaller(task: ReadTask, value: Blob | unknown, failed: boolean) {
    if (task.callerSettled) return;
    task.callerSettled = true;
    task.detachCaller();
    if (failed) task.reject(value);
    else task.resolve(value as Blob);
  }

  function release(task: ReadTask) {
    if (task.state === "settled") return;
    task.state = "settled";
    active.delete(task);
    queue = queue.filter((candidate) => candidate !== task);
    pump();
  }

  function cancel(task: ReadTask, reason: unknown) {
    if (task.state === "settled") return;
    task.controller.abort(reason);
    settleCaller(task, reason, true);
    if (task.state === "queued" || !task.transportStarted) release(task);
  }

  function start(task: ReadTask) {
    if (task.state !== "queued") return;
    task.state = "active";
    active.add(task);
    void Promise.resolve().then(async () => {
      if (task.state !== "active" || task.controller.signal.aborted) {
        release(task);
        return;
      }
      task.transportStarted = true;
      try {
        const blob = await transport(
          task.attachment,
          task.roomId,
          task.authority,
          task.mode,
          task.controller.signal,
          () => task.controller.signal.throwIfAborted()
        );
        settleCaller(task, blob, false);
      } catch (error) {
        settleCaller(task, error, true);
      } finally {
        release(task);
      }
    });
  }

  function pump() {
    while (active.size < MESSAGE_ATTACHMENT_READ_CONCURRENCY && queue.length) {
      const task = queue.shift();
      if (!task || task.state !== "queued") continue;
      if (task.controller.signal.aborted) {
        settleCaller(task, aborted(task.controller.signal), true);
        release(task);
      } else {
        start(task);
      }
    }
  }

  return {
    forAuthority: (roomId, authority) => ({
      read: (attachment, mode, signal) => {
        if (signal.aborted) return Promise.reject(aborted(signal));
        return new Promise<Blob>((resolve, reject) => {
          const controller = new AbortController();
          let task: ReadTask;
          const cancelFromCaller = () => cancel(task, aborted(signal));
          task = {
            attachment,
            roomId,
            authority,
            mode,
            controller,
            state: "queued",
            transportStarted: false,
            callerSettled: false,
            resolve,
            reject,
            detachCaller: () => signal.removeEventListener("abort", cancelFromCaller),
          };
          signal.addEventListener("abort", cancelFromCaller, { once: true });
          queue.push(task);
          pump();
        });
      },
    }),
  };
}
