type PrepareClipboardDispatch = () => Promise<() => void>;

export async function copyText(
  value: string,
  prepareDispatch?: PrepareClipboardDispatch
) {
  if (navigator.clipboard?.writeText) {
    const assertDispatch = await prepareDispatch?.();
    assertDispatch?.();
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // Browser permission rejection may still permit the synchronous fallback.
    }
  }
  const assertDispatch = await prepareDispatch?.();
  assertDispatch?.();
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "");
  textarea.style.position = "fixed";
  textarea.style.left = "-9999px";
  textarea.style.top = "0";
  textarea.style.opacity = "0";
  try {
    document.body.appendChild(textarea);
    textarea.focus({ preventScroll: true });
    textarea.select();
    textarea.setSelectionRange(0, value.length);
    assertDispatch?.();
    return document.execCommand("copy");
  } finally {
    textarea.remove();
  }
}
