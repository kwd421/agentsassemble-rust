import { useEffect, useState, type ReactNode } from "react";

export type ProviderSetupTone = "install" | "update" | "done" | "error" | "quiet";

// How long a finished install or update stays visible before it folds away.
export const SETUP_RESULT_VISIBLE_MS = 2600;
const SETUP_LEAVE_MS = 320;

/**
 * One row under the provider grid for a CLI's install, update and result states. The selected
 * provider chip above already names the provider, so the row carries only what changes.
 */
export default function ProviderSetupCard({ label, tone, glyph, busy = false, leaving = false, since,
  children, actions, detail }: {
  label: string;
  tone: ProviderSetupTone;
  glyph: ReactNode;
  busy?: boolean;
  leaving?: boolean;
  /** When set, the row shows how long the running step has taken. */
  since?: number;
  children: ReactNode;
  actions?: ReactNode;
  detail?: ReactNode;
}) {
  return <section className="dc-setup" aria-label={label} aria-busy={busy || undefined}
    data-tone={tone} data-busy={busy ? "true" : undefined} data-leaving={leaving ? "true" : undefined}>
    <div className="dc-setup-fold">
      <div className="dc-setup-inner">
        <div className="dc-setup-row">
          <span className="dc-setup-glyph" aria-hidden="true">{glyph}</span>
          <div className="dc-setup-main">{children}</div>
          {since !== undefined && <Elapsed since={since} />}
          {actions && <div className="dc-setup-actions">{actions}</div>}
        </div>
        {detail}
        {busy && <span className="dc-setup-progress" aria-hidden="true" />}
      </div>
    </div>
  </section>;
}

function Elapsed({ since }: { since: number }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  const seconds = Math.max(0, Math.floor((now - since) / 1000));
  return <span className="dc-setup-elapsed" aria-hidden="true">
    {Math.floor(seconds / 60)}:{String(seconds % 60).padStart(2, "0")}
  </span>;
}

/** Shows two versions with the unchanged leading segments dimmed, so the change reads at a glance. */
export function VersionShift({ from, to }: { from?: string; to: string }) {
  const next = to.split(".");
  const previous = from?.split(".") ?? [];
  let same = 0;
  while (same < next.length - 1 && next[same] === previous[same]) same += 1;
  const kept = next.slice(0, same).join(".");
  return <span className="dc-setup-versions">
    {from && <><span className="dc-setup-version-from">{from}</span><span aria-hidden="true">→</span></>}
    <span className="dc-setup-version-to">
      {kept && <span className="dc-setup-version-kept">{kept}.</span>}{next.slice(same).join(".")}
    </span>
  </span>;
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
