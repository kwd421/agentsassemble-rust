/**
 * Where a floating overlay should render so a modal dialog cannot cover it.
 *
 * A dialog opened with showModal() draws in the browser's top layer, above everything in the
 * page, and marks the rest of the document inert. An overlay portalled to document.body from
 * inside such a dialog is drawn under it and cannot be clicked, so it belongs to the dialog.
 */
export function overlayHost(anchor: Element | null): HTMLElement {
  const dialog = anchor?.closest("dialog");
  return dialog instanceof HTMLDialogElement && dialog.open ? dialog : document.body;
}
