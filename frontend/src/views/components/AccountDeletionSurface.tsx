import "./AccountDeletionSurface.css";
import { closeRemoteWorkspace } from "../../lib/remote/remoteWorkspace";
import { centralAccountEntryUrl } from "../../lib/central/identity";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { AccountDeletionActiveContext, AccountDeletionContext } from "./AccountDeletionContext";
import { AccountDeletionFlow } from "./AccountDeletionSettings";

export default function AccountDeletionSurface({ children }: { children: ReactNode }) {
  const [completed, setCompleted] = useState(false);
  const [opened, setOpened] = useState(false);
  const [mounted, setMounted] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => { if (opened) dialog.current?.showModal(); else dialog.current?.close(); }, [opened]);
  const close = () => {
    setOpened(false);
    if (completed) window.location.replace(isDesktopWebview() ? "/" : centralAccountEntryUrl());
  };
  const requestClose = () => dialog.current?.querySelector<HTMLButtonElement>('[aria-label="계정 탈퇴 닫기"]')?.click();
  return <AccountDeletionContext.Provider value={() => { setMounted(true); setOpened(true); }}>
    <AccountDeletionActiveContext.Provider value={opened}>
      {completed ? <div className="fixed inset-0 bg-[#101114]" /> : children}
      {mounted && <dialog ref={dialog} aria-label="계정 탈퇴" aria-modal="true" className="dc-dialog dc-dialog--wide dc-account-deletion-dialog" onCancel={event => { event.preventDefault(); requestClose(); }} onClick={event => {
        const bounds = event.currentTarget.getBoundingClientRect();
        if (event.target === event.currentTarget && (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom)) requestClose();
      }}>
        <AccountDeletionFlow disabled={false} onClose={close} onDeleted={() => { setCompleted(true); closeRemoteWorkspace(); }} />
      </dialog>}
    </AccountDeletionActiveContext.Provider>
  </AccountDeletionContext.Provider>;
}
