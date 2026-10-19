import { createResourceUrl, revokeResourceUrl } from "../../lib/remote/remoteResources";
import ResourceImage from "./ResourceImage";
import UploadLimitDialog from "./UploadLimitDialog";
import {
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type KeyboardEvent,
} from "react";
import type { LucideIcon } from "lucide-react";
import { AtSign, FileText, Paperclip, Plus, Send, Smile, Sparkles, Trash2, X } from "lucide-react";
import type { LobbyEvent } from "../../api";
import { useRoomSocket } from "../../RoomSocketContext";
import { RoomSocketSayError } from "../../roomSocketClient";
import {
  lobbySubmitFailureDraft,
  lobbySubmitSuccessDraft,
  selectLobbyAttachmentFiles,
} from "../../lib/lobbyComposerModel";
import { isUnauthorizedApiError, userFacingError } from "../../lib/apiErrors";
import type { RoomPostingMode } from "../../lib/roomGuestPosting";
import type { OutgoingDraft } from "../../lib/outgoingMessages";
import type { Mentionable } from "../../lib/mentionComposerModel";
import { resolveMentionMarkup } from "../../lib/mentionComposerModel";
import { parseVoteCommand } from "../../lib/votePoll";
import MentionInput from "./MentionInput";
import { ReplyDraft, type ReplySource } from "./MessageReply";
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
  pendingAttachments: StagedFile[];
  retry?: () => Promise<unknown>;
  retryReplyToEventId?: string;
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

// A file waits in the draft as a local File; nothing is sent until the message is.
type StagedFile = { id: string; filename: string; file: File };

/** What the composer needs from the room's outgoing-message owner. */
export type ComposerOutgoing = {
  ready: boolean;
  limit: number;
  check(file: File): string;
  send(draft: OutgoingDraft): void;
};

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
    notice: "앱 명령은 외부 Discord로 전송하지 않아요. AgentsAssemble 로컬 기능만 이 방에서 다뤄요.",
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
  channelName = "general",
  roomSessionToken = "",
  postingMode = "host",
  onGuestSessionExpired,
  replyTo,
  onCancelReply,
  onReplySent,
  outgoing,
}: {
  meetingId: string;
  onPosted: (events: LobbyEvent[]) => void;
  submitMessage?: (message: string) => Promise<LobbyEvent[]>;
  mentionables?: Mentionable[];
  disabledReason?: string;
  channelName?: string;
  roomSessionToken?: string;
  postingMode?: RoomPostingMode;
  onGuestSessionExpired?: () => void;
  replyTo?: ReplySource;
  onCancelReply?: () => void;
  onReplySent?: (eventId: string) => void;
  outgoing?: ComposerOutgoing;
}) {
  const fileInputRef = useRef<HTMLInputElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  useEffect(() => { if (replyTo) inputRef.current?.focus(); }, [replyTo?.eventId]);
  const restoreFocusAfterSubmitRef = useRef(false);
  const commandListId = useId();
  const emojiListId = useId();
  const toolsListId = useId();
  const toolsRef = useRef<HTMLDivElement>(null);
  const toolsButtonRef = useRef<HTMLButtonElement>(null);
  const [toolsOpen, setToolsOpen] = useState(false);
  const [activeToolIndex, setActiveToolIndex] = useState(0);
  useEffect(() => {
    if (!toolsOpen) return;
    const menu = toolsRef.current;
    menu?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
    const outside = (event: PointerEvent) => {
      if (!menu?.contains(event.target as Node) && !toolsButtonRef.current?.contains(event.target as Node)) setToolsOpen(false);
    };
    const resize = () => setToolsOpen(false);
    window.addEventListener("pointerdown", outside);
    window.addEventListener("resize", resize);
    return () => {
      window.removeEventListener("pointerdown", outside);
      window.removeEventListener("resize", resize);
    };
  }, [toolsOpen]);
  const [draftsByRoom, setDraftsByRoom] = useState<Record<string, LobbyComposerDraft>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  // Attachment problems (too large, too many, upload failed) are a passing notice:
  // they clear on their own, unlike a send failure that the draft depends on.
  const [attachmentError, setAttachmentError] = useState("");
  const shownError = error || attachmentError;
  const [tooManyOpen, setTooManyOpen] = useState(false);
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
  const attachmentLimit = outgoing?.limit ?? 0;
  const canUploadAttachments = Boolean(outgoing?.ready) && (
    postingMode === "host" ||
    (postingMode === "guest" && Boolean(roomSessionToken.trim())));
  const canSubmit = Boolean(message.trim() || pendingAttachments.length) && !busy && !disabled;
  const matchingCommands = matchingComposerCommands(message);
  const commandMenuOpen =
    !disabled &&
    !busy &&
    matchingCommands.length > 0 &&
    dismissedCommandMessage !== message;
  // Thumbnails of staged files, made from the local file.
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
    for (const id of stale) revokeResourceUrl(localPreviews[id].url);
    setLocalPreviews((current) => {
      const next = { ...current };
      for (const id of stale) delete next[id];
      return next;
    });
  }, [draftsByRoom, localPreviews]);
  useEffect(
    () => () => {
      for (const preview of Object.values(localPreviewsRef.current)) {
        revokeResourceUrl(preview.url);
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
      | StagedFile[]
      | ((current: StagedFile[]) => StagedFile[])
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

  function handleFileChange(event: ChangeEvent<HTMLInputElement>) {
    if (disabled || !canUploadAttachments) return;
    const selected = Array.from(event.currentTarget.files || []);
    event.currentTarget.value = "";
    uploadFiles(selected);
  }

  function handlePaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const pasted = Array.from(event.clipboardData.files);
    if (pasted.length === 0) return;
    // Files on the clipboard (a copied image or screenshot) become attachments; the
    // text of a paste that carries no file is left to the textarea.
    event.preventDefault();
    if (disabled || busy) return;
    if (!canUploadAttachments) {
      setAttachmentError("이 방에서는 파일을 첨부할 수 없어요.");
      return;
    }
    uploadFiles(pasted);
  }

  function uploadFiles(selected: File[]) {
    if (!selected.length || !outgoing) return;

    const { accepted, tooMany } = selectLobbyAttachmentFiles(
      pendingAttachments.length,
      selected,
      outgoing.limit
    );
    if (tooMany) {
      setTooManyOpen(true);
      return;
    }
    // One file that cannot be sent refuses the selection, with its reason.
    const refusal = accepted.map((file) => outgoing.check(file)).find(Boolean);
    if (refusal) {
      setAttachmentError(refusal);
      return;
    }
    setAttachmentError("");
    const staged = accepted.map((file) => ({
      id: crypto.randomUUID(),
      filename: file.name || "attachment.bin",
      file,
    }));
    const previews: Record<string, LocalAttachmentPreview> = {};
    for (const item of staged) {
      const kind = localPreviewKind(item.file.type);
      if (kind) previews[item.id] = { kind, url: createResourceUrl(item.file) };
    }
    setLocalPreviews((current) => ({ ...current, ...previews }));
    setPendingAttachments((current) => [...current, ...staged]);
  }

  function removePendingAttachment(attachmentId: string) {
    if (disabled || busy) return;
    setPendingAttachments((current) =>
      current.filter((attachment) => attachment.id !== attachmentId)
    );
  }

  async function handleSubmit() {
    if (disabled || busy) return;
    const draftMessage = message;
    const draftAttachments = pendingAttachments;
    const trimmed = resolveMentionMarkup(draftMessage, mentionables).trim();
    let replyToEventId = replyTo?.eventId;
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
        throw new Error("메시지를 보내려면 유효한 초대 세션이 필요해요.");
      }
      // "/vote 질문 | 옵션1 | 옵션2" opens a poll card instead of a message.
      const voteCommand = parseVoteCommand(trimmed);
      if (voteCommand) replyToEventId = undefined;
      const sayRequest = {
        message: voteCommand ? "" : trimmed,
        attachments: [],
        kind: voteCommand ? ("vote" as const) : ("message" as const),
        voteQuestion: voteCommand?.question || "",
        voteOptions: voteCommand?.options || [],
        replyToEventId,
      };
      if (draftAttachments.length > 0) {
        // A message with files leaves the composer at once and uploads as an
        // outgoing message; the room shows its progress.
        if (!outgoing) throw new Error("이 방에서는 파일을 첨부할 수 없어요.");
        outgoing.send({
          text: sayRequest.message,
          replyToEventId: sayRequest.replyToEventId,
          files: draftAttachments.map((item) => item.file),
          request: voteCommand
            ? { kind: "vote", voteQuestion: sayRequest.voteQuestion, voteOptions: sayRequest.voteOptions }
            : undefined,
        });
        const cleared = lobbySubmitSuccessDraft<StagedFile>();
        setMessage(cleared.message);
        setPendingAttachments(cleared.pendingAttachments);
        if (sayRequest.replyToEventId) onReplySent?.(sayRequest.replyToEventId);
        return;
      }
      const payload = activeDraft.retry && activeDraft.retryReplyToEventId === sayRequest.replyToEventId
        ? (await activeDraft.retry(), { events: [] })
        : submitMessage && !sayRequest.replyToEventId && sayRequest.kind === "message"
          ? { events: await submitMessage(sayRequest.message) }
          : roomSocket?.ready()
            ? await roomSocket.say(sayRequest)
            : await Promise.reject(
                new RoomSocketSayError(
                  "방 연결이 준비되지 않았어요. 연결된 뒤 다시 보내 주세요.",
                  "socket_not_ready"
                )
              );
      const cleared = lobbySubmitSuccessDraft<StagedFile>();
      setMessage(cleared.message);
      setPendingAttachments(cleared.pendingAttachments);
      if (sayRequest.replyToEventId) onReplySent?.(sayRequest.replyToEventId);
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
        userFacingError(errorValue, "채팅 메시지 전송 실패")
      );
      setDraftsByRoom((previous) => ({
        ...previous,
        [meetingId]: {
          message: restored.message,
          pendingAttachments: restored.pendingAttachments,
          retry: errorValue instanceof RoomSocketSayError && errorValue.retry
            ? errorValue.retry : activeDraft.retryReplyToEventId === replyToEventId ? activeDraft.retry : undefined,
          retryReplyToEventId: replyToEventId,
        },
      }));
      setError(restored.error);
    } finally {
      setBusy(false);
    }
  }

  async function submitVote(value: VoteComposerValue) {
    if (disabled || busy) {
      throw new Error("지금은 투표를 만들 수 없어요.");
    }
    if (pendingAttachments.length > 0) {
      if (!outgoing) throw new Error("이 방에서는 파일을 첨부할 수 없어요.");
      outgoing.send({
        text: "",
        files: pendingAttachments.map((item) => item.file),
        request: {
          kind: "vote",
          voteQuestion: value.question,
          voteOptions: value.options,
          voteDurationSeconds: value.durationSeconds,
        },
      });
      const cleared = lobbySubmitSuccessDraft<StagedFile>();
      setMessage(cleared.message);
      setPendingAttachments(cleared.pendingAttachments);
      return;
    }
    const key = JSON.stringify([meetingId, value]);
    if (voteRetry.current?.key !== key) voteRetry.current = null;
    setBusy(true);
    setError("");
    try {
      if (postingMode === "guest" && !roomSessionToken) {
        throw new Error("투표를 만들려면 유효한 초대 세션이 필요해요.");
      }
      if (!roomSocket?.ready()) {
        throw new RoomSocketSayError(
          "방 연결이 준비되지 않았어요. 연결된 뒤 다시 보내 주세요.",
          "socket_not_ready"
        );
      }
      const payload = voteRetry.current
        ? (await voteRetry.current.retry(), { events: [] })
        : await roomSocket.say({
        message: "",
        attachments: [],
        kind: "vote",
        voteQuestion: value.question,
        voteOptions: value.options,
        voteDurationSeconds: value.durationSeconds,
      });
      const cleared = lobbySubmitSuccessDraft<StagedFile>();
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

  const composerTools = [
    { id: "attachment", label: "첨부 추가", title: `첨부 ${pendingAttachments.length}/${attachmentLimit}`, icon: Paperclip,
      disabled: disabled || !canUploadAttachments || busy || pendingAttachments.length >= attachmentLimit,
      run: () => fileInputRef.current?.click() },
    ...COMPOSER_ACCESSORIES.map(accessory => ({ id: accessory.id, label: `채팅 ${accessory.label}`, title: accessory.title, icon: accessory.icon,
      disabled: busy || disabled, run: () => handleAccessoryClick(accessory) })),
    { id: "mention", label: "멘션 삽입", title: "@멘션", icon: AtSign, disabled: busy || disabled, run: () => insertText("@") },
    { id: "emoji", label: "이모지 삽입", title: "이모지", icon: Smile, disabled: busy || disabled, run: () => setEmojiOpen(current => !current) },
  ];

  return (
    <>
      <section className="dc-composer-shell">
      {replyTo && onCancelReply && <ReplyDraft source={replyTo} onCancel={onCancelReply} />}
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
        <ul className="dc-upload-tray" aria-label="보낼 파일">
          {pendingAttachments.map((attachment) => {
            const preview = localPreviews[attachment.id];
            return (
              <li key={attachment.id} className="dc-upload-card">
                <span className="dc-upload-card-preview">
                  {preview?.kind === "image" && <ResourceImage src={preview.url} alt="" />}
                  {preview?.kind === "video" && (
                    <video src={preview.url} muted playsInline preload="metadata" aria-hidden="true" />
                  )}
                  {!preview && <FileText size={40} aria-hidden="true" />}
                </span>
                <span className="dc-upload-card-name preserve-words">{attachment.filename}</span>
                <button
                  type="button"
                  onClick={() => removePendingAttachment(attachment.id)}
                  disabled={busy}
                  className="dc-upload-card-remove"
                  aria-label={`${attachment.filename} 첨부 제거`}
                  data-tip="첨부 제거"
                >
                  <Trash2 size={18} />
                </button>
              </li>
            );
          })}
        </ul>
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
          placeholder={disabledReason || `#${channelName}에 메시지 보내기`}
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
        <button ref={toolsButtonRef} type="button" className="dc-composer-button dc-composer-tools-toggle"
          aria-label="채팅 도구 열기" aria-haspopup="menu" aria-expanded={toolsOpen} aria-controls={toolsListId}
          disabled={busy || disabled} onClick={() => { setDismissedCommandMessage(message); setEmojiOpen(false); setToolsOpen(current => !current); }}>
          <Plus size={20} />
        </button>
        {toolsOpen && <div ref={toolsRef} className="dc-composer-tools-menu" onBlur={event => {
          if (!event.currentTarget.contains(event.relatedTarget) && event.relatedTarget !== toolsButtonRef.current) setToolsOpen(false);
        }} onKeyDown={event => {
          if (event.key === "Escape") {
            event.preventDefault(); setToolsOpen(false); toolsButtonRef.current?.focus();
          } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
            event.preventDefault();
            const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
            const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
            const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1 : (index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
            buttons[next]?.focus();
          }
        }}>
          <ComposerCommandMenu actions listId={toolsListId} activeIndex={activeToolIndex} onActiveIndexChange={setActiveToolIndex}
            commands={composerTools} onSelect={item => { setToolsOpen(false); toolsButtonRef.current?.focus(); item.run(); }} />
        </div>}
        {composerTools.map(tool => {
          const Icon = tool.icon;
          return <button key={tool.id} type="button" onClick={tool.run} disabled={tool.disabled}
            className="dc-composer-button dc-composer-wide-tool"
            data-role={tool.id === "apps" ? undefined : tool.id} data-accessory={tool.id === "apps" ? tool.id : undefined}
            aria-label={tool.label} title={tool.title}
            aria-expanded={tool.id === "emoji" ? emojiOpen : undefined} aria-controls={tool.id === "emoji" ? emojiListId : undefined}>
            {Icon ? <Icon size={20} /> : <span className="dc-composer-button-label">{tool.title}</span>}
          </button>;
        })}
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
          <Send size={20} />
        </button>
      </div>
      </section>
      {tooManyOpen && (
        <UploadLimitDialog limit={attachmentLimit} onClose={() => setTooManyOpen(false)} />
      )}
      {voteDialogOpen && (
        <VoteComposerDialog
          onClose={closeVoteDialog}
          onSubmit={submitVote}
        />
      )}
    </>
  );
}
