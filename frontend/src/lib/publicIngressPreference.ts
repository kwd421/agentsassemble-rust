// Owner's last external-access ("외부 접속") choice, persisted locally by
// the bundled UI only: whether public ingress was open when the owner last
// left it. The desktop app reopens it automatically at the next start
// through the same local-operator start path the button uses. Only this
// one boolean persists here — never tokens, URLs, grants or keys.
const PUBLIC_INGRESS_OPEN_KEY = "agentsassemble.publicIngress.open.v1";

export const PUBLIC_INGRESS_PREFERENCE_OPEN = "open";
export const PUBLIC_INGRESS_PREFERENCE_CLOSED = "closed";

/** The remembered choice: true when the owner last left external access open. Absent or unreadable means closed, as today. */
export function loadPublicIngressPreference(): boolean {
  try {
    return window.localStorage.getItem(PUBLIC_INGRESS_OPEN_KEY) === PUBLIC_INGRESS_PREFERENCE_OPEN;
  } catch {
    return false;
  }
}

/** Records the owner's last open/closed choice. Failures (private mode, blocked storage) keep the previous choice. */
export function savePublicIngressPreference(open: boolean): void {
  try {
    window.localStorage.setItem(
      PUBLIC_INGRESS_OPEN_KEY,
      open ? PUBLIC_INGRESS_PREFERENCE_OPEN : PUBLIC_INGRESS_PREFERENCE_CLOSED,
    );
  } catch {
    // Storage unavailable: the choice simply does not persist.
  }
}
