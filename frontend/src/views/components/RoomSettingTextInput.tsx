import { useState } from "react";

// Keep an editing draft across canonical echoes. Commit when focus leaves rather than
// issuing a room command for every keystroke and replacing text with its ACK.
export default function RoomSettingTextInput({ value, normalize, onCommit }: {
  value: string;
  normalize: (value: string) => string;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState<string | null>(null);
  return <input className="ops-input" value={draft ?? value}
    onFocus={() => setDraft(value)}
    onChange={(event) => setDraft(normalize(event.target.value))}
    onBlur={() => {
      if (draft !== null && draft !== value) onCommit(draft);
      setDraft(null);
    }} />;
}
