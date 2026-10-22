import { cleanMentionName } from "./mentionComposerModel";

/**
 * Display labels that are unique within a room: a name used once stays as it
 * is, and names shared by several participants get a number, "이름 (2)".
 * Numbers follow participant ID order, so every viewer sees the same number
 * for the same participant, and a number is never one another label already
 * uses (including `reserved`, such as the viewer's own name).
 */
export function distinctNames(
  entries: readonly { id: string; name: string }[],
  reserved: readonly string[] = [],
): Map<string, string> {
  const named = entries
    .map((entry) => ({ id: entry.id, name: cleanMentionName(entry.name) }))
    .sort((left, right) => (left.id < right.id ? -1 : left.id > right.id ? 1 : 0));
  const used = new Set(reserved.map((name) => cleanMentionName(name).toLowerCase()).filter(Boolean));
  const counts = new Map<string, number>();
  for (const { name } of named) {
    const key = name.toLowerCase();
    if (key) counts.set(key, (counts.get(key) || 0) + 1);
  }
  const labels = new Map<string, string>();
  // Unique names first, so a member literally named "이름 (2)" keeps it.
  for (const { id, name } of named) {
    const key = name.toLowerCase();
    if (key && counts.get(key) === 1 && !used.has(key)) {
      labels.set(id, name);
      used.add(key);
    }
  }
  for (const { id, name } of named) {
    if (labels.has(id) || !name) continue;
    let number = 1;
    while (used.has(`${name} (${number})`.toLowerCase())) number += 1;
    const label = `${name} (${number})`;
    labels.set(id, label);
    used.add(label.toLowerCase());
  }
  return labels;
}
