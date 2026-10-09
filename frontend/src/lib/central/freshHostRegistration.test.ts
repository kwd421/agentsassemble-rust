import { afterEach, expect, it, vi } from "vitest";
import { registerFreshLocalHost } from "./freshHostRegistration";
import { fetchDesktopCentralRegistration, initializeDesktopBootstrap, requestDesktopBootstrapStatus } from "../desktopBridge";
vi.mock("../desktopBridge", () => ({ isDesktopWebview: () => true, fetchDesktopCentralRegistration: vi.fn(), initializeDesktopBootstrap: vi.fn(), requestDesktopBootstrapStatus: vi.fn(), fetchDesktopOperatorRuntime: vi.fn() }));
vi.mock("./identity", () => ({ loadCentralSession: () => ({ token: "fixture", person: { person_id: "new-person", display_name: "New Guest" } }), signedRequest: vi.fn() }));
vi.mock("../deviceIdentity", () => ({ rememberGuestProfile: vi.fn() }));
afterEach(() => vi.clearAllMocks());
it("keeps registration authority unavailable until explicit fresh-action bootstrap completes", async () => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase: "empty" } as never);
  let complete!: (value: never) => void;
  vi.mocked(initializeDesktopBootstrap).mockImplementation(() => new Promise(resolve => { complete = resolve; }));
  vi.mocked(fetchDesktopCentralRegistration).mockRejectedValue(new Error("registration probe"));
  const operation = registerFreshLocalHost("server", "").catch(error => error);
  await vi.waitFor(() => expect(initializeDesktopBootstrap).toHaveBeenCalledOnce());
  expect(fetchDesktopCentralRegistration).not.toHaveBeenCalled();
  complete({ phase: "complete", profile: { display_name: "New Guest", avatar_image_url: "" } } as never);
  expect((await operation).message).toBe("registration probe");
  expect(fetchDesktopCentralRegistration).toHaveBeenCalledOnce();
});
it("does not initialize or issue registration authority when local bootstrap needs repair", async () => {
  vi.mocked(requestDesktopBootstrapStatus).mockResolvedValue({ phase: "repair_required" } as never);
  await expect(registerFreshLocalHost("server", "")).rejects.toThrow("로컬 신원");
  expect(initializeDesktopBootstrap).not.toHaveBeenCalled();
  expect(fetchDesktopCentralRegistration).not.toHaveBeenCalled();
});
