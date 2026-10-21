import ResourceImage from "./ResourceImage";
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type ClipboardEvent,
  type KeyboardEvent,
  type RefObject,
} from "react";
import { Bot, UserRound } from "../../ui/icons";
import {
  insertMentionText,
  mentionOptions,
  mentionQueryAtCursor,
  type Mentionable,
} from "../../lib/mentionComposerModel";
import ProviderLogo from "./ProviderLogo";
import {
  applyDisplayEdit,
  cursorInMention,
  draftDisplay,
  mentionLabels,
  replaceDisplayRange,
} from "../../lib/mentionDraftDisplay";

type MentionInputProps = {
  value: string;
  onChange: (value: string) => void;
  mentionables?: Mentionable[];
  inputRef?: RefObject<HTMLTextAreaElement | null>;
  onKeyDown?: (event: KeyboardEvent<HTMLTextAreaElement>) => void;
  onPaste?: (event: ClipboardEvent<HTMLTextAreaElement>) => void;
  className?: string;
  placeholder?: string;
  disabled?: boolean;
  maxLength?: number;
  ariaLabel?: string;
  externalListId?: string;
  externalActiveOptionId?: string;
  externalListOpen?: boolean;
};

export default function MentionInput({
  value,
  onChange,
  mentionables = [],
  inputRef,
  onKeyDown,
  onPaste,
  className,
  placeholder,
  disabled,
  maxLength,
  ariaLabel,
  externalListId,
  externalActiveOptionId,
  externalListOpen = false,
}: MentionInputProps) {
  const internalRef = useRef<HTMLTextAreaElement>(null);
  const targetRef = inputRef || internalRef;
  const mentionListId = useId();
  // `value` keeps each picked mention as `<@id>`; the box shows `@Name`.
  const labels = useMemo(() => mentionLabels(mentionables), [mentionables]);
  const display = useMemo(() => draftDisplay(value, labels), [labels, value]);
  const shown = display.text;
  const [activeOptionIndex, setActiveOptionIndex] = useState(0);
  const [mentionCursor, setMentionCursor] = useState(shown.length);
  const [dismissedMentionKey, setDismissedMentionKey] = useState("");
  const [suppressMentionSuggestions, setSuppressMentionSuggestions] = useState(false);
  const mentionMatch = useMemo(
    () => (cursorInMention(display, mentionCursor) ? null : mentionQueryAtCursor(shown, mentionCursor)),
    [display, mentionCursor, shown]
  );
  const mentionQueryKey = mentionMatch ? `${mentionMatch.start}:${mentionMatch.query}` : "";
  const options = useMemo(
    () =>
      suppressMentionSuggestions || (mentionQueryKey && dismissedMentionKey === mentionQueryKey)
        ? []
        : mentionOptions(mentionables, mentionMatch),
    [dismissedMentionKey, mentionMatch, mentionQueryKey, mentionables, suppressMentionSuggestions]
  );
  const activeOptionId =
    options.length > 0 ? `${mentionListId}-option-${activeOptionIndex}` : undefined;
  const controlledListId =
    options.length > 0 ? mentionListId : externalListOpen ? externalListId : undefined;
  const controlledOptionId =
    options.length > 0 ? activeOptionId : externalListOpen ? externalActiveOptionId : undefined;

  useEffect(() => {
    setActiveOptionIndex(0);
  }, [mentionQueryKey]);

  useEffect(() => {
    setMentionCursor((current) => Math.min(current, shown.length));
  }, [shown.length]);

  useEffect(() => {
    setActiveOptionIndex((current) => {
      if (options.length === 0) return 0;
      return Math.min(current, options.length - 1);
    });
  }, [options.length]);

  function syncMentionCursor() {
    setMentionCursor(targetRef.current?.selectionStart ?? shown.length);
  }

  function placeCaret(cursor: number) {
    window.setTimeout(() => {
      targetRef.current?.focus();
      targetRef.current?.setSelectionRange(cursor, cursor);
      setMentionCursor(cursor);
    }, 0);
  }

  function chooseMention(mentionable: Mentionable) {
    const cursor = targetRef.current?.selectionStart ?? shown.length;
    const query = mentionQueryAtCursor(shown, cursor);
    if (!query) return;
    // The typed `@query` becomes the bound token; the box shows `@Name `.
    const token = insertMentionText("", 0, { start: 0, query: query.query }, mentionable).message;
    const next = replaceDisplayRange(value, labels, query.start, cursor, token);
    const caret = draftDisplay(next.markup, mentionLabels([...mentionables, mentionable]))
      .text.length - (shown.length - cursor);
    setDismissedMentionKey("");
    setSuppressMentionSuggestions(true);
    setMentionCursor(caret);
    onChange(next.markup);
    placeCaret(caret);
  }

  function handleInputChange(event: ChangeEvent<HTMLTextAreaElement>) {
    setDismissedMentionKey("");
    setSuppressMentionSuggestions(false);
    const typed = event.target.selectionStart ?? event.target.value.length;
    const next = applyDisplayEdit(value, labels, event.target.value);
    setMentionCursor(next.cursor);
    onChange(next.markup);
    // Removing a whole mention changes more than the box did; keep the caret.
    if (next.cursor !== typed) placeCaret(next.cursor);
  }

  function handleMentionKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (options.length === 0) {
      onKeyDown?.(event);
      return;
    }

    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActiveOptionIndex((current) => (current + 1) % options.length);
      return;
    }

    if (event.key === "ArrowUp") {
      event.preventDefault();
      setActiveOptionIndex((current) => (current - 1 + options.length) % options.length);
      return;
    }

    if (event.key === "Enter" || event.key === "Tab") {
      event.preventDefault();
      const option = options[activeOptionIndex] || options[0];
      if (option) chooseMention(option);
      return;
    }

    if (event.key === "Escape") {
      event.preventDefault();
      setDismissedMentionKey(mentionQueryKey);
      return;
    }

    onKeyDown?.(event);
  }

  return (
    <>
      {options.length > 0 && (
        <div
          id={mentionListId}
          className="dc-mention-popover"
          role="listbox"
          aria-label="멘션 후보"
        >
          {options.map((option, index) => (
            <button
              key={option.token}
              id={`${mentionListId}-option-${index}`}
              type="button"
              onMouseDown={(event) => event.preventDefault()}
              onMouseEnter={() => setActiveOptionIndex(index)}
              onClick={() => chooseMention(option)}
              role="option"
              aria-selected={index === activeOptionIndex}
              aria-label={
                option.detail ? `${option.label}, ${option.detail}` : option.label
              }
            >
              <span
                className="dc-mention-avatar"
                data-participant-kind={option.participantKind || "unknown"}
              >
                {option.avatarImage ? (
                  <ResourceImage src={option.avatarImage} alt="" />
                ) : option.participantKind === "agent" ? (
                  <ProviderLogo
                    providerKind={option.providerKind}
                    size={32}
                    fallback={<Bot size={16} />}
                  />
                ) : (
                  <UserRound size={17} />
                )}
              </span>
              <span className="dc-mention-copy">
                <strong>{option.label}</strong>
                {option.detail && <small>{option.detail}</small>}
              </span>
            </button>
          ))}
        </div>
      )}
      <textarea
        ref={targetRef}
        value={shown}
        onChange={handleInputChange}
        onKeyDown={handleMentionKeyDown}
        onPaste={onPaste}
        onKeyUp={syncMentionCursor}
        onClick={syncMentionCursor}
        onSelect={syncMentionCursor}
        className={className}
        placeholder={placeholder}
        disabled={disabled}
        maxLength={maxLength}
        aria-label={ariaLabel}
        aria-autocomplete="list"
        aria-controls={controlledListId}
        aria-expanded={options.length > 0 || externalListOpen}
        aria-activedescendant={controlledOptionId}
        rows={1}
      />
    </>
  );
}
