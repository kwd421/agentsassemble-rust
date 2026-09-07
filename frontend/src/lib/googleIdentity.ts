export type GoogleIdentityApi = {
  initialize(options: { client_id: string; nonce: string; callback: (response: { credential?: string }) => void }): void;
  renderButton(target: HTMLElement, options: Record<string, string | number | boolean>): void;
  cancel(): void;
};
export function googleIdentityApi(): GoogleIdentityApi | undefined {
  return (window as typeof window & { google?: { accounts?: { id?: GoogleIdentityApi } } }).google?.accounts?.id;
}

let loading: Promise<void> | null = null;
export function loadGoogleIdentityScript(): Promise<void> {
  if (googleIdentityApi()) return Promise.resolve();
  if (loading) return loading;
  // This loader owns its single script and bounded load attempt. Failed nodes are removed
  // so an explicit retry creates a new request rather than waiting on an already-fired event.
  loading = new Promise<void>((resolve, reject) => {
    const script = document.createElement("script");
    const finish = (error?: Error) => {
      clearTimeout(timeout);
      script.onload = null;
      script.onerror = null;
      if (error) { script.remove(); reject(error); } else resolve();
    };
    const timeout = window.setTimeout(() => finish(new Error("Google 로그인을 불러오는 데 시간이 걸려요. 다시 시도해 주세요.")), 15_000);
    script.onload = () => finish(googleIdentityApi() ? undefined : new Error("Google 로그인 모듈을 사용할 수 없어요."));
    script.onerror = () => finish(new Error("Google 로그인을 불러오지 못했어요."));
    script.src = "https://accounts.google.com/gsi/client";
    script.referrerPolicy = "strict-origin-when-cross-origin";
    script.async = true;
    document.head.append(script);
  }).catch((error: unknown) => { loading = null; throw error; });
  return loading;
}
