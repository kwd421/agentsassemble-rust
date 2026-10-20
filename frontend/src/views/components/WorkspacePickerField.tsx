import { useState } from "react";
import { Folder } from "../../ui/icons";
import { chooseLocalWorkspace } from "../../api";
import { userFacingError } from "../../lib/apiErrors";

function workspacePickerErrorMessage(error: unknown): string {
  return userFacingError(error, "작업 폴더를 선택하지 못했어요. 다시 시도해 주세요.");
}

export default function WorkspacePickerField({
  value,
  disabled = false,
  description = "",
  onChange,
  onError,
}: {
  value: string;
  disabled?: boolean;
  description?: string;
  onChange: (path: string) => void;
  onError: (message: string) => void;
}) {
  const [busy, setBusy] = useState(false);

  async function chooseWorkspace() {
    if (busy || disabled) return;
    setBusy(true);
    onError("");
    try {
      const selected = await chooseLocalWorkspace();
      if (selected.selected && selected.path) onChange(selected.path);
    } catch (error) {
      onError(workspacePickerErrorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <label className="dc-agent-field">
      <span>작업 폴더</span>
      <div className="dc-agent-folder-field">
        <Folder size={16} aria-hidden="true" />
        <input className="dc-input"
          aria-label="선택한 작업 폴더"
          value={value}
          placeholder="선택되지 않음"
          readOnly
        />
        <button type="button" disabled={busy || disabled} onClick={() => void chooseWorkspace()}>
          {busy ? "선택 중..." : "폴더 선택"}
        </button>
      </div>
      {description && <small>{description}</small>}
    </label>
  );
}
