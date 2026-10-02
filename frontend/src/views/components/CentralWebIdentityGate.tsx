import CentralServerList from "./CentralServerList";
import { useEffect, useRef, useState } from "react";
import { bootstrapCentral, loadCentralSession, logoutCentral, openCentralOwnedServer, type CentralBootstrap, type CentralServer } from "../../lib/centralIdentity";
import { prepareCentralWebGoogle } from "../../lib/centralWebGoogle";
import { googleIdentityApi, loadGoogleIdentityScript } from "../../lib/googleIdentity";

export default function CentralWebIdentityGate() {
  const [account, setAccount] = useState<CentralBootstrap | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState("");
  const [googleReady, setGoogleReady] = useState(false);
  const googleButton = useRef<HTMLDivElement>(null);
  const login = useRef<AbortController | null>(null);
  const operation = useRef(false);

  useEffect(() => {
    let active = true;
    void bootstrapCentral().then((value) => { if (active) setAccount(value); })
      .catch((reason: unknown) => { if (active) setError(message(reason)); })
      .finally(() => { if (active) setBusy(false); });
    return () => { active = false; login.current?.abort(); googleIdentityApi()?.cancel(); };
  }, []);

  function cancelLogin() {
    login.current?.abort();
    login.current = null;
    googleIdentityApi()?.cancel();
    googleButton.current?.replaceChildren();
    operation.current = false;
    setGoogleReady(false);
    setBusy(false);
  }

  async function startLogin() {
    if (operation.current) return;
    operation.current = true;
    setBusy(true); setError("");
    const controller = new AbortController();
    login.current = controller;
    try {
      const prepared = await prepareCentralWebGoogle(controller.signal);
      await loadGoogleIdentityScript();
      if (controller.signal.aborted) return;
      const google = googleIdentityApi();
      if (!google || !googleButton.current) throw new Error("Google 로그인을 표시하지 못했어요.");
      google.initialize({ client_id: prepared.clientId, nonce: prepared.nonce, callback: (response) => {
        if (controller.signal.aborted || operation.current) return;
        operation.current = true; setBusy(true); setError("");
        void (async () => {
          try {
            if (!response.credential) throw new Error("Google 로그인이 완료되지 않았어요.");
            await prepared.complete(response.credential);
            setAccount(await bootstrapCentral());
          } catch (reason) { setError(message(reason)); }
          finally { cancelLogin(); }
        })();
      } });
      google.renderButton(googleButton.current, { theme: "outline", size: "large", text: "continue_with", width: 320 });
      setGoogleReady(true);
    } catch (reason) {
      if (!controller.signal.aborted) setError(message(reason));
    } finally {
      if (!controller.signal.aborted) { operation.current = false; setBusy(false); }
    }
  }

  async function refresh() {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { setAccount(await bootstrapCentral()); }
    catch (reason) { setAccount(null); setError(message(reason)); }
    finally { operation.current = false; setBusy(false); }
  }

  async function open(server: CentralServer) {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await openCentralOwnedServer(server); }
    catch (reason) { setError(message(reason)); }
    finally { operation.current = false; setBusy(false); }
  }

  async function logout() {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await logoutCentral(); setAccount(null); }
    catch (reason) { setError(message(reason)); }
    finally { operation.current = false; setBusy(false); }
  }

  return <div className="fixed inset-0 z-[400] grid place-items-center overflow-auto bg-[#101114] p-5">
    <main aria-label="중앙 계정" aria-busy={busy} className="grid w-full max-w-[560px] gap-4 rounded-xl border border-white/10 bg-[#202126] p-6 text-text-primary shadow-2xl">
      <h1 className="text-2xl font-black">AgentsAssemble</h1>
      <p>{account ? `${account.person.display_name}님의 서버` : "Google 계정으로 로그인해 기존 서버와 방을 열어 보세요."}</p>
      {busy && <p role="status">확인 중…</p>}
      {error && <p role="alert" className="text-red-300">{error}</p>}
      {!account && <>
        {!googleReady && <button type="button" className="ops-button" disabled={busy} onClick={() => void startLogin()}>Google로 계속</button>}
        <div ref={googleButton} hidden={!googleReady} />
        {googleReady && <button type="button" className="ops-button" disabled={busy} onClick={cancelLogin}>로그인 취소</button>}
        {loadCentralSession() && <button type="button" className="ops-button" disabled={busy} onClick={() => void refresh()}>다시 확인</button>}
      </>}
      {account && <>
        {account.servers.length === 0 && <p>등록된 서버가 없습니다. 호스트 앱에서 같은 Google 계정으로 로그인하고 외부 접속을 열어 주세요.</p>}
        <CentralServerList key={account.person.person_id} servers={account.servers} busy={busy} onOpen={open} onRefresh={refresh} />
        <button type="button" className="ops-button" disabled={busy} onClick={() => void refresh()}>서버 목록 새로고침</button>
      </>}
      {loadCentralSession() && <button type="button" className="ops-button" disabled={busy} onClick={() => void logout()}>로그아웃</button>}
      <p className="text-sm text-text-muted">방의 대화와 파일은 해당 호스트에 보관됩니다. 호스트가 온라인이어야 접속할 수 있어요.</p>
    </main>
  </div>;
}

function message(reason: unknown): string {
  return reason instanceof Error ? reason.message : "요청을 완료하지 못했어요. 다시 시도해 주세요.";
}
