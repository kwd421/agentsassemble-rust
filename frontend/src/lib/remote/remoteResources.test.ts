import { afterEach, expect, it, vi } from "vitest";
import { createResourceUrl, readResourceBlob, revokeResourceUrl } from "./remoteResources";
import { closeRemoteWorkspace, installRemoteWorkspace } from "./remoteWorkspace";
import type { RemoteTransport } from "./remoteTransport";
afterEach(() => { closeRemoteWorkspace(); vi.restoreAllMocks(); });
it("bounds live resources across reads, retains in-use URLs and revokes them on close", async () => {
  const callbacks = new Set<() => void>();
  const remote = { hello: { origin: "https://host.test" }, active: true, onClose: (fn: () => void) => { callbacks.add(fn); return () => callbacks.delete(fn); },
    close: () => { remote.active = false; for (const fn of callbacks) fn(); } };
  installRemoteWorkspace({ transport: remote as unknown as RemoteTransport, owner: { sessionToken: "resource-owner" } as never, deviceToken: "device", clientId: "client" });
  vi.spyOn(URL, "createObjectURL").mockReturnValueOnce("blob:large").mockReturnValueOnce("blob:small");
  const revoke = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const large = createResourceUrl({ size: 64 * 1024 * 1024 } as Blob);
  await expect(readResourceBlob(new Response(new Uint8Array([1])))).rejects.toThrow("열려 있는 항목을 닫고");
  expect(revoke).not.toHaveBeenCalled();
  revokeResourceUrl(large);
  const small = createResourceUrl(await readResourceBlob(new Response(new Uint8Array([1]))));
  closeRemoteWorkspace();
  expect(revoke).toHaveBeenCalledWith(small);
});
