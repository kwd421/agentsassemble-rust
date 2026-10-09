import "./AccountDeletionSurface.css";
import { useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from "react";
import { AccountDeletionContext } from "./AccountDeletionContext";
import { AccountDeletionFlow } from "./AccountDeletionSettings";
import { clearDeletionGoogleReturn, deletionGoogleReturnSnapshot, subscribeDeletionGoogleReturn } from "../../lib/central/accountDeletionGoogle";

export default function AccountDeletionSurface({ children }: { children: ReactNode }) {
  const returned = useSyncExternalStore(subscribeDeletionGoogleReturn, deletionGoogleReturnSnapshot);
  const [opened, setOpened] = useState(false);
  const [everOpened, setEverOpened] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  const visible = opened || Boolean(returned);
  useEffect(() => { if (returned) setEverOpened(true); }, [returned]);
  useEffect(() => {
    if (!dialog.current) return;
    if (visible && !dialog.current.open) dialog.current.showModal();
    else if (!visible && dialog.current.open) dialog.current.close();
  }, [visible]);
  const close = () => { clearDeletionGoogleReturn(); setOpened(false); };
  return <AccountDeletionContext.Provider value={() => { setEverOpened(true); setOpened(true); }}>
    {children}
    {(everOpened || returned) && <dialog ref={dialog} aria-label="계정 탈퇴" aria-modal="true" className="dc-account-deletion-dialog" onCancel={event => event.preventDefault()}>
      <AccountDeletionFlow disabled={false} onClose={close} initial={returned} />
    </dialog>}
  </AccountDeletionContext.Provider>;
}
