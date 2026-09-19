import { afterEach, describe, expect, it } from "vitest";
import { overlayHost } from "./overlayHost";

afterEach(() => { document.body.innerHTML = ""; });

describe("overlayHost", () => {
  it("keeps an overlay inside the modal dialog that owns it", () => {
    const dialog = document.createElement("dialog");
    const anchor = document.createElement("div");
    dialog.append(anchor);
    document.body.append(dialog);
    // A closed dialog draws nothing, so its overlay belongs to the page.
    expect(overlayHost(anchor)).toBe(document.body);
    dialog.open = true;
    expect(overlayHost(anchor)).toBe(dialog);
  });

  it("falls back to the page for an anchor outside any dialog", () => {
    const plain = document.createElement("div");
    document.body.append(plain);
    expect(overlayHost(plain)).toBe(document.body);
    expect(overlayHost(null)).toBe(document.body);
  });
});
