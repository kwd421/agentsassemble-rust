import { cleanup, render, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import ResourceImage from "./ResourceImage";
import { bindRemoteSession, isKnownHostOrigin, remoteTransportForOrigin } from "../../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../../lib/remote/remoteTransport";

const png = () => new Response(new Uint8Array([137, 80, 78, 71]), { headers: { "content-type": "image/png" } });

function memberTransport(origin: string) {
  const callbacks = new Set<() => void>();
  const transport = {
    hello: { origin },
    active: true,
    fetch: vi.fn(async (path: string) => {
      if (!transport.active) throw new Error("closed");
      return path.startsWith("/api/attachments/") ? png() : new Response("{}", { status: 404 });
    }),
    onClose: (fn: () => void) => { callbacks.add(fn); return () => callbacks.delete(fn); },
    close: () => { transport.active = false; for (const fn of callbacks) fn(); },
  };
  return transport;
}

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("loads a background member host image through its session transport, never by direct URL", async () => {
  // Attached but not open: the session is bound, no workspace installed.
  const origin = "https://member-background.test";
  const transport = memberTransport(origin);
  bindRemoteSession("member-session-token", transport as unknown as RemoteTransport, "friend");
  const plain = vi.fn(); vi.stubGlobal("fetch", plain);
  vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:member-image");
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const view = render(<ResourceImage src={`${origin}/api/attachments/abcdefgh?view=1`} alt="member avatar" />);
  await waitFor(() => expect(view.container.querySelector("img")?.getAttribute("src")).toBe("blob:member-image"));
  // Encrypted member transport carried it (relative path, no tunnel URL).
  expect(transport.fetch).toHaveBeenCalledWith("/api/attachments/abcdefgh?view=1", expect.anything());
  // Nothing ever fetched the tunnel URL directly (desktop CSP blocks it,
  // and it carries no auth).
  expect(plain).not.toHaveBeenCalled();
  transport.close();
});

it("renders nothing instead of a direct tunnel URL when the host transport is gone", async () => {
  const origin = "https://member-offline.test";
  const transport = memberTransport(origin);
  bindRemoteSession("member-offline-token", transport as unknown as RemoteTransport, "friend");
  transport.close();
  expect(remoteTransportForOrigin(origin)).toBeNull();
  expect(isKnownHostOrigin(origin)).toBe(true);
  const plain = vi.fn(); vi.stubGlobal("fetch", plain);
  const view = render(<ResourceImage src={`${origin}/api/attachments/abcdefgh?view=1`} alt="member avatar" />);
  // Settle any fetch attempt, then confirm no direct URL ever rendered.
  await waitFor(() => expect(transport.fetch).not.toHaveBeenCalled());
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(view.container.querySelector("img")?.getAttribute("src")).toBeNull();
  expect(plain).not.toHaveBeenCalled();
});

it("still renders foreign avatars directly", () => {
  expect(isKnownHostOrigin("https://lh3.googleusercontent.com")).toBe(false);
  const view = render(<ResourceImage src="https://lh3.googleusercontent.com/photo.jpg" alt="google avatar" />);
  expect(view.container.querySelector("img")?.getAttribute("src")).toBe("https://lh3.googleusercontent.com/photo.jpg");
});
