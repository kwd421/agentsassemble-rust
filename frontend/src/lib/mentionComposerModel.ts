export type MentionQuery = {
  start: number;
  query: string;
};

export type Mentionable = {
  token: string;
  label: string;
  avatarImage?: string;
  detail?: string;
  participantKind?: "human" | "agent";
  providerKind?: string;
};

type MentionableInput = Mentionable | string;

function cleanMentionName(name: string) {
  return name
    .replace(/[\r\n<>]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function normalizedMentionable(value: MentionableInput): Mentionable | null {
  const token = cleanMentionName(typeof value === "string" ? value : value.token);
  const label = cleanMentionName(typeof value === "string" ? value : value.label) || token;
  if (!token) return null;
  if (typeof value === "string") return { token, label };
  return {
    ...value,
    token,
    label,
    detail: cleanMentionName(value.detail || "") || undefined,
  };
}

export function mentionQueryAtCursor(message: string, cursor = message.length): MentionQuery | null {
  const safeCursor = Math.max(0, Math.min(cursor, message.length));
  const beforeCursor = message.slice(0, safeCursor);
  const match = /(^|\s)@((?:[^\s@#:`*~<>\r\n][^@#:`*~<>\r\n]{0,47})?)$/u.exec(beforeCursor);
  if (!match) return null;
  return {
    start: beforeCursor.length - match[2].length - 1,
    query: match[2].replace(/\s+/g, " ").trim().toLowerCase(),
  };
}

export function mentionOptions(
  mentionables: MentionableInput[],
  query: MentionQuery | null,
  limit = 6
): Mentionable[] {
  if (!query) return [];
  const seen = new Set<string>();
  const options: Mentionable[] = [];
  for (const rawMentionable of mentionables) {
    const mentionable = normalizedMentionable(rawMentionable);
    if (!mentionable) continue;
    const key = mentionable.token.toLowerCase();
    const searchable = `${mentionable.label}\n${mentionable.token}`.toLowerCase();
    if (seen.has(key) || !searchable.includes(query.query)) continue;
    seen.add(key);
    options.push(mentionable);
    if (options.length >= limit) break;
  }
  return options;
}

export function formatMentionToken(value: MentionableInput): string {
  const mentionable = normalizedMentionable(value);
  if (!mentionable) return "@";
  if (typeof value !== "string") return `<@${mentionable.token}>`;
  if (/\s/u.test(mentionable.token)) return `<@${mentionable.token}>`;
  return `@${mentionable.token}`;
}

const MENTION_MARKUP_SPAN_PATTERN = /(<@[^>\r\n]{1,80}>)/g;

function escapeMentionPattern(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// The composer shows the readable name; the wire markup is resolved at send time
// so the visible draft never carries the raw token.
export function resolveMentionMarkup(message: string, mentionables: MentionableInput[]): string {
  const tokensByLabel = new Map<string, string>();
  for (const raw of mentionables) {
    const mentionable = normalizedMentionable(raw);
    if (!mentionable || mentionable.label === mentionable.token) continue;
    if (!tokensByLabel.has(mentionable.label)) tokensByLabel.set(mentionable.label, mentionable.token);
  }
  if (tokensByLabel.size === 0) return message;
  const labels = [...tokensByLabel.keys()]
    .sort((left, right) => right.length - left.length)
    .map(escapeMentionPattern);
  const pattern = new RegExp(`(^|\\s)@(${labels.join("|")})(?=$|[\\s.,!?;:)\\]}>*~\`#])`, "gu");
  return message
    .split(MENTION_MARKUP_SPAN_PATTERN)
    .map((part, index) => {
      if (index % 2 === 1) return part;
      return part.replace(pattern, (_match, prefix, label) => `${prefix}<@${tokensByLabel.get(label)}>`);
    })
    .join("");
}

export function insertMentionText(
  message: string,
  cursor: number,
  query: MentionQuery | null,
  mentionable: MentionableInput
): { message: string; cursor: number } {
  const safeCursor = Math.max(0, Math.min(cursor, message.length));
  const display = normalizedMentionable(mentionable);
  if (!query || !display) {
    return {
      message,
      cursor: safeCursor,
    };
  }
  // The composer shows the readable name; the wire markup is resolved at send time.
  const token = `@${display.label} `;
  const start = Math.max(0, Math.min(query.start, safeCursor));
  const nextMessage = `${message.slice(0, start)}${token}${message.slice(safeCursor)}`;
  return {
    message: nextMessage,
    cursor: start + token.length,
  };
}
