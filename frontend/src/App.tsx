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
  memberReturn,
}: {
  deviceToken: string;
  clientId: string;
  memberReturn?: import("./lib/central/memberConnect").MemberReturn;
}) {
  const controller = useAppController(deviceToken, clientId, memberReturn);
  const finishStartup = useCallback(() => { void controller.refreshCentralDirectory(); }, [controller.refreshCentralDirectory]);
  if (controller.centralDirectory?.status === "authentication-required" && !controller.guestJoinToken) return <StartupIdentityGate deviceToken={deviceToken} onComplete={finishStartup} />;
  return <>
    {!isDesktopWebview() && <FrontendUpdateNotice connected={controller.canonicalRoom.connectionState === "connected"} />}
    <AppView controller={controller} />
  </>;
}
