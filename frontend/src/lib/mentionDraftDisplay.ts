import type { Mentionable } from "./mentionComposerModel";

/**
 * The composer keeps a picked mention as `<@id>` (bound to one participant)
 * but shows it the way Discord does, as `@Name`. A draft is the stored text;
 * its display swaps each known mention for its name and remembers where.
 */
export type DraftMention = { start: number; end: number; markupStart: number; markupEnd: number };
export type DraftDisplay = { text: string; mentions: DraftMention[] };

const MARKUP = /<@([^>\r\n]{1,80})>/g;

export function mentionLabels(mentionables: readonly Mentionable[]): ReadonlyMap<string, string> {
  const labels = new Map<string, string>();
  for (const { token, label } of mentionables) {
    const id = token.trim();
    if (id && !labels.has(id)) labels.set(id, label.replace(/[\r\n<>]/g, " ").trim() || id);
  }
  return labels;
}

/** Unknown ids stay as written, so nothing in the draft is lost. */
export function draftDisplay(markup: string, labels: ReadonlyMap<string, string>): DraftDisplay {
  let text = "";
  let last = 0;
  const mentions: DraftMention[] = [];
  for (const match of markup.matchAll(MARKUP)) {
    const label = labels.get(match[1].trim());
    if (!label) continue;
    text += markup.slice(last, match.index);
    const start = text.length;
    text += `@${label}`;
    mentions.push({ start, end: text.length, markupStart: match.index, markupEnd: match.index + match[0].length });
    last = match.index + match[0].length;
  }
  return { text: text + markup.slice(last), mentions };
}

/** A display position outside every mention, as a position in the stored text. */
function markupIndex(display: DraftDisplay, index: number): number {
  let delta = 0;
  for (const mention of display.mentions) {
    if (index < mention.end) break;
    delta = mention.markupEnd - mention.end;
  }
  return index + delta;
}

/**
 * Replaces display range [start, end) with `text`. An edit that touches a
 * mention removes all of it, the way Discord deletes a mention as one piece.
 * Returns the stored text and the caret in display positions.
 */
export function replaceDisplayRange(
  markup: string,
  labels: ReadonlyMap<string, string>,
  start: number,
  end: number,
  text: string,
): { markup: string; cursor: number } {
  const display = draftDisplay(markup, labels);
  let from = Math.max(0, Math.min(start, display.text.length));
  let to = Math.max(from, Math.min(end, display.text.length));
  for (const mention of display.mentions) {
    const touches = from === to ? from > mention.start && from < mention.end : from < mention.end && to > mention.start;
    if (touches) {
      from = Math.min(from, mention.start);
      to = Math.max(to, mention.end);
    }
  }
  return {
    markup: `${markup.slice(0, markupIndex(display, from))}${text}${markup.slice(markupIndex(display, to))}`,
    cursor: from + text.length,
  };
}

/** The textarea's new text, applied to the draft as one edit. */
export function applyDisplayEdit(
  markup: string,
  labels: ReadonlyMap<string, string>,
  nextText: string,
): { markup: string; cursor: number } {
  const previous = draftDisplay(markup, labels).text;
  const limit = Math.min(previous.length, nextText.length);
  let prefix = 0;
  while (prefix < limit && previous[prefix] === nextText[prefix]) prefix += 1;
  let suffix = 0;
  while (
    suffix < limit - prefix &&
    previous[previous.length - 1 - suffix] === nextText[nextText.length - 1 - suffix]
  ) suffix += 1;
  return replaceDisplayRange(
    markup,
    labels,
    prefix,
    previous.length - suffix,
    nextText.slice(prefix, nextText.length - suffix),
  );
}

/** Whether a display position sits inside or right after a mention. */
export function cursorInMention(display: DraftDisplay, cursor: number): boolean {
  return display.mentions.some((mention) => cursor > mention.start && cursor <= mention.end);
}
