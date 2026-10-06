import { afterEach, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import type { ReactNode } from "react";

const { draw } = vi.hoisted(() => ({ draw: vi.fn() }));
vi.mock("react-dom/client", () => ({ default: { createRoot: () => ({ render: draw }) } }));
vi.mock("./App", () => ({ default: () => <p>chat</p> }));
vi.mock("./views/components/StartupIdentityBoundary", () => ({ default: () => <p>identity gate</p> }));
vi.mock("./views/components/LocalAttendeePanel", () => ({ default: () => <p>local attendee setup</p> }));
vi.mock("./lib/central/memberConnect", () => ({ consumeMemberReturn: () => undefined,
  consumeCentralMemberRequest: () => undefined, clearCentralMemberRequest: vi.fn() }));
vi.mock("./lib/central/identity", () => ({ isCentralWebEntry: () => false }));

afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); draw.mockClear(); });

it.each([
  ["tauri://localhost/index.html?attendee-create=draft", true, "local attendee setup"],
  ["https://tauri.localhost/index.html?attendee-create=draft", true, "local attendee setup"],
  ["http://tauri.localhost/index.html?attendee-create=draft", true, "local attendee setup"],
  ["https://remote.example/?attendee-create=draft", true, "identity gate"],
  ["http://localhost/?attendee-create=draft", false, "identity gate"],
  ["tauri://localhost/index.html", true, "identity gate"],
  ["tauri://localhost/index.html?provider-setup=codex", true, "identity gate"],
])("routes %s with desktop=%s through only its own boundary", async (url, desktop, expected) => {
  const location = new URL(url);
  vi.stubGlobal("window", { location: { href: url, origin: url.startsWith("tauri:") ? "tauri://localhost" : location.origin },
    __TAURI_INTERNALS__: desktop ? {} : undefined });
  await import("./main");
  expect(renderToStaticMarkup(draw.mock.calls[0][0] as ReactNode)).toBe(`<p>${expected}</p>`);
});
