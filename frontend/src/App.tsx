import { hasRemoteWorkspace } from "./lib/remote/remoteWorkspace";
import { loadMemberAnchors } from "./lib/central/memberAnchors";
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
  const authRequired = controller.centralDirectory?.status === "authentication-required" && !controller.guestJoinToken && !controller.guestSession && !controller.ownerProfileSession;
  const ownerConflict = Boolean(controller.centralDirectory?.live?.owner_server_conflict);
  // A notice appears only when nothing at all can be opened. An own-server
  // mismatch never gates: it stays a quiet entry in the app shell.
  const personId = controller.centralDirectory?.person?.person_id ?? "";
  const hasOwned = (controller.centralDirectory?.servers || []).some((server) => server.relation === "owner");
  const hasMember = (controller.centralDirectory?.servers || []).some((server) => server.relation === "member");
  let hasAnchor = false;
  try {
    hasAnchor = Boolean(personId && loadMemberAnchors(personId).length > 0);
  } catch {
    hasAnchor = false;
  }
  const nothingOpenable = Boolean(
    controller.centralDirectory?.person && !ownerConflict && !controller.hostingRestriction &&
      controller.centralDirectory.status !== "central-unconfirmed" &&
      !hasOwned && !hasMember && !hasAnchor && controller.rooms.length === 0,
  );
  if (controller.hostingRestriction || authRequired || ownerConflict || nothingOpenable) return <StartupIdentityGate deviceToken={deviceToken} onComplete={finishStartup} />;
  return <>
    {!isDesktopWebview() && !hasRemoteWorkspace() && <FrontendUpdateNotice connected={controller.canonicalRoom.connectionState === "connected"} />}
    <AppView controller={controller} />
  </>;
}
