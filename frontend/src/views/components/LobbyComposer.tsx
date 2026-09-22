import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type KeyboardEvent,
} from "react";
import type { LucideIcon } from "lucide-react";
import { AtSign, Paperclip, Send, Smile, Sparkles, X } from "lucide-react";
import {
  uploadLobbyAttachment,
  type LobbyAttachmentRef,
  type LobbyEvent,
} from "../../api";
import { useRoomSocket } from "../../RoomSocketContext";
import { RoomSocketSayError } from "../../roomSocketClient";
import {
  MAX_ATTACHMENTS_MESSAGE,
  MAX_ATTACHMENTS_PER_EVENT,
  lobbySubmitFailureDraft,
  lobbySubmitSuccessDraft,
  selectLobbyAttachmentFiles,
} from "../../lib/lobbyComposerModel";
import { isUnauthorizedApiError } from "../../lib/apiErrors";
import type { RoomPostingMode } from "../../lib/roomGuestPosting";
import type { Mentionable } from "../../lib/mentionComposerModel";
import { parseVoteCommand } from "../../lib/votePoll";
import MentionInput from "./MentionInput";
import ComposerCommandMenu, {
  matchingComposerCommands,
  type ComposerCommand,
} from "./ComposerCommandMenu";
import VoteComposerDialog, {
  type VoteComposerValue,
} from "./VoteComposerDialog";

type ComposerAccessory = {
  id: "apps";
  label: string;
  title: string;
  notice: string;
  insertText?: string;
  icon?: LucideIcon;
};

type LobbyComposerDraft = {
  message: string;
  pendingAttachments: LobbyAttachmentRef[];
  retry?: () => Promise<unknown>;
};

type LocalAttachmentPreview = {
  kind: "image" | "video";
  url: string;
};

function localPreviewKind(contentType: string): LocalAttachmentPreview["kind"] | null {
  if (typeof URL.createObjectURL !== "function") return null;
  if (contentType.startsWith("image/")) return "image";
  if (contentType.startsWith("video/")) return "video";
  return null;
}

type AttachmentUploadOperation = {
  controller: AbortController;
};

function requireCurrentAttachmentUpload(
  operation: AttachmentUploadOperation,
  current: AttachmentUploadOperation | null
) {
  operation.controller.signal.throwIfAborted();
  if (operation !== current) {
    throw new DOMException("Attachment upload retired.", "AbortError");
  }
}

const ATTACHMENT_ERROR_MS = 5000;

const EMPTY_LOBBY_COMPOSER_DRAFT: LobbyComposerDraft = {
  message: "",
  pendingAttachments: [],
};

const COMPOSER_ACCESSORIES: ComposerAccessory[] = [
  {
    id: "apps",
    label: "앱",
    title: "앱",
    notice: "앱 명령은 외부 Discord로 전송하지 않습니다. AgentsAssemble 로컬 기능만 이 방에서 다룹니다.",
    insertText: "/",
    icon: Sparkles,
  },
];

const COMPOSER_EMOJIS = ["🙂", "😂", "😍", "🤔", "👍", "👏", "🎉", "❤️", "🔥", "✅", "👀", "🚀"];

export default function LobbyComposer({
  meetingId,
  onPosted,
  submitMessage,
  mentionables = [],
  disabledReason,
  roomSessionToken = "",
  roomDeviceToken = "",
  postingMode = "host",
  onGuestSessionExpired,
}: {
  meetingId: string;
  onPosted: (events: LobbyEvent[]) => void;
  submitMessage?: (message: string) => Promise<LobbyEvent[]>;
  mentionables?: Mentionable[];
  disabledReason?: string;
  roomSessionToken?: string;
  roomDeviceToken?: string;
  postingMode?: RoomPostingMode;
  onGuestSessionExpired?: () => void;
}) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const restoreFocusAfterSubmitRef = useRef(false);
  const activeUploadOperation = useRef<AttachmentUploadOperation | null>(null);
  const commandListId = useId();
  const emojiListId = useId();
  const [draftsByRoom, setDraftsByRoom] = useState<Record<string, LobbyComposerDraft>>({});
  const [busy, setBusy] = useState(false);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState("");
  // Attachment problems (too large, too many, upload failed) are a passing notice:
  // they clear on their own, unlike a send failure that the draft depends on.
  const [attachmentError, setAttachmentError] = useState("");
  const shownError = error || attachmentError;
  useEffect(() => {
    if (!attachmentError) return undefined;
    const timer = window.setTimeout(() => setAttachmentError(""), ATTACHMENT_ERROR_MS);
    return () => window.clearTimeout(timer);
  }, [attachmentError]);
  const [accessoryNotice, setAccessoryNotice] = useState("");
  const [emojiOpen, setEmojiOpen] = useState(false);
  const [voteDialogOpen, setVoteDialogOpen] = useState(false);
  const voteRetry = useRef<{ key: string; retry: NonNullable<RoomSocketSayError["retry"]> } | null>(null);
  const [activeCommandIndex, setActiveCommandIndex] = useState(0);
  const [dismissedCommandMessage, setDismissedCommandMessage] = useState("");
  const roomSocket = useRoomSocket();
  const activeDraft = draftsByRoom[meetingId] || EMPTY_LOBBY_COMPOSER_DRAFT;
  const message = activeDraft.message;
  const pendingAttachments = activeDraft.pendingAttachments;
  const disabled = Boolean(disabledReason);
  const canUploadAttachments =
    postingMode === "host" ||
    (postingMode === "guest" && Boolean(roomSessionToken.trim()));
  const canSubmit = Boolean(message.trim() || pendingAttachments.length) && !busy && !uploading && !disabled;
  const matchingCommands = matchingComposerCommands(message);
  const commandMenuOpen =
    !disabled &&
    !busy &&
    matchingCommands.length > 0 &&
    dismissedCommandMessage !== message;
  // Thumbnails of attachments that are uploaded but not sent yet, made from the local
  // file so nothing is read back from the server.
  const [localPreviews, setLocalPreviews] = useState<Record<string, LocalAttachmentPreview>>({});
  const localPreviewsRef = useRef(localPreviews);
  localPreviewsRef.current = localPreviews;
  useEffect(() => {
    const pending = new Set(
      Object.values(draftsByRoom).flatMap((draft) =>
        draft.pendingAttachments.map((attachment) => attachment.id)
      )
    );
    const stale = Object.keys(localPreviews).filter((id) => !pending.has(id));
    if (stale.length === 0) return;
    for (const id of stale) URL.revokeObjectURL(localPreviews[id].url);
    setLocalPreviews((current) => {
      const next = { ...current };
      for (const id of stale) delete next[id];
      return next;
    });
  }, [draftsByRoom, localPreviews]);
  useEffect(
    () => () => {
      for (const preview of Object.values(localPreviewsRef.current)) {
        URL.revokeObjectURL(preview.url);
      }
    },
    []
  );
  const closeVoteDialog = useCallback(() => { voteRetry.current = null; setVoteDialogOpen(false); }, []);

  function setMessage(nextMessage: string) {
    setDraftsByRoom((previous) => {
      const current = previous[meetingId] || EMPTY_LOBBY_COMPOSER_DRAFT;
      if (current.message === nextMessage) return previous;
      return {
        ...previous,
        [meetingId]: { ...current, message: nextMessage, retry: undefined },
      };
    });
  }

  function updateMessage(nextMessage: string) {
    setMessage(nextMessage);
    setActiveCommandIndex(0);
    if (nextMessage !== dismissedCommandMessage) setDismissedCommandMessage("");
  }

  function selectCommand(command: ComposerCommand) {
    setMessage(command.command);
    setDismissedCommandMessage(command.command);
    setAccessoryNotice("");
    if (command.id === "vote") setVoteDialogOpen(true);
  }

  function setPendingAttachments(
    update:
      | LobbyAttachmentRef[]
      | ((current: LobbyAttachmentRef[]) => LobbyAttachmentRef[])
  ) {
    setDraftsByRoom((previous) => {
      const current = previous[meetingId] || EMPTY_LOBBY_COMPOSER_DRAFT;
      const nextAttachments =
        typeof update === "function"
          ? update(current.pendingAttachments)
          : update;
      if (current.pendingAttachments === nextAttachments) return previous;
      return {
        ...previous,
        [meetingId]: {
          ...current,
          pendingAttachments: nextAttachments,
          retry: undefined,
        },
      };
    });
  }

  useEffect(() => {
    setVoteDialogOpen(false);
    setEmojiOpen(false);
  }, [meetingId]);

  useLayoutEffect(() => {
    setUploading(false);
    return () => {
      const operation = activeUploadOperation.current;
      activeUploadOperation.current = null;
      operation?.controller.abort();
    };
  }, [disabledReason, meetingId, postingMode, roomSessionToken, roomDeviceToken]);

  useEffect(() => {
    if (busy || !restoreFocusAfterSubmitRef.current) return;
    restoreFocusAfterSubmitRef.current = false;
    inputRef.current?.focus();
  }, [busy]);

  function insertText(text: string) {
    const input = inputRef.current;
    const start = input?.selectionStart ?? message.length;
    const end = input?.selectionEnd ?? message.length;
    const next = `${message.slice(0, start)}${text}${message.slice(end)}`;
    setMessage(next);
    window.setTimeout(() => {
      inputRef.current?.focus();
      inputRef.current?.setSelectionRange(start + text.length, start + text.length);
    }, 0);
  }

  function handleAccessoryClick(accessory: ComposerAccessory) {
    if (disabled || busy) return;
    setError("");
    setAccessoryNotice(accessory.notice);
    if (accessory.insertText) insertText(accessory.insertText);
  }

  async function handleFileChange(event: ChangeEvent<HTMLInputElement>) {
    if (disabled || !canUploadAttachments) return;
    const selected = Array.from(event.currentTarget.files || []);
    event.currentTarget.value = "";
    await uploadFiles(selected);
  }

  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const pasted = Array.from(event.clipboardData.files);
    if (pasted.length === 0) return;
    // Files on the clipboard (a copied image or screenshot) become attachments; the
    // text of a paste that carries no file is left to the textarea.
    event.preventDefault();
    if (disabled || busy || uploading) return;
    if (!canUploadAttachments) {
      setAttachmentError("이 방에서는 파일을 첨부할 수 없어요.");
      return;
    }
    void uploadFiles(pasted);
  }

  async function uploadFiles(selected: File[]) {
    if (!selected.length) return;

    const { accepted: filesToUpload, error: selectionError } = selectLobbyAttachmentFiles(
      pendingAttachments.length,
      selected
    );
    if (filesToUpload.length === 0) {
      setAttachmentError(selectionError || MAX_ATTACHMENTS_MESSAGE);
      return;
    }
    setAttachmentError(selectionError);

    const operation: AttachmentUploadOperation = {
      controller: new AbortController(),
    };
    activeUploadOperation.current = operation;
    setUploading(true);
    try {
      const uploaded: LobbyAttachmentRef[] = [];
      const previews: Record<string, LocalAttachmentPreview> = {};
      for (const file of filesToUpload) {
        const attachment = await uploadLobbyAttachment(file, {
          roomId: meetingId,
          sessionToken: roomSessionToken,
          deviceToken: roomDeviceToken,
          signal: operation.controller.signal,
          beforeDispatch: () =>
            requireCurrentAttachmentUpload(operation, activeUploadOperation.current),
        });
        uploaded.push(attachment);
        const kind = localPreviewKind(file.type);
        if (kind) previews[attachment.id] = { kind, url: URL.createObjectURL(file) };
      }
      requireCurrentAttachmentUpload(operation, activeUploadOperation.current);
      setLocalPreviews((current) => ({ ...current, ...previews }));
      setPendingAttachments((current) =>
        [...current, ...uploaded].slice(0, MAX_ATTACHMENTS_PER_EVENT)
      );
    } catch (errorValue) {
      if (
        activeUploadOperation.current === operation &&
        !operation.controller.signal.aborted
      ) {
        setAttachmentError(errorValue instanceof Error ? errorValue.message : "첨부 업로드 실패");
      }
    } finally {
      if (activeUploadOperation.current === operation) {
        activeUploadOperation.current = null;
        setUploading(false);
      }
    }
  }

  function removePendingAttachment(attachmentId: string) {
    if (disabled || busy || uploading) return;
    setPendingAttachments((current) =>
      current.filter((attachment) => attachment.id !== attachmentId)
    );
  }

  async function handleSubmit() {
    if (disabled || busy || uploading) return;
    const draftMessage = message;
    const draftAttachments = pendingAttachments;
    const trimmed = draftMessage.trim();
    if (!trimmed && draftAttachments.length === 0) return;
    if (trimmed.toLocaleLowerCase() === "/vote") {
      setError("");
      setDismissedCommandMessage(draftMessage);
      setVoteDialogOpen(true);
      return;
    }

    restoreFocusAfterSubmitRef.current = true;
    setBusy(true);
    setError("");
    try {
      if (postingMode === "guest" && !roomSessionToken) {
        throw new Error("메시지를 보내려면 유효한 초대 세션이 필요합니다.");
      }
      // "/vote 질문 | 옵션1 | 옵션2" opens a poll card instead of a message.
      const voteCommand = parseVoteCommand(trimmed);
      const sayRequest = {
        message: voteCommand ? "" : trimmed,
        attachments: draftAttachments,
        kind: voteCommand ? ("vote" as const) : ("message" as const),
        voteQuestion: voteCommand?.question || "",
        voteOptions: voteCommand?.options || [],
      };
      const payload = activeDraft.retry
        ? (await activeDraft.retry(), { events: [] })
        : submitMessage && sayRequest.kind === "message" && sayRequest.attachments.length === 0
          ? { events: await submitMessage(sayRequest.message) }
          : roomSocket?.ready()
            ? await roomSocket.say(sayRequest)
            : await Promise.reject(
                new RoomSocketSayError(
                  "방 연결이 준비되지 않았습니다. 연결된 뒤 다시 보내 주세요.",
                  "socket_not_ready"
                )
              );
      const cleared = lobbySubmitSuccessDraft<LobbyAttachmentRef>();
      setMessage(cleared.message);
      setPendingAttachments(cleared.pendingAttachments);
      onPosted(payload.events || (payload.event ? [payload.event] : []));
    } catch (errorValue) {
      if (
        isUnauthorizedApiError(errorValue) ||
        (errorValue instanceof RoomSocketSayError && errorValue.category === "session_revoked")
      ) {
        onGuestSessionExpired?.();
      }
      const restored = lobbySubmitFailureDraft(
        draftMessage,
        draftAttachments,
        errorValue instanceof Error ? errorValue.message : "채팅 메시지 전송 실패"
      );
      setDraftsByRoom((previous) => ({
        ...previous,
        [meetingId]: {
          message: restored.message,
          pendingAttachments: restored.pendingAttachments,
          retry: errorValue instanceof RoomSocketSayError && errorValue.retry
            ? errorValue.retry : activeDraft.retry,
        },
      }));
      setError(restored.error);
    } finally {
      setBusy(false);
    }
  }

  async function submitVote(value: VoteComposerValue) {
    if (disabled || busy || uploading) {
      throw new Error("지금은 투표를 만들 수 없습니다.");
    }
    const key = JSON.stringify([meetingId, value, pendingAttachments]);
    if (voteRetry.current?.key !== key) voteRetry.current = null;
    setBusy(true);
    setError("");
    try {
      if (postingMode === "guest" && !roomSessionToken) {
        throw new Error("투표를 만들려면 유효한 초대 세션이 필요합니다.");
      }
      if (!roomSocket?.ready()) {
        throw new RoomSocketSayError(
          "방 연결이 준비되지 않았습니다. 연결된 뒤 다시 보내 주세요.",
          "socket_not_ready"
        );
      }
      const payload = voteRetry.current
        ? (await voteRetry.current.retry(), { events: [] })
        : await roomSocket.say({
        message: "",
        attachments: pendingAttachments,
        kind: "vote",
        voteQuestion: value.question,
        voteOptions: value.options,
        voteDurationSeconds: value.durationSeconds,
      });
      const cleared = lobbySubmitSuccessDraft<LobbyAttachmentRef>();
      setMessage(cleared.message);
      setPendingAttachments(cleared.pendingAttachments);
      onPosted(payload.events || (payload.event ? [payload.event] : []));
    } catch (errorValue) {
      if (errorValue instanceof RoomSocketSayError && errorValue.retry) {
        voteRetry.current = { key, retry: errorValue.retry };
      }
      if (
        isUnauthorizedApiError(errorValue) ||
        (errorValue instanceof RoomSocketSayError &&
          errorValue.category === "session_revoked")
      ) {
        onGuestSessionExpired?.();
      }
      throw errorValue;
    } finally {
      setBusy(false);
    }
  }

  function handleKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (commandMenuOpen) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActiveCommandIndex((current) => (current + 1) % matchingCommands.length);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActiveCommandIndex(
          (current) => (current - 1 + matchingCommands.length) % matchingCommands.length
        );
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        const command = matchingCommands[activeCommandIndex] || matchingCommands[0];
        if (command) selectCommand(command);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setDismissedCommandMessage(message);
        return;
      }
    }
    if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
    if (event.shiftKey) return;
    event.preventDefault();
    void handleSubmit();
  }

  return (
    <>
      <section className="dc-composer-shell">
      {shownError && (
        <p className="mb-2 flex items-start gap-2 rounded border border-danger/30 bg-danger/10 px-3 py-2 text-[12px] font-semibold text-danger preserve-words">
          <span className="min-w-0 flex-1">{shownError}</span>
          <button
            type="button"
            onClick={() => {
              setError("");
              setAttachmentError("");
            }}
            className="grid h-4 w-4 shrink-0 place-items-center rounded text-danger/80 hover:text-danger"
            aria-label="알림 닫기"
          >
            <X size={12} />
          </button>
        </p>
      )}
      {disabledReason && (
        <p className="dc-composer-readonly preserve-words">
          {disabledReason}
        </p>
      )}
      {accessoryNotice && !disabledReason && (
        <p className="dc-composer-accessory-notice preserve-words" aria-live="polite">
          {accessoryNotice}
        </p>
      )}

      {pendingAttachments.length > 0 && (
        <div className="mb-2 flex flex-wrap gap-2">
          {pendingAttachments.map((attachment) => {
            const preview = localPreviews[attachment.id];
            return (
              <span
                key={attachment.id}
                className={`dc-composer-attachment inline-flex max-w-full text-[12px] font-bold text-text-secondary ${
                  preview ? "w-40 flex-col items-stretch gap-1.5 p-1.5" : "items-center gap-2 px-3 py-1.5"
                }`}
              >
                {preview?.kind === "image" && (
                  <img
                    src={preview.url}
                    alt=""
                    className="h-32 w-full rounded-[3px] bg-black/30 object-contain"
                  />
                )}
                {preview?.kind === "video" && (
                  <video
                    src={preview.url}
                    muted
                    playsInline
                    preload="metadata"
                    aria-hidden="true"
                    className="h-32 w-full rounded-[3px] bg-black/30 object-contain"
                  />
                )}
                <span className={`flex min-w-0 items-center gap-2 ${preview ? "px-0.5" : ""}`}>
                  <span className="min-w-0 flex-1 truncate preserve-words">{attachment.filename}</span>
                  <button
                    type="button"
                    onClick={() => removePendingAttachment(attachment.id)}
                    disabled={busy || uploading}
                    className="grid h-5 w-5 shrink-0 place-items-center rounded border border-line/70 text-text-muted hover:border-danger/45 hover:text-danger"
                    aria-label={`${attachment.filename} 첨부 제거`}
                  >
                    <X size={12} />
                  </button>
                </span>
              </span>
            );
          })}
        </div>
      )}

      <div className="dc-composer-bar">
        {commandMenuOpen && (
          <ComposerCommandMenu
            listId={commandListId}
            commands={matchingCommands}
            activeIndex={activeCommandIndex}
            onActiveIndexChange={setActiveCommandIndex}
            onSelect={selectCommand}
          />
        )}
        <MentionInput
          inputRef={inputRef}
          value={message}
          onChange={updateMessage}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          className="dc-composer-input"
          placeholder={disabledReason || (uploading ? "첨부 업로드 중..." : "메시지 입력")}
          disabled={busy || disabled}
          mentionables={mentionables}
          ariaLabel="채팅 입력"
          externalListId={commandListId}
          externalActiveOptionId={
            commandMenuOpen ? `${commandListId}-option-${activeCommandIndex}` : undefined
          }
          externalListOpen={commandMenuOpen}
        />
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={handleFileChange}
          aria-label="채팅 첨부 선택"
        />
        <button
          type="button"
          onClick={() => fileInputRef.current?.click()}
          disabled={
            disabled ||
            !canUploadAttachments ||
            busy ||
            uploading ||
            pendingAttachments.length >= MAX_ATTACHMENTS_PER_EVENT
          }
          className="dc-composer-button"
          data-role="attachment"
          aria-label="첨부 추가"
          title={`첨부 ${pendingAttachments.length}/${MAX_ATTACHMENTS_PER_EVENT}`}
        >
          <Paperclip size={17} />
        </button>
        {COMPOSER_ACCESSORIES.map((accessory) => {
          const Icon = accessory.icon;
          return (
            <button
              key={accessory.id}
              type="button"
              onClick={() => handleAccessoryClick(accessory)}
              disabled={busy || disabled}
              className="dc-composer-button"
              data-accessory={accessory.id}
              aria-label={`채팅 ${accessory.label}`}
              title={accessory.title}
            >
              {Icon ? <Icon size={17} /> : <span className="dc-composer-button-label">{accessory.label}</span>}
            </button>
          );
        })}
        <button
          type="button"
          onClick={() => insertText("@")}
          disabled={busy || disabled}
          className="dc-composer-button"
          data-role="mention"
          aria-label="멘션 삽입"
          title="@멘션"
        >
          <AtSign size={17} />
        </button>
        <button
          type="button"
          onClick={() => setEmojiOpen((current) => !current)}
          disabled={busy || disabled}
          className="dc-composer-button"
          data-role="emoji"
          aria-label="이모지 삽입"
          aria-expanded={emojiOpen}
          aria-controls={emojiListId}
          title="이모지"
        >
          <Smile size={17} />
        </button>
        {emojiOpen && (
          <div
            id={emojiListId}
            className="dc-composer-emoji-picker"
            role="listbox"
            aria-label="이모지 선택"
          >
            {COMPOSER_EMOJIS.map((emoji) => (
              <button
                key={emoji}
                type="button"
                role="option"
                aria-selected="false"
                aria-label={emoji}
                onClick={() => {
                  insertText(emoji);
                  setEmojiOpen(false);
                }}
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
        <button
          type="button"
          onClick={handleSubmit}
          disabled={!canSubmit}
          className="dc-composer-button send"
          data-role="send"
          aria-label={activeDraft.retry ? "같은 요청 다시 보내기" : "채팅 메시지 보내기"}
          title={activeDraft.retry ? "같은 요청 다시 보내기" : "채팅 메시지 보내기"}
        >
          <Send size={17} />
        </button>
      </div>
      </section>
      {voteDialogOpen && (
        <VoteComposerDialog
          onClose={closeVoteDialog}
          onSubmit={submitVote}
        />
      )}
    </>
  );
}
