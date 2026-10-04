import { useCallback } from "react";
import StartupIdentityGate from "./views/components/StartupIdentityGate";
import "./styles/componentOrder";
import AppView from "./app/AppView";
import { useAppController } from "./app/useAppController";
import FrontendUpdateNotice from "./views/components/FrontendUpdateNotice";
import { isDesktopWebview } from "./lib/desktopBridge";

export default function App({
  deviceToken,
  clientId,
}: {
  deviceToken: string;
  clientId: string;
}) {
  const controller = useAppController(deviceToken, clientId);
  const finishStartup = useCallback(() => { void controller.refreshCentralDirectory(); }, [controller.refreshCentralDirectory]);
  if (controller.centralDirectory?.status === "authentication-required") return <StartupIdentityGate deviceToken={deviceToken} onComplete={finishStartup} />;
  return <>
    {!isDesktopWebview() && <FrontendUpdateNotice connected={controller.canonicalRoom.connectionState === "connected"} />}
    <AppView controller={controller} />
  </>;
}
