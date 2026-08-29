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
  mode: ReadMode;
  controller: AbortController;
  state: TaskState;
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
  retire: () => void;
}>;

function aborted(signal: AbortSignal) {
  return signal.reason || new DOMException("Attachment read aborted.", "AbortError");
}

export function createMessageAttachmentReadScheduler(
  roomId: string,
  authority: MessageAttachmentAuthority,
  transport: ReadTransport = fetchMessageAttachmentBlob
): MessageAttachmentReadScheduler {
  let retired = false;
  let queue: ReadTask[] = [];
  const active = new Set<ReadTask>();

  function finish(task: ReadTask, value: Blob | unknown, failed: boolean) {
    if (task.state === "settled") return;
    task.state = "settled";
    active.delete(task);
    queue = queue.filter((candidate) => candidate !== task);
    task.detachCaller();
    if (failed) task.reject(value);
    else task.resolve(value as Blob);
    pump();
  }

  function cancel(task: ReadTask, reason: unknown) {
    if (task.state === "settled") return;
    task.controller.abort(reason);
    finish(task, reason, true);
  }

  function start(task: ReadTask) {
    if (task.state !== "queued") return;
    task.state = "active";
    active.add(task);
    void Promise.resolve()
      .then(() => transport(
        task.attachment,
        roomId,
        authority,
        task.mode,
        task.controller.signal,
        () => task.controller.signal.throwIfAborted()
      ))
      .then(
        (blob) => finish(task, blob, false),
        (error) => finish(task, error, true)
      );
  }

  function pump() {
    if (retired) return;
    while (active.size < MESSAGE_ATTACHMENT_READ_CONCURRENCY && queue.length) {
      const task = queue.shift();
      if (!task || task.state !== "queued") continue;
      if (task.controller.signal.aborted) {
        finish(task, aborted(task.controller.signal), true);
      } else {
        start(task);
      }
    }
  }

  return {
    read: (attachment, mode, signal) => {
      if (retired || signal.aborted) {
        return Promise.reject(
          signal.aborted ? aborted(signal) : new DOMException(
            "Attachment read generation retired.",
            "AbortError"
          )
        );
      }
      return new Promise<Blob>((resolve, reject) => {
        const controller = new AbortController();
        let task: ReadTask;
        const cancelFromCaller = () => cancel(task, aborted(signal));
        task = {
          attachment,
          mode,
          controller,
          state: "queued",
          resolve,
          reject,
          detachCaller: () => signal.removeEventListener("abort", cancelFromCaller),
        };
        signal.addEventListener("abort", cancelFromCaller, { once: true });
        queue.push(task);
        pump();
      });
    },
    retire: () => {
      if (retired) return;
      retired = true;
      [...queue, ...active].forEach((task) =>
        cancel(task, new DOMException("Attachment read generation retired.", "AbortError"))
      );
      queue = [];
      active.clear();
    },
  };
}
