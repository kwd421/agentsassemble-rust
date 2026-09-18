import { useEffect, useState, type ReactNode } from "react";
import ProviderLogo from "./ProviderLogo";

export type ProviderSetupTone = "install" | "update" | "done" | "error";

// How long a finished install or update stays visible before it leaves the dialog.
export const SETUP_RESULT_VISIBLE_MS = 2600;
const SETUP_LEAVE_MS = 320;

/** The shared frame for a provider CLI's install, update and result states. */
export default function ProviderSetupCard({ providerId, headingId, title, tone, badge, busy = false, leaving = false,
  children, detail, actions }: {
  providerId: string;
  headingId: string;
  title: string;
  tone: ProviderSetupTone;
  badge: string;
  busy?: boolean;
  leaving?: boolean;
  children?: ReactNode;
  detail?: ReactNode;
  actions?: ReactNode;
}) {
  return <section className="dc-provider-setup" aria-labelledby={headingId} aria-busy={busy || undefined}
    data-tone={tone} data-busy={busy ? "true" : undefined} data-leaving={leaving ? "true" : undefined}>
    <div className="dc-provider-setup-head">
      <span className="dc-provider-setup-logo" aria-hidden="true"><ProviderLogo providerId={providerId} size={24} /></span>
      <div className="dc-provider-setup-body">
        <h3 id={headingId} className="dc-provider-setup-title">
          {title}<span className="dc-provider-setup-badge">{badge}</span>
        </h3>
        {children}
      </div>
    </div>
    {detail}
    {actions && <div className="dc-provider-setup-actions">{actions}</div>}
    {busy && <span className="dc-provider-setup-progress" aria-hidden="true" />}
  </section>;
}

/** Shows a finished result briefly, then reports leaving and finally gone. */
export function useTransientResult(active: boolean): "shown" | "leaving" | "gone" {
  const [stage, setStage] = useState<"shown" | "leaving" | "gone">("shown");
  useEffect(() => {
    if (!active) {
      setStage("shown");
      return;
    }
    const leave = window.setTimeout(() => setStage("leaving"), SETUP_RESULT_VISIBLE_MS);
    const gone = window.setTimeout(() => setStage("gone"), SETUP_RESULT_VISIBLE_MS + SETUP_LEAVE_MS);
    return () => { window.clearTimeout(leave); window.clearTimeout(gone); };
  }, [active]);
  return active ? stage : "shown";
}
