import { beforeEach, expect, it, vi } from "vitest";
import { finishGoogleVerification, GoogleRegistrationRequired } from "./googleRegistration";
import { loadCentralSession, saveSession, unsignedPost } from "./identity";
vi.mock("./identity", () => ({ loadCentralSession: vi.fn(), saveSession: vi.fn(), unsignedPost: vi.fn() }));
const body = { handoff_id: "goh_fixture", authorization_code: "4/fixture-code", code_verifier: "v".repeat(43) };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(loadCentralSession).mockReturnValue(null); });
it.each(["native", "web"] as const)("%s verification of deleted/absent identity never provisions before separate explicit registration", async flow => {
  for (const status of ["deleted", "absent"] as const) {
    let choice: GoogleRegistrationRequired | null = null;
    try { finishGoogleVerification({ status }, flow, body, Date.now() / 1000 + 300, null); }
    catch (reason) { if (reason instanceof GoogleRegistrationRequired) choice = reason; else throw reason; }
    expect(choice?.identityStatus).toBe(status); expect(unsignedPost).not.toHaveBeenCalled(); expect(saveSession).not.toHaveBeenCalled();
    vi.mocked(unsignedPost).mockResolvedValue({ status: "registered", login_required: true });
    await expect(choice!.register()).rejects.toThrow("다시 로그인");
    expect(vi.mocked(unsignedPost).mock.calls.at(-1)).toEqual([`/v1/auth/google/${flow}/register`, body, undefined]);
    expect(saveSession).not.toHaveBeenCalled(); vi.mocked(unsignedPost).mockClear();
  }
});
it("expiry and account change refuse registration before dispatch", async () => {
  const expired = new GoogleRegistrationRequired("deleted", "native", body, Date.now() / 1000 - 1, null);
  await expect(expired.register()).rejects.toThrow("만료");
  const active = new GoogleRegistrationRequired("absent", "native", body, Date.now() / 1000 + 300, null);
  vi.mocked(loadCentralSession).mockReturnValue({ token: "replacement" } as NonNullable<ReturnType<typeof loadCentralSession>>);
  await expect(active.register()).rejects.toThrow("계정이 바뀌었어요"); expect(unsignedPost).not.toHaveBeenCalled();
});

it("passes cancellation to registration and never saves a response after abort", async () => {
  const controller = new AbortController();
  const registration = new GoogleRegistrationRequired("absent", "web", body, Date.now() / 1000 + 300, null);
  let resolve!: (value: unknown) => void;
  vi.mocked(unsignedPost).mockImplementation(() => new Promise(done => { resolve = done; }));
  const result = registration.register(controller.signal);
  const rejected = expect(result).rejects.toMatchObject({ name: "AbortError" });
  expect(unsignedPost).toHaveBeenCalledWith("/v1/auth/google/web/register", body, controller.signal);
  controller.abort(); resolve({ status: "complete", person: {}, session: {} });
  await rejected; expect(saveSession).not.toHaveBeenCalled();
});
